#![allow(dead_code)]

//! Event-Driven Power and Battery Source Listener
//! Uses RegisterPowerSettingNotification to receive WM_POWERBROADCAST on charger events,
//! and runs a lightweight background heartbeat thread to guarantee live battery updates.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use windows::core::GUID;
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::System::Power::{
    GetSystemPowerStatus, RegisterPowerSettingNotification, HPOWERNOTIFY, SYSTEM_POWER_STATUS,
};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, DEVICE_NOTIFY_WINDOW_HANDLE};

pub const WM_APP_POWER_CHANGED: u32 = 0x8003;

// GUID_ACDC_POWER_SOURCE: 5d3e9a59-e9d5-4b00-ac24-aad55b705571
pub const GUID_ACDC_POWER_SOURCE: GUID = GUID::from_u128(0x5d3e9a59_e9d5_4b00_ac24_aad55b705571);
// GUID_POWER_SAVING_STATUS: e00958d0-c213-4ace-ac77-fc0d7b03865d
pub const GUID_POWER_SAVING_STATUS: GUID = GUID::from_u128(0xe00958d0_c213_4ace_ac77_fc0d7b03865d);
// GUID_BATTERY_PERCENTAGE_REMAINING: a7ad8041-b45a-4cae-87a3-eecbb468a9e1
pub const GUID_BATTERY_PERCENTAGE_REMAINING: GUID = GUID::from_u128(0xa7ad8041_b45a_4cae_87a3_eecbb468a9e1);

pub struct PowerListener {
    notify_acdc: Option<HPOWERNOTIFY>,
    notify_saver: Option<HPOWERNOTIFY>,
    notify_bat: Option<HPOWERNOTIFY>,
    running: Arc<AtomicBool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
            notify_bat: None,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start(&mut self, hwnd: HWND) {
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

            let handle_bat = RegisterPowerSettingNotification(
                HANDLE(hwnd.0),
                &GUID_BATTERY_PERCENTAGE_REMAINING,
                DEVICE_NOTIFY_WINDOW_HANDLE,
            );
            if let Ok(h) = handle_bat {
                self.notify_bat = Some(h);
                println!("[Power] Registered Battery Percentage setting notification.");
            }
        }

        // Background heartbeat: polls GetSystemPowerStatus every 1.5s to ensure live battery percentage
        // (Windows OS power notifications can be delayed or throttled by OEM power drivers)
        self.running.store(true, Ordering::SeqCst);
        let running = Arc::clone(&self.running);
        let hwnd_val = hwnd.0 as usize;

        thread::Builder::new()
            .name("power-monitor".to_string())
            .spawn(move || {
                let hwnd = HWND(hwnd_val as *mut _);
                let mut last_state = Self::query_power_status();

                while running.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(1500));
                    let cur_state = Self::query_power_status();
                    if cur_state != last_state {
                        last_state = cur_state;
                        unsafe {
                            let _ = PostMessageW(hwnd, WM_APP_POWER_CHANGED, WPARAM(0), LPARAM(0));
                        }
                    }
                }
            })
            .expect("Failed to spawn power monitor thread");
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
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
