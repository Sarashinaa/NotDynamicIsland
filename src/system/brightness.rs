#![allow(dead_code)]

//! Event-Driven Display Brightness Listener
//! Uses RegisterPowerSettingNotification to receive WM_POWERBROADCAST on brightness changes.

use windows::core::GUID;
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::System::Power::{RegisterPowerSettingNotification, HPOWERNOTIFY};
use windows::Win32::UI::WindowsAndMessaging::DEVICE_NOTIFY_WINDOW_HANDLE;

pub const WM_APP_BRIGHTNESS_CHANGED: u32 = 0x8007;

// GUID_DEVICE_POWER_POLICY_VIDEO_BRIGHTNESS: aded5e82-b909-4619-9949-f5d71dac0bcb
pub const GUID_DEVICE_POWER_POLICY_VIDEO_BRIGHTNESS: GUID =
    GUID::from_u128(0xaded5e82_b909_4619_9949_f5d71dac0bcb);

pub struct BrightnessListener {
    notify_handle: Option<HPOWERNOTIFY>,
}

impl BrightnessListener {
    pub fn new() -> Self {
        Self { notify_handle: None }
    }

    pub fn register(&mut self, hwnd: HWND) {
        unsafe {
            let handle = RegisterPowerSettingNotification(
                HANDLE(hwnd.0),
                &GUID_DEVICE_POWER_POLICY_VIDEO_BRIGHTNESS,
                DEVICE_NOTIFY_WINDOW_HANDLE,
            );
            if let Ok(h) = handle {
                self.notify_handle = Some(h);
                println!("[Brightness] Registered video brightness power setting notification.");
            }
        }
    }
}
