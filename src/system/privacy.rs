//! Privacy & Hardware Sensor Monitor (Camera & Microphone In-Use Detection + Quick Mic Mute)
//! Checks Windows CapabilityAccessManager ConsentStore keys for real-time camera and microphone activity.
//! Provides instantaneous system-level microphone mute/unmute via Windows Core Audio.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_READ, REG_VALUE_TYPE,
};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, IMMDeviceEnumerator, MMDeviceEnumerator,
    Endpoints::IAudioEndpointVolume,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};

pub const WM_APP_PRIVACY_CHANGED: u32 = 0x800A;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrivacyStatus {
    pub camera_active: bool,
    pub camera_app: String,
    pub mic_active: bool,
    pub mic_app: String,
    pub mic_muted: bool,
}

impl PrivacyStatus {
    pub fn query() -> Self {
        let (cam_active, cam_app) = check_capability(w!("Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\webcam"));
        let (mic_active, mic_app) = check_capability(w!("Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone"));
        let mic_muted = if mic_active {
            query_default_mic_muted().unwrap_or(false)
        } else {
            false
        };

        Self {
            camera_active: cam_active,
            camera_app: cam_app,
            mic_active,
            mic_app,
            mic_muted,
        }
    }
}

fn check_capability(sub_key: PCWSTR) -> (bool, String) {
    unsafe {
        let mut h_root = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, sub_key, 0, KEY_READ, &mut h_root).is_err() {
            return (false, String::new());
        }

        let mut is_active = false;
        let mut active_app = String::new();

        // 1. Check direct subkeys (packaged apps)
        scan_subkeys_for_active(h_root, &mut is_active, &mut active_app);

        // 2. Check "NonPackaged" subkeys (Win32 apps like Discord, Chrome, OBS, Zoom)
        if !is_active {
            let mut h_nonpackaged = HKEY::default();
            if RegOpenKeyExW(h_root, w!("NonPackaged"), 0, KEY_READ, &mut h_nonpackaged).is_ok() {
                scan_subkeys_for_active(h_nonpackaged, &mut is_active, &mut active_app);
                let _ = RegCloseKey(h_nonpackaged);
            }
        }

        let _ = RegCloseKey(h_root);
        (is_active, active_app)
    }
}

unsafe fn scan_subkeys_for_active(h_parent: HKEY, is_active: &mut bool, active_app: &mut String) {
    let mut index = 0u32;
    let mut name_buf = [0u16; 512];

    loop {
        let mut name_len = name_buf.len() as u32;
        let enum_res = unsafe {
            RegEnumKeyExW(
                h_parent,
                index,
                windows::core::PWSTR(name_buf.as_mut_ptr()),
                &mut name_len,
                None,
                windows::core::PWSTR::null(),
                None,
                None,
            )
        };
        if enum_res.is_err() {
            break;
        }

        let subkey_name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
        if subkey_name == "NonPackaged" {
            index += 1;
            continue;
        }

        // Open this app's subkey
        let mut h_app = HKEY::default();
        let sub_pcwstr = windows::core::HSTRING::from(&subkey_name);
        let open_res = unsafe { RegOpenKeyExW(h_parent, PCWSTR(sub_pcwstr.as_ptr()), 0, KEY_READ, &mut h_app) };
        if open_res.is_ok() {
            let mut stop_time: u64 = 0;
            let mut data_len = std::mem::size_of::<u64>() as u32;
            let mut val_type = REG_VALUE_TYPE::default();

            let query_res = unsafe {
                RegQueryValueExW(
                    h_app,
                    w!("LastUsedTimeStop"),
                    None,
                    Some(&mut val_type),
                    Some(&mut stop_time as *mut u64 as *mut u8),
                    Some(&mut data_len),
                )
            };

            if query_res.is_ok() {
                if stop_time == 0 {
                    // Active!
                    *is_active = true;
                    *active_app = format_app_name(&subkey_name);
                    unsafe { let _ = RegCloseKey(h_app); }
                    break;
                }
            }
            unsafe { let _ = RegCloseKey(h_app); }
        }

        index += 1;
    }
}

fn format_app_name(raw: &str) -> String {
    // If Win32 path with # separators, e.g. "C:#Program Files#Google#Chrome#Application#chrome.exe"
    if raw.contains('#') {
        let last_part = raw.split('#').last().unwrap_or(raw);
        let name_without_ext = last_part.strip_suffix(".exe").unwrap_or(last_part);
        return match name_without_ext.to_lowercase().as_str() {
            "chrome" => "Google Chrome".to_string(),
            "brave" => "Brave".to_string(),
            "msedge" => "Microsoft Edge".to_string(),
            "discord" => "Discord".to_string(),
            "zoom" => "Zoom".to_string(),
            "obs64" | "obs" => "OBS Studio".to_string(),
            "powerpnt" => "PowerPoint".to_string(),
            "whatsapp" => "WhatsApp".to_string(),
            _ => {
                let mut c = name_without_ext.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                }
            }
        };
    }

    // Packaged UWP app, e.g. "Microsoft.WindowsCamera_8wekyb3d8bbwe"
    if raw.contains("WindowsCamera") {
        return "Camera".to_string();
    }
    if raw.contains("WhatsApp") {
        return "WhatsApp".to_string();
    }
    if raw.contains("Discord") {
        return "Discord".to_string();
    }

    raw.split('_').next().unwrap_or(raw).to_string()
}

pub fn query_default_mic_muted() -> Option<bool> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let device = enumerator.GetDefaultAudioEndpoint(eCapture, eCommunications).ok()?;
        let endpoint_vol: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None).ok()?;
        let muted = endpoint_vol.GetMute().ok()?.as_bool();
        Some(muted)
    }
}

pub fn set_default_mic_mute(mute: bool) -> bool {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        if let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) {
            if let Ok(device) = enumerator.GetDefaultAudioEndpoint(eCapture, eCommunications) {
                if let Ok(endpoint_vol) = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) {
                    return endpoint_vol.SetMute(mute, std::ptr::null()).is_ok();
                }
            }
        }
        false
    }
}

pub fn toggle_default_mic_mute() -> Option<bool> {
    let current = query_default_mic_muted()?;
    let new_state = !current;
    if set_default_mic_mute(new_state) {
        Some(new_state)
    } else {
        None
    }
}

pub struct PrivacyListener {
    running: Arc<AtomicBool>,
}

impl PrivacyListener {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start(&mut self, hwnd: HWND) {
        self.running.store(true, Ordering::SeqCst);
        let running = Arc::clone(&self.running);
        let hwnd_val = hwnd.0 as usize;

        thread::Builder::new()
            .name("privacy-monitor".to_string())
            .spawn(move || {
                unsafe {
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                }
                let hwnd = HWND(hwnd_val as *mut _);
                let mut prev_status = PrivacyStatus::default();

                while running.load(Ordering::SeqCst) {
                    let cur_status = PrivacyStatus::query();
                    if cur_status != prev_status {
                        prev_status = cur_status.clone();
                        let wparam = WPARAM(
                            (if cur_status.camera_active { 1 } else { 0 })
                            | (if cur_status.mic_active { 2 } else { 0 })
                            | (if cur_status.mic_muted { 4 } else { 0 })
                        );
                        unsafe {
                            let _ = PostMessageW(hwnd, WM_APP_PRIVACY_CHANGED, wparam, LPARAM(0));
                        }
                    }
                    thread::sleep(Duration::from_millis(1000));
                }
                unsafe {
                    windows::Win32::System::Com::CoUninitialize();
                }
            })
            .expect("Failed to spawn privacy monitor thread");
    }

    #[allow(dead_code)]
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
    }
}
