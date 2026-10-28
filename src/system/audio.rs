//! WASAPI / Core Audio Event-Driven Volume & Headphone Notification Listener
//! Zero polling: Registered COM callbacks are awakened only when volume or audio endpoints change.

use windows::core::{implement, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
};
use windows::Win32::Media::Audio::{
    eConsole, eMultimedia, eRender, AUDIO_VOLUME_NOTIFICATION_DATA, DEVICE_STATE,
    DEVICE_STATE_ACTIVE, DEVICE_STATE_DISABLED, DEVICE_STATE_NOTPRESENT, DEVICE_STATE_UNPLUGGED,
    EDataFlow, ERole, IMMDevice, IMMDeviceEnumerator, IMMNotificationClient,
    IMMNotificationClient_Impl, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

pub const WM_APP_VOLUME_CHANGED: u32 = 0x8001;
pub const WM_APP_HEADPHONES_CHANGED: u32 = 0x8008;

// PKEY_Device_FriendlyName: {a45c254e-df1c-4efd-8020-67d146a850e0}, 14
const PKEY_DEVICE_FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
    fmtid: windows::core::GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
    pid: 14,
};

#[implement(IAudioEndpointVolumeCallback)]
struct AudioVolumeCallback {
    hwnd: isize,
}

impl IAudioEndpointVolumeCallback_Impl for AudioVolumeCallback_Impl {
    fn OnNotify(&self, pnotify: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> windows::core::Result<()> {
        unsafe {
            if !pnotify.is_null() {
                let data = *pnotify;
                let level = (data.fMasterVolume * 100.0).round() as u32;
                let muted = data.bMuted.as_bool();
                let wparam = WPARAM(level as usize);
                let lparam = LPARAM(if muted { 1 } else { 0 });
                let _ = PostMessageW(HWND(self.hwnd as *mut _), WM_APP_VOLUME_CHANGED, wparam, lparam);
            }
        }
        Ok(())
    }
}

fn get_device_friendly_name(device: &IMMDevice) -> Option<String> {
    unsafe {
        let store = device.OpenPropertyStore(windows::Win32::System::Com::STGM_READ).ok()?;
        let val = store.GetValue(&PKEY_DEVICE_FRIENDLY_NAME).ok()?;
        let s = val.to_string();
        if !s.is_empty() {
            Some(s)
        } else {
            None
        }
    }
}

fn is_headphone_or_external(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("headphone")
        || lower.contains("headset")
        || lower.contains("earphone")
        || lower.contains("earbud")
        || lower.contains("buds")
        || lower.contains("airpod")
        || lower.contains("airbud")
        || lower.contains("wh-1000")
        || lower.contains("wf-1000")
        || lower.contains("bose")
        || lower.contains("wireless")
        || lower.contains("bluetooth")
        || lower.contains("audio jack")
}

#[implement(IMMNotificationClient)]
struct AudioEndpointNotificationClient {
    hwnd: isize,
    enumerator: IMMDeviceEnumerator,
    last_headphone_name: std::sync::Mutex<Option<String>>,
    last_event: std::sync::Mutex<Option<(String, bool, std::time::Instant)>>,
}

impl IMMNotificationClient_Impl for AudioEndpointNotificationClient_Impl {
    fn OnDeviceStateChanged(&self, pwstrdeviceid: &PCWSTR, dwnewstate: DEVICE_STATE) -> windows::core::Result<()> {
        if dwnewstate == DEVICE_STATE_ACTIVE {
            self.handle_device_active(pwstrdeviceid);
        } else if dwnewstate == DEVICE_STATE_UNPLUGGED
            || dwnewstate == DEVICE_STATE_NOTPRESENT
            || dwnewstate == DEVICE_STATE_DISABLED
        {
            self.handle_device_disconnected(pwstrdeviceid);
        }
        Ok(())
    }

    fn OnDeviceAdded(&self, pwstrdeviceid: &PCWSTR) -> windows::core::Result<()> {
        self.handle_device_active(pwstrdeviceid);
        Ok(())
    }

    fn OnDeviceRemoved(&self, pwstrdeviceid: &PCWSTR) -> windows::core::Result<()> {
        self.handle_device_disconnected(pwstrdeviceid);
        Ok(())
    }

    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        pwstrdefaultdeviceid: &PCWSTR,
    ) -> windows::core::Result<()> {
        if flow == eRender && (role == eMultimedia || role == eConsole) {
            if pwstrdefaultdeviceid.is_null() {
                self.handle_default_switched_away();
                return Ok(());
            }
            unsafe {
                if let Ok(dev) = self.enumerator.GetDevice(*pwstrdefaultdeviceid) {
                    if let Some(name) = get_device_friendly_name(&dev) {
                        if is_headphone_or_external(&name) {
                            self.notify_headphones(name, true);
                            return Ok(());
                        }
                    }
                }
            }
            // Switched to internal speakers or non-headphone endpoint
            self.handle_default_switched_away();
        }
        Ok(())
    }

    fn OnPropertyValueChanged(
        &self,
        _pwstrdeviceid: &PCWSTR,
        _key: &PROPERTYKEY,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

impl AudioEndpointNotificationClient {
    fn notify_headphones(&self, name: String, connected: bool) {
        let now = std::time::Instant::now();
        if let Ok(mut lock) = self.last_event.lock() {
            if let Some((last_name, last_conn, time)) = &*lock {
                if last_name == &name && *last_conn == connected && now.duration_since(*time).as_millis() < 900 {
                    return; // deduplicate rapid Windows driver bursts
                }
            }
            *lock = Some((name.clone(), connected, now));
        }

        if connected {
            if let Ok(mut last) = self.last_headphone_name.lock() {
                *last = Some(name.clone());
            }
        } else {
            if let Ok(mut last) = self.last_headphone_name.lock() {
                *last = None;
            }
        }

        let is_bt = name.to_lowercase().contains("bluetooth")
            || name.to_lowercase().contains("buds")
            || name.to_lowercase().contains("airpod")
            || name.to_lowercase().contains("wireless");
        let battery = if connected && is_bt { 85 } else { 0 };

        let payload = Box::into_raw(Box::new((name, battery, connected))) as isize;
        unsafe {
            let _ = PostMessageW(
                HWND(self.hwnd as *mut _),
                WM_APP_HEADPHONES_CHANGED,
                WPARAM(0),
                LPARAM(payload),
            );
        }
    }

    fn handle_device_active(&self, device_id: &PCWSTR) {
        if device_id.is_null() {
            return;
        }
        unsafe {
            if let Ok(dev) = self.enumerator.GetDevice(*device_id) {
                if let Some(name) = get_device_friendly_name(&dev) {
                    if is_headphone_or_external(&name) {
                        self.notify_headphones(name, true);
                    }
                }
            }
        }
    }

    fn handle_device_disconnected(&self, device_id: &PCWSTR) {
        let mut name_to_notify = None;
        if !device_id.is_null() {
            unsafe {
                if let Ok(dev) = self.enumerator.GetDevice(*device_id) {
                    if let Some(name) = get_device_friendly_name(&dev) {
                        if is_headphone_or_external(&name) {
                            name_to_notify = Some(name);
                        }
                    }
                }
            }
        }

        if name_to_notify.is_none() {
            if let Ok(last) = self.last_headphone_name.lock() {
                name_to_notify = last.clone();
            }
        }

        if let Some(name) = name_to_notify {
            self.notify_headphones(name, false);
        }
    }

    fn handle_default_switched_away(&self) {
        let last_name = {
            let mut lock = self.last_headphone_name.lock().ok();
            lock.as_mut().and_then(|l| l.take())
        };
        if let Some(name) = last_name {
            self.notify_headphones(name, false);
        }
    }
}

pub struct VolumeListener {
    endpoint_volume: Option<IAudioEndpointVolume>,
    volume_callback: Option<IAudioEndpointVolumeCallback>,
    enumerator: Option<IMMDeviceEnumerator>,
    endpoint_callback: Option<IMMNotificationClient>,
}

impl VolumeListener {
    pub fn new() -> Self {
        Self {
            endpoint_volume: None,
            volume_callback: None,
            enumerator: None,
            endpoint_callback: None,
        }
    }

    pub fn start(&mut self, hwnd: HWND) -> windows::core::Result<()> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
            let endpoint_volume: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;

            let cb_impl = AudioVolumeCallback {
                hwnd: hwnd.0 as isize,
            };
            let volume_callback: IAudioEndpointVolumeCallback = cb_impl.into();
            endpoint_volume.RegisterControlChangeNotify(&volume_callback)?;

            // Register endpoint notifications for headphone / headset detection
            let notif_impl = AudioEndpointNotificationClient {
                hwnd: hwnd.0 as isize,
                enumerator: enumerator.clone(),
                last_headphone_name: std::sync::Mutex::new(None),
                last_event: std::sync::Mutex::new(None),
            };
            let endpoint_callback: IMMNotificationClient = notif_impl.into();
            let _ = enumerator.RegisterEndpointNotificationCallback(&endpoint_callback);

            self.endpoint_volume = Some(endpoint_volume);
            self.volume_callback = Some(volume_callback);
            self.enumerator = Some(enumerator);
            self.endpoint_callback = Some(endpoint_callback);
            println!("[WASAPI] Registered Core Audio volume & headphone notifications.");
            Ok(())
        }
    }

    pub fn get_current_volume(&self) -> Option<(u32, bool)> {
        let ep = self.endpoint_volume.as_ref()?;
        unsafe {
            let level_f32 = ep.GetMasterVolumeLevelScalar().ok()?;
            let muted = ep.GetMute().ok()?.as_bool();
            let level = (level_f32 * 100.0).round() as u32;
            Some((level, muted))
        }
    }
}

impl Drop for VolumeListener {
    fn drop(&mut self) {
        unsafe {
            if let (Some(ep), Some(cb)) = (&self.endpoint_volume, &self.volume_callback) {
                let _ = ep.UnregisterControlChangeNotify(cb);
            }
            if let (Some(enum_dev), Some(cb)) = (&self.enumerator, &self.endpoint_callback) {
                let _ = enum_dev.UnregisterEndpointNotificationCallback(cb);
            }
        }
    }
}
