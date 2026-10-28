#![allow(dead_code)]

//! Event-Driven Power and Battery Source Listener
//! Uses RegisterPowerSettingNotification to receive WM_POWERBROADCAST on charger events.

use windows::core::GUID;
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::System::Power::{
    GetSystemPowerStatus, RegisterPowerSettingNotification, HPOWERNOTIFY, SYSTEM_POWER_STATUS,
};

pub const WM_APP_POWER_CHANGED: u32 = 0x8003;

// GUID_ACDC_POWER_SOURCE: 5d3e9a59-e9d5-4b00-ac24-aad55b705571
pub const GUID_ACDC_POWER_SOURCE: GUID = GUID::from_u128(0x5d3e9a59_e9d5_4b00_ac24_aad55b705571);
// GUID_POWER_SAVING_STATUS: e00958d0-c213-4ace-ac77-fc0d7b03865d
pub const GUID_POWER_SAVING_STATUS: GUID = GUID::from_u128(0xe00958d0_c213_4ace_ac77_fc0d7b03865d);

use windows::Win32::UI::WindowsAndMessaging::DEVICE_NOTIFY_WINDOW_HANDLE;

pub struct PowerListener {
    notify_acdc: Option<HPOWERNOTIFY>,
    notify_saver: Option<HPOWERNOTIFY>,
}

#[derive(Clone, Copy, Debug)]
pub struct PowerState {
    pub is_charging: bool,
    pub is_energy_saver: bool,
    pub battery_percent: u32,
    pub has_battery: bool,
}

impl PowerListener {
    pub fn new() -> Self {
        Self {
            notify_acdc: None,
            notify_saver: None,
        }
    }

    pub fn register(&mut self, hwnd: HWND) {
        unsafe {
            let handle_acdc = RegisterPowerSettingNotification(
                HANDLE(hwnd.0),
                &GUID_ACDC_POWER_SOURCE,
                DEVICE_NOTIFY_WINDOW_HANDLE,
            );
            if let Ok(h) = handle_acdc {
                self.notify_acdc = Some(h);
                println!("[Power] Registered ACDC power setting notification.");
            }

            let handle_saver = RegisterPowerSettingNotification(
                HANDLE(hwnd.0),
                &GUID_POWER_SAVING_STATUS,
                DEVICE_NOTIFY_WINDOW_HANDLE,
            );
            if let Ok(h) = handle_saver {
                self.notify_saver = Some(h);
                println!("[Power] Registered Energy Saver setting notification.");
            }
        }
    }

    pub fn query_power_status() -> PowerState {
        unsafe {
            let mut sps = SYSTEM_POWER_STATUS::default();
            if GetSystemPowerStatus(&mut sps).is_ok() {
                let is_charging = sps.ACLineStatus == 1;
                let is_energy_saver = sps.SystemStatusFlag == 1;
                let battery_percent = if sps.BatteryLifePercent <= 100 {
                    sps.BatteryLifePercent as u32
                } else {
                    100
                };
                let has_battery = sps.BatteryFlag != 128 && sps.BatteryFlag != 255;
                PowerState {
                    is_charging,
                    is_energy_saver,
                    battery_percent,
                    has_battery,
                }
            } else {
                PowerState {
                    is_charging: true,
                    is_energy_saver: false,
                    battery_percent: 100,
                    has_battery: false,
                }
            }
        }
    }
}
