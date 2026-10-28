//! Synced Lyrics Fetcher & Synchronizer
//! Uses LRCLIB (open lyrics database) via WinHTTP API
//! Matches lyrics.png in follow_this_design

use std::sync::{Arc, Mutex};
use std::thread;
use windows::core::{w, PCWSTR};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
    WINHTTP_FLAG_SECURE,
};

#[derive(Clone, Debug, Default)]
pub struct SyncedLyrics {
    #[allow(dead_code)]
    pub track_title: String,
    pub lines: Vec<(f32, String)>, // (timestamp in seconds, lyric line text)
}

impl SyncedLyrics {
    /// Returns the active index in `lines` based on current high-resolution playback time.
    /// Incorporates a calibrated 0.30s vocal anticipation lead so lyrics highlight right as the singer articulates.
    pub fn get_active_index(&self, position_secs: f32) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        let effective_time = position_secs + 0.30;
        let mut active_idx = 0;
        for (i, (t, _)) in self.lines.iter().enumerate() {
            if *t <= effective_time {
                active_idx = i;
            } else {
                break;
            }
        }
        active_idx
    }

    /// Returns (previous_line, current_active_line, next_line)
    #[allow(dead_code)]
    pub fn get_current_lines(&self, position_secs: f32) -> (String, String, String) {
        if self.lines.is_empty() {
            return (
                String::new(),
                "♪ Enjoying the music ♪".to_string(),
                String::new(),
            );
        }

        let active_idx = self.get_active_index(position_secs);

        let prev = if active_idx > 0 {
            self.lines[active_idx - 1].1.clone()
        } else {
            String::new()
        };

        let curr = self.lines[active_idx].1.clone();

        let next = if active_idx + 1 < self.lines.len() {
            self.lines[active_idx + 1].1.clone()
        } else {
            String::new()
        };

        (prev, curr, next)
    }
}

pub struct LyricsManager {
    current: Arc<Mutex<SyncedLyrics>>,
    last_fetched_title: Arc<Mutex<String>>,
}

impl LyricsManager {
    pub fn new() -> Self {
        Self {
            current: Arc::new(Mutex::new(SyncedLyrics::default())),
            last_fetched_title: Arc::new(Mutex::new(String::new())),
        }
    }

    pub fn get_lyrics(&self) -> SyncedLyrics {
        self.current.lock().map(|l| l.clone()).unwrap_or_default()
    }

    pub fn fetch_for_track(&self, title: &str, artist: &str) {
        let title_clean = title.trim().to_string();
        let artist_clean = artist.trim().to_string();

        if title_clean.is_empty() {
            return;
        }

        {
            let mut last = self.last_fetched_title.lock().unwrap();
            if *last == title_clean {
                return; // already fetching/fetched
            }
            *last = title_clean.clone();
        }

        let current_arc = self.current.clone();

        thread::spawn(move || {
            let lyrics = fetch_lrclib_lyrics(&title_clean, &artist_clean);
            if let Ok(mut lock) = current_arc.lock() {
                *lock = lyrics;
            }
        });
    }
}

fn fetch_lrclib_lyrics(title: &str, artist: &str) -> SyncedLyrics {
    let mut result = SyncedLyrics {
        track_title: title.to_string(),
        lines: Vec::new(),
    };

    // Construct query URL: /api/get?track_name=...&artist_name=...
    let enc_title = url_encode(title);
    let enc_artist = url_encode(artist);
    let path_str = format!("/api/get?track_name={}&artist_name={}", enc_title, enc_artist);
    let mut path_wide: Vec<u16> = path_str.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let session = WinHttpOpen(
            w!("NotDynamicIsland/1.0"),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            None,
            None,
            0,
        );
        if session.is_null() {
            return result;
        }

        let connect = WinHttpConnect(session, w!("lrclib.net"), 443, 0);
        if connect.is_null() {
            let _ = WinHttpCloseHandle(session);
            return result;
        }

        let request = WinHttpOpenRequest(
            connect,
            w!("GET"),
            PCWSTR(path_wide.as_mut_ptr()),
            None,
            None,
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        );

        if !request.is_null() {
            let sent = WinHttpSendRequest(request, None, None, 0, 0, 0);
            if sent.is_ok() {
                let rec = WinHttpReceiveResponse(request, std::ptr::null_mut());
                if rec.is_ok() {
                    let mut body = Vec::new();
                    let mut buf = [0u8; 4096];
                    let mut read_bytes = 0u32;
                    while WinHttpReadData(
                        request,
                        buf.as_mut_ptr() as *mut _,
                        buf.len() as u32,
                        &mut read_bytes,
                    )
                    .is_ok()
                        && read_bytes > 0
                    {
                        body.extend_from_slice(&buf[..read_bytes as usize]);
                    }

                    if let Ok(json_str) = String::from_utf8(body) {
                        result.lines = parse_lrc_json(&json_str);
                    }
                }
            }
            let _ = WinHttpCloseHandle(request);
        }

        let _ = WinHttpCloseHandle(connect);
        let _ = WinHttpCloseHandle(session);
    }

    result
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn parse_lrc_json(json: &str) -> Vec<(f32, String)> {
    let mut lines = Vec::new();
    let marker = "\"syncedLyrics\":\"";
    if let Some(start_idx) = json.find(marker) {
        let content = &json[start_idx + marker.len()..];
        let end_idx = content.find("\",\"").unwrap_or(content.len());
        let lrc_raw = &content[..end_idx];

        // Replace literal \n and \\" with real newlines
        let unescaped = lrc_raw.replace("\\n", "\n").replace("\\\"", "\"");
        let mut global_offset_secs: f32 = 0.0;

        for line in unescaped.lines() {
            let line = line.trim();
            // Parse [offset:+/-ms] tag
            if line.to_lowercase().starts_with("[offset:") {
                if let Some(close) = line.find(']') {
                    let offset_str = line[8..close].trim();
                    if let Ok(ms) = offset_str.parse::<f32>() {
                        global_offset_secs = ms / 1000.0;
                    }
                }
                continue;
            }

            // Format: [mm:ss.xx] Lyric text
            if line.starts_with('[') {
                if let Some(close_bracket) = line.find(']') {
                    let time_part = &line[1..close_bracket];
                    let text_part = line[close_bracket + 1..].trim();

                    if let Some(secs) = parse_lrc_time(time_part) {
                        if !text_part.is_empty() {
                            let adjusted = (secs + global_offset_secs).max(0.0);
                            lines.push((adjusted, text_part.to_string()));
                        }
                    }
                }
            }
        }
    }
    lines
}

fn parse_lrc_time(t: &str) -> Option<f32> {
    let parts: Vec<&str> = t.split(':').collect();
    if parts.len() == 2 {
        let mins: f32 = parts[0].trim().parse().ok()?;
        let secs: f32 = parts[1].trim().parse().ok()?;
        Some(mins * 60.0 + secs)
    } else {
        None
    }
}
