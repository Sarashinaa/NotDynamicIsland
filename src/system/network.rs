//! Real-Time Network Transfer Speed Monitor
//! Uses Windows IP Helper API (GetIfTable2) to measure aggregate network download & upload throughput.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, GetIpAddrTable, MIB_IF_TABLE2, MIB_IPADDRROW_XP, MIB_IPADDRTABLE,
};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

pub const WM_APP_NETWORK_CHANGED: u32 = 0x800B;
pub const WM_APP_TIME_TICK: u32 = 0x800E;
pub const WM_APP_NETWORK_TOPOLOGY_CHANGED: u32 = 0x8018;

/// Minimum download throughput threshold required to replace the date display in the Dynamic Island
/// 25 Megabits per second = 3,125,000 bytes/sec (~3.125 MB/s)
pub const MIN_DOWNLOAD_THRESHOLD_BYTES: u64 = 3_125_000;

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

/// Computes an order-independent fingerprint of active non-loopback, non-APIPA IPv4 addresses.
/// Returns 0 when no routable IPv4 interface is active.
pub fn query_ipv4_fingerprint() -> u64 {
    unsafe {
        let mut size: u32 = 0;
        let _ = GetIpAddrTable(None, &mut size, false);
        if size == 0 || size > 65536 {
            return 0;
        }

        let mut buf = vec![0u32; (size as usize + 3) / 4];
        let mut actual_size = (buf.len() * 4) as u32;
        let table_ptr = buf.as_mut_ptr() as *mut MIB_IPADDRTABLE;
        if GetIpAddrTable(Some(table_ptr), &mut actual_size, false) != 0 {
            return 0;
        }

        let count = (*table_ptr).dwNumEntries as usize;
        let header_bytes = std::mem::offset_of!(MIB_IPADDRTABLE, table);
        if header_bytes + count.saturating_mul(std::mem::size_of::<MIB_IPADDRROW_XP>()) > buf.len() * 4 {
            return 0;
        }

        let rows = std::slice::from_raw_parts((*table_ptr).table.as_ptr(), count);
        let mut addrs: Vec<(u32, u32)> = Vec::new();
        for row in rows {
            if row.dwAddr == 0 {
                continue;
            }
            let [b0, b1, _, _] = row.dwAddr.to_ne_bytes();
            if b0 == 127 || (b0 == 169 && b1 == 254) {
                continue;
            }
            addrs.push((row.dwAddr, row.dwMask));
        }

        if addrs.is_empty() {
            return 0;
        }
        addrs.sort_unstable();

        let mut hash: u64 = 0xcbf29ce484222325;
        for (addr, mask) in addrs {
            for b in addr.to_ne_bytes() {
                hash ^= b as u64;
                hash = hash.wrapping_mul(0x100000001b3);
            }
            for b in mask.to_ne_bytes() {
                hash ^= b as u64;
                hash = hash.wrapping_mul(0x100000001b3);
            }
        }
        if hash == 0 { 1 } else { hash }
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
                let mut was_download_active = false;
                let mut prev_minute = unsafe {
                    windows::Win32::System::SystemInformation::GetLocalTime().wMinute
                };
                let mut last_ip_fp = query_ipv4_fingerprint();
                let mut topology_ticks: u8 = 0;
                let mut pending_topology_notify: u8 = 0;

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

                    // Wake animation only when download speed reaches >= 25 Mbps (3,125,000 bytes/sec),
                    // or wake once when it drops below threshold to immediately restore date display.
                    let is_download_active = down_rate >= MIN_DOWNLOAD_THRESHOLD_BYTES;
                    if is_download_active || was_download_active {
                        unsafe {
                            let _ = PostMessageW(hwnd, WM_APP_NETWORK_CHANGED, WPARAM(0), LPARAM(0));
                        }
                    }
                    was_download_active = is_download_active;

                    // Also check if clock minute has flipped to ensure Dynamic Island resting clock is always live
                    let cur_minute = unsafe {
                        windows::Win32::System::SystemInformation::GetLocalTime().wMinute
                    };
                    if cur_minute != prev_minute {
                        prev_minute = cur_minute;
                        unsafe {
                            let _ = PostMessageW(hwnd, WM_APP_TIME_TICK, WPARAM(0), LPARAM(0));
                        }
                    }

                    // Debounced IPv4 topology change detection (checks every 5s, settles 2s for DHCP/DNS)
                    if pending_topology_notify > 0 {
                        pending_topology_notify -= 1;
                        if pending_topology_notify == 0 {
                            unsafe {
                                let _ = PostMessageW(hwnd, WM_APP_NETWORK_TOPOLOGY_CHANGED, WPARAM(0), LPARAM(0));
                            }
                        }
                    }
                    topology_ticks = (topology_ticks + 1) % 5;
                    if topology_ticks == 0 {
                        let cur_fp = query_ipv4_fingerprint();
                        if cur_fp != last_ip_fp {
                            last_ip_fp = cur_fp;
                            if cur_fp != 0 {
                                pending_topology_notify = 2;
                            }
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
