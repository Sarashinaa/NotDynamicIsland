//! Real-Time Network Transfer Speed Monitor
//! Uses Windows IP Helper API (GetIfTable2) to measure aggregate network download & upload throughput.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

pub const WM_APP_NETWORK_CHANGED: u32 = 0x800B;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NetworkSpeed {
    pub download_bytes_sec: u64,
    pub upload_bytes_sec: u64,
}

impl NetworkSpeed {
    pub fn format_download(&self) -> String {
        format_rate(self.download_bytes_sec)
    }

    #[allow(dead_code)]
    pub fn format_upload(&self) -> String {
        format_rate(self.upload_bytes_sec)
    }
}

fn format_rate(bytes_sec: u64) -> String {
    if bytes_sec >= 1_000_000_000 {
        format!("{:.1} GB/s", bytes_sec as f64 / 1_000_000_000.0)
    } else if bytes_sec >= 1_000_000 {
        format!("{:.1} MB/s", bytes_sec as f64 / 1_000_000.0)
    } else if bytes_sec >= 1_000 {
        format!("{:.0} KB/s", bytes_sec as f64 / 1_000.0)
    } else {
        "0 KB/s".to_string()
    }
}

pub fn query_total_octets() -> (u64, u64) {
    unsafe {
        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut table).is_err() || table.is_null() {
            return (0, 0);
        }

        let num_entries = (*table).NumEntries as usize;
        let entries_slice = std::slice::from_raw_parts((*table).Table.as_ptr(), num_entries);

        let mut total_in = 0u64;
        let mut total_out = 0u64;

        for entry in entries_slice {
            // Check if operational and not loopback (IF_TYPE_SOFTWARE_LOOPBACK = 24)
            if entry.OperStatus.0 == 1 && entry.Type != 24 {
                total_in += entry.InOctets;
                total_out += entry.OutOctets;
            }
        }

        FreeMibTable(table as *const std::ffi::c_void);
        (total_in, total_out)
    }
}

pub struct NetworkListener {
    running: Arc<AtomicBool>,
    speed: Arc<Mutex<NetworkSpeed>>,
}

impl NetworkListener {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            speed: Arc::new(Mutex::new(NetworkSpeed::default())),
        }
    }

    pub fn get_speed(&self) -> NetworkSpeed {
        self.speed.lock().map(|s| *s).unwrap_or_default()
    }

    pub fn start(&mut self, hwnd: HWND) {
        self.running.store(true, Ordering::SeqCst);
        let running = Arc::clone(&self.running);
        let speed_arc = Arc::clone(&self.speed);
        let hwnd_val = hwnd.0 as usize;

        thread::Builder::new()
            .name("network-monitor".to_string())
            .spawn(move || {
                let hwnd = HWND(hwnd_val as *mut _);
                let (mut prev_in, mut prev_out) = query_total_octets();
                let mut prev_time = Instant::now();

                while running.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(1000));
                    let now = Instant::now();
                    let dt = now.duration_since(prev_time).as_secs_f64().max(0.1);
                    prev_time = now;

                    let (cur_in, cur_out) = query_total_octets();
                    let in_diff = cur_in.saturating_sub(prev_in);
                    let out_diff = cur_out.saturating_sub(prev_out);
                    prev_in = cur_in;
                    prev_out = cur_out;

                    let down_rate = (in_diff as f64 / dt).round() as u64;
                    let up_rate = (out_diff as f64 / dt).round() as u64;

                    let new_speed = NetworkSpeed {
                        download_bytes_sec: down_rate,
                        upload_bytes_sec: up_rate,
                    };

                    if let Ok(mut lock) = speed_arc.lock() {
                        *lock = new_speed;
                    }

                    // Wake animation only if network is actively transferring (> 100 KB/s)
                    if down_rate > 100_000 || up_rate > 100_000 {
                        unsafe {
                            let _ = PostMessageW(hwnd, WM_APP_NETWORK_CHANGED, WPARAM(0), LPARAM(0));
                        }
                    }
                }
            })
            .expect("Failed to spawn network monitor thread");
    }

    #[allow(dead_code)]
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
    }
}
