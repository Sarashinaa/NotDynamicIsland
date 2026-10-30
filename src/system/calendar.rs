//! Calendar Management with Month Grid, Daily Agenda, and Google Calendar Live Sync
//! Fully native, zero dependencies, ultra-low resource usage (< 10 MB RAM, 0% CPU idle).

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Networking::WinHttp::*;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

pub const WM_APP_CALENDAR_UPDATED: u32 = 0x800F;

#[derive(Clone, Debug, PartialEq)]
pub struct CalendarEvent {
    pub id: u32,
    pub title: String,
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub time_str: String,
    pub color_idx: u8, // 0: Sky Blue, 1: Emerald Green, 2: Amber, 3: Purple, 4: Rose
    pub is_gcal: bool,
}

pub struct CalendarManager {
    pub events: Vec<CalendarEvent>,
    pub next_id: u32,
    pub view_year: u16,
    pub view_month: u8,
    pub selected_day: u8,
    pub today_year: u16,
    pub today_month: u8,
    pub today_day: u8,
    pub gcal_url: String,
    pub is_syncing: bool,
    pub sync_status: String,
    pub is_input_active: bool,
    pub input_buffer: String,
    pub scroll_offset: usize,
    file_path: PathBuf,
    sync_url_path: PathBuf,
    is_sync_running: Arc<AtomicBool>,
}

pub fn is_leap_year(year: u16) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

pub fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 => 31,
        2 => if is_leap_year(year) { 29 } else { 28 },
        3 => 31,
        4 => 30,
        5 => 31,
        6 => 30,
        7 => 31,
        8 => 31,
        9 => 30,
        10 => 31,
        11 => 30,
        12 => 31,
        _ => 30,
    }
}

/// Tomohiko Sakamoto's algorithm: 0 = Sunday, 1 = Monday, ..., 6 = Saturday
pub fn day_of_week(y: u16, m: u8, d: u8) -> u8 {
    static T: [u16; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut y = y;
    if m < 3 {
        y -= 1;
    }
    ((y + y / 4 - y / 100 + y / 400 + T[(m - 1) as usize] + d as u16) % 7) as u8
}

#[allow(dead_code)]
pub fn month_name(month: u8) -> &'static str {
    match month {
        1 => "JANUARY",
        2 => "FEBRUARY",
        3 => "MARCH",
        4 => "APRIL",
        5 => "MAY",
        6 => "JUNE",
        7 => "JULY",
        8 => "AUGUST",
        9 => "SEPTEMBER",
        10 => "OCTOBER",
        11 => "NOVEMBER",
        12 => "DECEMBER",
        _ => "CALENDAR",
    }
}

pub fn month_short(month: u8) -> &'static str {
    match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "???",
    }
}

pub fn day_name_short(dow: u8) -> &'static str {
    match dow {
        0 => "Sun",
        1 => "Mon",
        2 => "Tue",
        3 => "Wed",
        4 => "Thu",
        5 => "Fri",
        _ => "Sat",
    }
}

impl CalendarManager {
    pub fn new() -> Self {
        let (today_year, today_month, today_day) = unsafe {
            let st = GetLocalTime();
            (st.wYear, st.wMonth as u8, st.wDay as u8)
        };

        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        let exe_file = exe_dir.join("calendar.json");

        let (file_path, sync_url_path) = if exe_file.exists() {
            (exe_file, exe_dir.join("calendar_sync.txt"))
        } else {
            let app_data = std::env::var("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|_| exe_dir.clone());
            let island_dir = app_data.join("NotDynamicIsland");
            let _ = fs::create_dir_all(&island_dir);
            (
                island_dir.join("calendar.json"),
                island_dir.join("calendar_sync.txt"),
            )
        };

        let mut mgr = Self {
            events: Vec::new(),
            next_id: 1,
            view_year: today_year,
            view_month: today_month,
            selected_day: today_day,
            today_year,
            today_month,
            today_day,
            gcal_url: String::new(),
            is_syncing: false,
            sync_status: "Offline".to_string(),
            is_input_active: false,
            input_buffer: String::new(),
            scroll_offset: 0,
            file_path,
            sync_url_path,
            is_sync_running: Arc::new(AtomicBool::new(false)),
        };

        mgr.load_sync_url();
        mgr.load_local();

        // If no events exist, add a helpful initial welcome event for today
        if mgr.events.is_empty() {
            mgr.add_event(
                "Welcome to Dynamic Island Calendar 🏝️".to_string(),
                "All Day".to_string(),
                0,
            );
        }

        mgr
    }

    pub fn update_today(&mut self) {
        let (y, m, d) = unsafe {
            let st = GetLocalTime();
            (st.wYear, st.wMonth as u8, st.wDay as u8)
        };
        self.today_year = y;
        self.today_month = m;
        self.today_day = d;
    }

    pub fn prev_month(&mut self) {
        if self.view_month == 1 {
            self.view_month = 12;
            self.view_year = self.view_year.saturating_sub(1);
        } else {
            self.view_month -= 1;
        }
        let max_days = days_in_month(self.view_year, self.view_month);
        self.selected_day = self.selected_day.clamp(1, max_days);
        self.scroll_offset = 0;
    }

    pub fn next_month(&mut self) {
        if self.view_month == 12 {
            self.view_month = 1;
            self.view_year = self.view_year.saturating_add(1);
        } else {
            self.view_month += 1;
        }
        let max_days = days_in_month(self.view_year, self.view_month);
        self.selected_day = self.selected_day.clamp(1, max_days);
        self.scroll_offset = 0;
    }

    pub fn jump_to_today(&mut self) {
        self.update_today();
        self.view_year = self.today_year;
        self.view_month = self.today_month;
        self.selected_day = self.today_day;
        self.scroll_offset = 0;
    }

    pub fn select_day(&mut self, day: u8) {
        let max_days = days_in_month(self.view_year, self.view_month);
        self.selected_day = day.clamp(1, max_days);
        self.scroll_offset = 0;
    }

    pub fn get_events_for_selected(&self) -> Vec<&CalendarEvent> {
        self.events
            .iter()
            .filter(|e| {
                e.year == self.view_year
                    && e.month == self.view_month
                    && e.day == self.selected_day
            })
            .collect()
    }

    pub fn has_events_on_day(&self, year: u16, month: u8, day: u8) -> bool {
        self.events
            .iter()
            .any(|e| e.year == year && e.month == month && e.day == day)
    }

    pub fn add_event(&mut self, title: String, time_str: String, color_idx: u8) {
        let id = self.next_id;
        self.next_id += 1;
        self.events.push(CalendarEvent {
            id,
            title,
            year: self.view_year,
            month: self.view_month,
            day: self.selected_day,
            time_str,
            color_idx: color_idx % 5,
            is_gcal: false,
        });
        self.save_local();
    }

    pub fn delete_event(&mut self, id: u32) {
        self.events.retain(|e| e.id != id);
        self.save_local();
    }

    pub fn start_input(&mut self) {
        self.is_input_active = true;
        self.input_buffer.clear();
    }

    pub fn cancel_input(&mut self) {
        self.is_input_active = false;
        self.input_buffer.clear();
    }

    pub fn commit_input(&mut self) {
        let clean = self.input_buffer.trim().to_string();
        if !clean.is_empty() {
            // Check if user specified a time prefix, e.g. "14:00 Team Review"
            let (time_str, title) = parse_user_event_input(&clean);
            let color = ((self.selected_day as usize + self.events.len()) % 5) as u8;
            self.add_event(title, time_str, color);
        }
        self.is_input_active = false;
        self.input_buffer.clear();
    }

    pub fn input_char(&mut self, c: char) {
        if self.input_buffer.chars().count() < 40 {
            self.input_buffer.push(c);
        }
    }

    pub fn input_backspace(&mut self) {
        self.input_buffer.pop();
    }

    pub fn load_sync_url(&mut self) {
        if self.sync_url_path.exists() {
            if let Ok(content) = fs::read_to_string(&self.sync_url_path) {
                let trimmed = content.trim().to_string();
                if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                    self.gcal_url = trimmed;
                    self.sync_status = "GCal Ready".to_string();
                    return;
                }
            }
        } else {
            // Write a helpful sample config file so user can easily paste their Google Calendar link
            let help = "# Paste your Google Calendar secret iCal URL below:\n# Example: https://calendar.google.com/calendar/ical/.../basic.ics\n";
            let _ = fs::write(&self.sync_url_path, help);
        }
    }

    pub fn load_local(&mut self) {
        if self.file_path.exists() {
            if let Ok(content) = fs::read_to_string(&self.file_path) {
                let mut loaded = Vec::new();
                let mut rest = content.as_str();

                while let Some(start_obj) = rest.find('{') {
                    let after_start = &rest[start_obj..];
                    if let Some(end_obj) = after_start.find('}') {
                        let obj_str = &after_start[..=end_obj];
                        let id = extract_json_num(obj_str, "\"id\":").unwrap_or(0);
                        let year = extract_json_num(obj_str, "\"year\":").unwrap_or(2026) as u16;
                        let month = extract_json_num(obj_str, "\"month\":").unwrap_or(10) as u8;
                        let day = extract_json_num(obj_str, "\"day\":").unwrap_or(1) as u8;
                        let color_idx = extract_json_num(obj_str, "\"color\":").unwrap_or(0) as u8;
                        let is_gcal = obj_str.contains("\"gcal\":true");
                        let title = extract_json_str(obj_str, "\"title\":").unwrap_or_default();
                        let time_str = extract_json_str(obj_str, "\"time\":").unwrap_or_else(|| "All Day".to_string());

                        if !title.is_empty() && id > 0 {
                            if id >= self.next_id {
                                self.next_id = id + 1;
                            }
                            loaded.push(CalendarEvent {
                                id,
                                title,
                                year,
                                month,
                                day,
                                time_str,
                                color_idx,
                                is_gcal,
                            });
                        }
                        rest = &after_start[end_obj + 1..];
                    } else {
                        break;
                    }
                }

                if !loaded.is_empty() {
                    self.events = loaded;
                }
            }
        }
    }

    pub fn save_local(&self) {
        let mut json = String::from("[\n");
        for (i, e) in self.events.iter().enumerate() {
            let comma = if i + 1 < self.events.len() { "," } else { "" };
            let escaped_title = e.title.replace('\\', "\\\\").replace('"', "\\\"");
            let escaped_time = e.time_str.replace('\\', "\\\\").replace('"', "\\\"");
            json.push_str(&format!(
                "  {{\"id\":{},\"year\":{},\"month\":{},\"day\":{},\"color\":{},\"gcal\":{},\"time\":\"{}\",\"title\":\"{}\"}}{}\n",
                e.id, e.year, e.month, e.day, e.color_idx, e.is_gcal, escaped_time, escaped_title, comma
            ));
        }
        json.push_str("]\n");
        let _ = fs::write(&self.file_path, json);
    }

    /// Trigger background sync with Google Calendar iCal feed
    pub fn trigger_sync(&mut self, hwnd: HWND) {
        if self.gcal_url.is_empty() {
            self.load_sync_url();
        }
        if self.gcal_url.is_empty() {
            self.sync_status = "No URL (see calendar_sync.txt)".to_string();
            return;
        }

        if self.is_sync_running.swap(true, Ordering::SeqCst) {
            return; // Sync already in flight
        }

        self.is_syncing = true;
        self.sync_status = "Syncing...".to_string();
        let running_flag = Arc::clone(&self.is_sync_running);
        let url = self.gcal_url.clone();
        let hwnd_val = hwnd.0 as usize;
        let file_path = self.file_path.clone();

        thread::Builder::new()
            .name("calendar-sync".to_string())
            .spawn(move || {
                let hwnd = HWND(hwnd_val as *mut _);
                match fetch_url_native(&url) {
                    Ok(ics_content) => {
                        let parsed_events = parse_ics_feed(&ics_content);
                        if !parsed_events.is_empty() {
                            // Merge into local calendar
                            merge_gcal_events(&file_path, parsed_events);
                        }
                    }
                    Err(e) => {
                        eprintln!("[Calendar] Google Calendar sync failed: {}", e);
                    }
                }
                running_flag.store(false, Ordering::SeqCst);
                unsafe {
                    let _ = PostMessageW(hwnd, WM_APP_CALENDAR_UPDATED, WPARAM(0), LPARAM(0));
                }
            })
            .expect("Failed to spawn calendar sync thread");
    }

    /// Call when WM_APP_CALENDAR_UPDATED arrives to reload merged events
    pub fn on_sync_completed(&mut self) {
        self.is_syncing = false;
        self.load_local();
        self.sync_status = "Google Calendar Synced".to_string();
    }
}

fn parse_user_event_input(text: &str) -> (String, String) {
    // If user starts with time like "14:00 Meeting" or "09:30-10:30 Standup"
    let parts: Vec<&str> = text.splitn(2, ' ').collect();
    if parts.len() == 2 {
        let first = parts[0];
        if (first.contains(':') || first.contains('.')) && first.chars().any(|c| c.is_ascii_digit()) {
            return (first.to_string(), parts[1].to_string());
        }
    }
    ("All Day".to_string(), text.to_string())
}

fn extract_json_str(source: &str, key: &str) -> Option<String> {
    let key_pos = source.find(key)?;
    let val_start = key_pos + key.len();
    let quote1 = source[val_start..].find('"')? + val_start + 1;
    let quote2 = source[quote1..].find('"')? + quote1;
    Some(source[quote1..quote2].replace("\\\"", "\"").replace("\\\\", "\\"))
}

fn extract_json_num(source: &str, key: &str) -> Option<u32> {
    let key_pos = source.find(key)?;
    let val_start = key_pos + key.len();
    let num_str: String = source[val_start..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    num_str.parse::<u32>().ok()
}

/// Native Windows WinHttp HTTPS fetcher (Zero external dependencies, built into Windows!)
pub fn fetch_url_native(url: &str) -> Result<String, String> {
    let trimmed = url.trim();
    let is_https = trimmed.starts_with("https://");
    let is_http = trimmed.starts_with("http://");
    if !is_https && !is_http {
        return Err("URL must start with http:// or https://".to_string());
    }

    let rest = if is_https { &trimmed[8..] } else { &trimmed[7..] };
    let slash_idx = rest.find('/').unwrap_or(rest.len());
    let host = &rest[..slash_idx];
    let path = if slash_idx < rest.len() { &rest[slash_idx..] } else { "/" };

    let host_w: Vec<u16> = host.encode_utf16().chain(std::iter::once(0)).collect();
    let path_w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let user_agent = windows::core::w!("NotDynamicIsland/1.0");

    unsafe {
        let session = WinHttpOpen(
            user_agent,
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        );
        if session.is_null() {
            return Err("WinHttpOpen failed".to_string());
        }

        let port = if is_https { 443 } else { 80 };
        let connect = WinHttpConnect(session, PCWSTR(host_w.as_ptr()), port, 0);
        if connect.is_null() {
            let _ = WinHttpCloseHandle(session);
            return Err("WinHttpConnect failed".to_string());
        }

        let flags = if is_https {
            WINHTTP_FLAG_SECURE
        } else {
            WINHTTP_OPEN_REQUEST_FLAGS(0)
        };
        let verb = windows::core::w!("GET");
        let request = WinHttpOpenRequest(
            connect,
            verb,
            PCWSTR(path_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            flags,
        );
        if request.is_null() {
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return Err("WinHttpOpenRequest failed".to_string());
        }

        let send_ok = WinHttpSendRequest(
            request,
            None,
            None,
            0,
            0,
            0,
        );
        if send_ok.is_err() {
            let _ = WinHttpCloseHandle(request);
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return Err("WinHttpSendRequest failed".to_string());
        }

        let recv_ok = WinHttpReceiveResponse(request, std::ptr::null_mut());
        if recv_ok.is_err() {
            let _ = WinHttpCloseHandle(request);
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return Err("WinHttpReceiveResponse failed".to_string());
        }

        let mut response_bytes: Vec<u8> = Vec::new();
        loop {
            let mut size_to_read = 0u32;
            if WinHttpQueryDataAvailable(request, &mut size_to_read).is_err() || size_to_read == 0 {
                break;
            }
            let mut temp_buf = vec![0u8; size_to_read as usize];
            let mut downloaded = 0u32;
            if WinHttpReadData(
                request,
                temp_buf.as_mut_ptr() as *mut _,
                size_to_read,
                &mut downloaded,
            ).is_ok() && downloaded > 0 {
                response_bytes.extend_from_slice(&temp_buf[..downloaded as usize]);
            } else {
                break;
            }
        }

        let _ = WinHttpCloseHandle(request);
        let _ = WinHttpCloseHandle(connect);
        let _ = WinHttpCloseHandle(session);

        String::from_utf8(response_bytes).map_err(|e| format!("UTF-8 decode error: {}", e))
    }
}

/// Lightweight streaming parser for standard iCal (.ics) Google Calendar feed
pub fn parse_ics_feed(content: &str) -> Vec<CalendarEvent> {
    let mut events = Vec::new();
    let mut cur_title = String::new();
    let mut cur_dtstart = String::new();
    let mut in_vevent = false;
    let mut event_id = 1000u32;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "BEGIN:VEVENT" {
            in_vevent = true;
            cur_title.clear();
            cur_dtstart.clear();
        } else if trimmed == "END:VEVENT" {
            if in_vevent && !cur_title.is_empty() && !cur_dtstart.is_empty() {
                if let Some((year, month, day, time_str)) = parse_ics_dtstart(&cur_dtstart) {
                    event_id += 1;
                    events.push(CalendarEvent {
                        id: event_id,
                        title: cur_title.clone(),
                        year,
                        month,
                        day,
                        time_str,
                        color_idx: (events.len() % 5) as u8,
                        is_gcal: true,
                    });
                }
            }
            in_vevent = false;
        } else if in_vevent {
            if trimmed.starts_with("SUMMARY:") {
                cur_title = trimmed["SUMMARY:".len()..].to_string();
            } else if trimmed.starts_with("DTSTART") {
                if let Some(colon) = trimmed.find(':') {
                    cur_dtstart = trimmed[colon + 1..].to_string();
                }
            }
        }
    }
    events
}

fn parse_ics_dtstart(val: &str) -> Option<(u16, u8, u8, String)> {
    // Formats:
    // 20261031T090000Z
    // 20261031
    if val.len() < 8 {
        return None;
    }
    let year = val[0..4].parse::<u16>().ok()?;
    let month = val[4..6].parse::<u8>().ok()?;
    let day = val[6..8].parse::<u8>().ok()?;

    let time_str = if val.len() >= 13 && val.contains('T') {
        let t_pos = val.find('T')?;
        let hh = &val[t_pos + 1..t_pos + 3];
        let mm = &val[t_pos + 3..t_pos + 5];
        format!("{}:{}", hh, mm)
    } else {
        "All Day".to_string()
    };

    Some((year, month, day, time_str))
}

fn merge_gcal_events(file_path: &PathBuf, gcal_events: Vec<CalendarEvent>) {
    // Read existing, keep local events (is_gcal == false), replace all is_gcal == true with latest
    let mut all_events: Vec<CalendarEvent> = Vec::new();
    if file_path.exists() {
        if let Ok(content) = fs::read_to_string(file_path) {
            let mut rest = content.as_str();
            while let Some(start_obj) = rest.find('{') {
                let after_start = &rest[start_obj..];
                if let Some(end_obj) = after_start.find('}') {
                    let obj_str = &after_start[..=end_obj];
                    let is_gcal = obj_str.contains("\"gcal\":true");
                    if !is_gcal {
                        let id = extract_json_num(obj_str, "\"id\":").unwrap_or(0);
                        let year = extract_json_num(obj_str, "\"year\":").unwrap_or(2026) as u16;
                        let month = extract_json_num(obj_str, "\"month\":").unwrap_or(10) as u8;
                        let day = extract_json_num(obj_str, "\"day\":").unwrap_or(1) as u8;
                        let color_idx = extract_json_num(obj_str, "\"color\":").unwrap_or(0) as u8;
                        let title = extract_json_str(obj_str, "\"title\":").unwrap_or_default();
                        let time_str = extract_json_str(obj_str, "\"time\":").unwrap_or_else(|| "All Day".to_string());
                        if !title.is_empty() {
                            all_events.push(CalendarEvent {
                                id,
                                title,
                                year,
                                month,
                                day,
                                time_str,
                                color_idx,
                                is_gcal: false,
                            });
                        }
                    }
                    rest = &after_start[end_obj + 1..];
                } else {
                    break;
                }
            }
        }
    }

    all_events.extend(gcal_events);

    // Save back
    let mut json = String::from("[\n");
    for (i, e) in all_events.iter().enumerate() {
        let comma = if i + 1 < all_events.len() { "," } else { "" };
        let escaped_title = e.title.replace('\\', "\\\\").replace('"', "\\\"");
        let escaped_time = e.time_str.replace('\\', "\\\\").replace('"', "\\\"");
        json.push_str(&format!(
            "  {{\"id\":{},\"year\":{},\"month\":{},\"day\":{},\"color\":{},\"gcal\":{},\"time\":\"{}\",\"title\":\"{}\"}}{}\n",
            e.id, e.year, e.month, e.day, e.color_idx, e.is_gcal, escaped_time, escaped_title, comma
        ));
    }
    json.push_str("]\n");
    let _ = fs::write(file_path, json);
}
