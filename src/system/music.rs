//! Zero-dependency Music Playback Automation for AI Assistant
//! Supports YouTube Music / YouTube (auto-play first search result via WinHTTP stream,
//! optional Edge/Chrome/Brave standalone App mode with single-window reuse)
//! and Spotify Desktop URI search (`spotify:search:...`), running asynchronously off the UI thread.

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, WPARAM};
use windows::Win32::Networking::WinHttp::*;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, EnumWindows, GetClassNameW, GetWindowLongW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindowVisible, PostMessageW, ASFW_ANY, GWL_STYLE, SW_SHOWNORMAL,
    WM_CLOSE, WS_CHILD,
};

use crate::system::app_launcher::sanitize_browser_profile;
use crate::system::weather::url_encode_query;

pub const WM_APP_MUSIC_STATUS: u32 = 0x8019;

#[derive(Clone, Debug)]
pub struct MusicStatusPayload {
    pub message: String,
    pub is_error: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromiumBrowser {
    Edge,
    Chrome,
    Brave,
}

impl ChromiumBrowser {
    pub fn exe_name(&self) -> &'static str {
        match self {
            ChromiumBrowser::Edge => "msedge.exe",
            ChromiumBrowser::Chrome => "chrome.exe",
            ChromiumBrowser::Brave => "brave.exe",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            ChromiumBrowser::Edge => "Edge",
            ChromiumBrowser::Chrome => "Chrome",
            ChromiumBrowser::Brave => "Brave",
        }
    }

    pub fn resolve_executable(&self) -> PathBuf {
        let rel = match self {
            ChromiumBrowser::Edge => r"Microsoft\Edge\Application\msedge.exe",
            ChromiumBrowser::Chrome => r"Google\Chrome\Application\chrome.exe",
            ChromiumBrowser::Brave => r"BraveSoftware\Brave-Browser\Application\brave.exe",
        };
        for env_key in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
            if let Ok(base) = std::env::var(env_key) {
                let candidate = PathBuf::from(base).join(rel);
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
        PathBuf::from(self.exe_name())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MusicBrowserMode {
    Default,
    Browser {
        browser: ChromiumBrowser,
        app_mode: bool,
        profile: Option<String>,
    },
}

impl Default for MusicBrowserMode {
    fn default() -> Self {
        Self::Default
    }
}

impl MusicBrowserMode {
    pub fn parse_config(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let (mode_raw, profile_raw) = match trimmed.split_once(':') {
            Some((left, right)) => (left, Some(right)),
            None => (trimmed, None),
        };
        let normalized = mode_raw
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("-")
            .to_ascii_lowercase()
            .replace('_', "-");

        let profile = profile_raw
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(sanitize_browser_profile);

        match normalized.as_str() {
            "default" | "auto" | "system" | "sistem" | "normal" => Some(Self::Default),
            "edge-app" | "edgeapp" | "edge-pwa" | "msedge-app" | "msedge-pwa" => {
                Some(Self::Browser {
                    browser: ChromiumBrowser::Edge,
                    app_mode: true,
                    profile,
                })
            }
            "edge" | "msedge" => Some(Self::Browser {
                browser: ChromiumBrowser::Edge,
                app_mode: false,
                profile,
            }),
            "chrome-app" | "chromeapp" | "chrome-pwa" => Some(Self::Browser {
                browser: ChromiumBrowser::Chrome,
                app_mode: true,
                profile,
            }),
            "chrome" | "google-chrome" => Some(Self::Browser {
                browser: ChromiumBrowser::Chrome,
                app_mode: false,
                profile,
            }),
            "brave-app" | "braveapp" | "brave-pwa" => Some(Self::Browser {
                browser: ChromiumBrowser::Brave,
                app_mode: true,
                profile,
            }),
            "brave" => Some(Self::Browser {
                browser: ChromiumBrowser::Brave,
                app_mode: false,
                profile,
            }),
            _ => None,
        }
    }

    pub fn to_config_string(&self) -> String {
        match self {
            Self::Default => "default".to_string(),
            Self::Browser {
                browser,
                app_mode,
                profile,
            } => {
                let base = match (browser, app_mode) {
                    (ChromiumBrowser::Edge, true) => "edge-app",
                    (ChromiumBrowser::Edge, false) => "edge",
                    (ChromiumBrowser::Chrome, true) => "chrome-app",
                    (ChromiumBrowser::Chrome, false) => "chrome",
                    (ChromiumBrowser::Brave, true) => "brave-app",
                    (ChromiumBrowser::Brave, false) => "brave",
                };
                if let Some(p) = profile {
                    format!("{}:{}", base, sanitize_browser_profile(p))
                } else {
                    base.to_string()
                }
            }
        }
    }

    pub fn display_label(&self) -> String {
        match self {
            Self::Default => "Browser Default".to_string(),
            Self::Browser {
                browser,
                app_mode,
                profile,
            } => {
                let mode_str = if *app_mode {
                    format!("{} App", browser.display_name())
                } else {
                    browser.display_name().to_string()
                };
                if let Some(p) = profile {
                    format!("{} ({})", mode_str, p)
                } else {
                    mode_str
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MusicPlatform {
    YouTubeMusic,
    YouTube,
    Spotify,
}

impl MusicPlatform {
    pub fn display_name(&self) -> &'static str {
        match self {
            MusicPlatform::YouTubeMusic => "YouTube Music",
            MusicPlatform::YouTube => "YouTube",
            MusicPlatform::Spotify => "Spotify",
        }
    }

    pub fn from_token(token: &str) -> Option<Self> {
        match token.trim().to_ascii_lowercase().as_str() {
            "ytmusic" | "youtube music" | "yt music" | "ytm" => Some(MusicPlatform::YouTubeMusic),
            "youtube" | "yt" | "yutup" => Some(MusicPlatform::YouTube),
            "spotify" | "spoti" => Some(MusicPlatform::Spotify),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayMusicRequest {
    pub query: String,
    pub platform: MusicPlatform,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VerbKind {
    Unrestricted,
    SemiAmbiguous,
    NounRequired,
}

fn strip_ascii_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    debug_assert!(prefix.is_ascii());
    let sb = s.as_bytes();
    let pb = prefix.as_bytes();
    if sb.len() >= pb.len() && sb[..pb.len()].eq_ignore_ascii_case(pb) && s.is_char_boundary(pb.len()) {
        Some(&s[pb.len()..])
    } else {
        None
    }
}

fn strip_ascii_suffix_ci<'a>(s: &'a str, suffix: &str) -> Option<&'a str> {
    debug_assert!(suffix.is_ascii());
    let sb = s.as_bytes();
    let xb = suffix.as_bytes();
    if sb.len() >= xb.len() {
        let split = sb.len() - xb.len();
        if sb[split..].eq_ignore_ascii_case(xb) && s.is_char_boundary(split) {
            return Some(&s[..split]);
        }
    }
    None
}

fn strip_trailing_polite_and_punct(mut s: &str) -> &str {
    const POLITE_SUFFIXES: &[&str] = &[
        " dong",
        " ya",
        " yah",
        " pls",
        " please",
        " tolong",
        " sekarang",
        " dulu",
        " deh",
        " bro",
        " gan",
        " min",
        " nya",
    ];
    loop {
        let before = s;
        s = s.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '!' | '?' | '.' | ','));
        for suffix in POLITE_SUFFIXES {
            if let Some(rest) = strip_ascii_suffix_ci(s, suffix) {
                s = rest.trim_end();
            }
        }
        if s == before {
            break;
        }
    }
    s
}

fn strip_leading_platform(s: &str) -> (&str, Option<MusicPlatform>) {
    const LEADING_PLATFORMS: &[(&str, MusicPlatform)] = &[
        ("di youtube music ", MusicPlatform::YouTubeMusic),
        ("on youtube music ", MusicPlatform::YouTubeMusic),
        ("di yt music ", MusicPlatform::YouTubeMusic),
        ("on yt music ", MusicPlatform::YouTubeMusic),
        ("di ytmusic ", MusicPlatform::YouTubeMusic),
        ("on ytmusic ", MusicPlatform::YouTubeMusic),
        ("di ytm ", MusicPlatform::YouTubeMusic),
        ("on ytm ", MusicPlatform::YouTubeMusic),
        ("di youtube ", MusicPlatform::YouTube),
        ("on youtube ", MusicPlatform::YouTube),
        ("di yt ", MusicPlatform::YouTube),
        ("on yt ", MusicPlatform::YouTube),
        ("di spotify ", MusicPlatform::Spotify),
        ("on spotify ", MusicPlatform::Spotify),
        ("di spoti ", MusicPlatform::Spotify),
    ];
    for (prefix, platform) in LEADING_PLATFORMS {
        if let Some(rest) = strip_ascii_prefix_ci(s, prefix) {
            return (rest.trim_start(), Some(*platform));
        }
    }
    (s, None)
}

fn strip_trailing_platform(s: &str) -> (&str, Option<MusicPlatform>) {
    const TRAILING_PLATFORMS: &[(&str, MusicPlatform)] = &[
        // Check YouTube Music variants BEFORE plain YouTube variants
        (" di youtube music", MusicPlatform::YouTubeMusic),
        (" on youtube music", MusicPlatform::YouTubeMusic),
        (" via youtube music", MusicPlatform::YouTubeMusic),
        (" pakai youtube music", MusicPlatform::YouTubeMusic),
        (" pake youtube music", MusicPlatform::YouTubeMusic),
        (" di yt music", MusicPlatform::YouTubeMusic),
        (" on yt music", MusicPlatform::YouTubeMusic),
        (" via yt music", MusicPlatform::YouTubeMusic),
        (" di ytmusic", MusicPlatform::YouTubeMusic),
        (" on ytmusic", MusicPlatform::YouTubeMusic),
        (" di ytm", MusicPlatform::YouTubeMusic),
        (" on ytm", MusicPlatform::YouTubeMusic),
        (" youtube music", MusicPlatform::YouTubeMusic),
        (" yt music", MusicPlatform::YouTubeMusic),
        (" di youtube", MusicPlatform::YouTube),
        (" on youtube", MusicPlatform::YouTube),
        (" via youtube", MusicPlatform::YouTube),
        (" pakai youtube", MusicPlatform::YouTube),
        (" pake youtube", MusicPlatform::YouTube),
        (" di yt", MusicPlatform::YouTube),
        (" on yt", MusicPlatform::YouTube),
        (" via yt", MusicPlatform::YouTube),
        (" di yutup", MusicPlatform::YouTube),
        (" youtube", MusicPlatform::YouTube),
        (" di spotify", MusicPlatform::Spotify),
        (" on spotify", MusicPlatform::Spotify),
        (" via spotify", MusicPlatform::Spotify),
        (" pakai spotify", MusicPlatform::Spotify),
        (" pake spotify", MusicPlatform::Spotify),
        (" di spoti", MusicPlatform::Spotify),
        (" spotify", MusicPlatform::Spotify),
    ];
    for (suffix, platform) in TRAILING_PLATFORMS {
        if let Some(rest) = strip_ascii_suffix_ci(s, suffix) {
            return (rest.trim_end(), Some(*platform));
        }
    }
    (s, None)
}

fn clean_music_query(raw: &str) -> String {
    let mut s = raw.trim();
    loop {
        if s.len() >= 2
            && ((s.starts_with('"') && s.ends_with('"'))
                || (s.starts_with('\'') && s.ends_with('\'')))
        {
            s = s[1..s.len() - 1].trim();
        } else {
            break;
        }
    }

    let words: Vec<&str> = s.split_whitespace().collect();
    if words.is_empty() {
        return String::new();
    }

    let mut cleaned_words = Vec::with_capacity(words.len());
    for (idx, word) in words.iter().enumerate() {
        let is_interior = idx > 0 && idx + 1 < words.len();
        if is_interior
            && (word.eq_ignore_ascii_case("dari")
                || word.eq_ignore_ascii_case("oleh")
                || word.eq_ignore_ascii_case("by"))
        {
            continue;
        }
        cleaned_words.push(*word);
    }
    cleaned_words.join(" ")
}

/// Extracts natural-language music playback requests in Indonesian or English.
pub fn extract_play_music_intent(prompt: &str) -> Option<PlayMusicRequest> {
    let mut cur = prompt.trim();
    if cur.is_empty() {
        return None;
    }

    const POLITE_WRAPPERS: &[&str] = &[
        "minta tolong ",
        "tolong ",
        "minta ",
        "coba ",
        "bisa ",
        "boleh ",
        "mau ",
        "pengen ",
        "please ",
        "pls ",
    ];

    for _ in 0..2 {
        let mut stripped = false;
        for wrapper in POLITE_WRAPPERS {
            if let Some(rest) = strip_ascii_prefix_ci(cur, wrapper) {
                cur = rest.trim_start();
                stripped = true;
                break;
            }
        }
        if !stripped {
            break;
        }
    }

    const UNRESTRICTED_VERBS: &[&str] = &[
        "putarkan ",
        "putarin ",
        "puterkan ",
        "puterin ",
        "putar ",
        "puter ",
        "play ",
        "dengerin ",
        "dengarkan ",
        "dengar ",
    ];
    const SEMI_AMBIGUOUS_VERBS: &[&str] = &[
        "setelkan ",
        "setelin ",
        "setel ",
        "stel ",
        "nyetel ",
    ];
    const NOUN_REQUIRED_VERBS: &[&str] = &[
        "mainkan ",
        "mainin ",
        "nyalain ",
        "nyalakan ",
    ];

    let (after_verb, verb_kind, matched_play) = UNRESTRICTED_VERBS
        .iter()
        .find_map(|v| {
            strip_ascii_prefix_ci(cur, v).map(|r| (r, VerbKind::Unrestricted, *v == "play "))
        })
        .or_else(|| {
            SEMI_AMBIGUOUS_VERBS
                .iter()
                .find_map(|v| strip_ascii_prefix_ci(cur, v).map(|r| (r, VerbKind::SemiAmbiguous, false)))
        })
        .or_else(|| {
            NOUN_REQUIRED_VERBS
                .iter()
                .find_map(|v| strip_ascii_prefix_ci(cur, v).map(|r| (r, VerbKind::NounRequired, false)))
        })?;

    let mut rem = after_verb.trim_start();
    if matched_play {
        let lower_rem = rem.trim().to_lowercase();
        if lower_rem == "pause"
            || lower_rem == "/pause"
            || lower_rem == "game"
            || lower_rem.starts_with("game ")
        {
            return None;
        }
    }

    let (after_lead_plat, leading_platform) = strip_leading_platform(rem);
    rem = after_lead_plat;

    const MUSIC_NOUNS: &[&str] = &["lagu ", "musik ", "music ", "song ", "track "];
    let mut has_music_noun = false;
    for noun in MUSIC_NOUNS {
        if let Some(rest) = strip_ascii_prefix_ci(rem, noun) {
            rem = rest.trim_start();
            has_music_noun = true;
            break;
        }
    }

    match verb_kind {
        VerbKind::NounRequired if !has_music_noun => return None,
        VerbKind::SemiAmbiguous if !has_music_noun => {
            const SYSTEM_KEYWORDS: &[&str] = &[
                "timer",
                "fokus",
                "pomodoro",
                "alarm",
                "lokasi",
                "kota",
                "cuaca",
                "volume",
                "suara",
                "brightness",
                "kecerahan",
                "mic",
                "mikrofon",
            ];
            let lower_rem = rem.to_lowercase();
            if SYSTEM_KEYWORDS.iter().any(|kw| lower_rem.contains(kw)) {
                return None;
            }
        }
        _ => {}
    }

    rem = strip_trailing_polite_and_punct(rem);
    let (after_tail_plat, trailing_platform) = strip_trailing_platform(rem);
    rem = strip_trailing_polite_and_punct(after_tail_plat);

    let query = clean_music_query(rem);
    if query.is_empty() {
        return None;
    }

    let lower_query = query.to_lowercase();
    if matches!(
        lower_query.as_str(),
        "lagu"
            | "musik"
            | "music"
            | "song"
            | "track"
            | "spotify"
            | "youtube"
            | "yt"
            | "yt music"
            | "youtube music"
            | "ytm"
    ) {
        return None;
    }

    let platform = trailing_platform
        .or(leading_platform)
        .unwrap_or(MusicPlatform::YouTubeMusic);

    Some(PlayMusicRequest { query, platform })
}

/// Parses `[ACTION:PLAY_MUSIC:query]` or `[ACTION:PLAY_MUSIC:query:platform]`.
pub fn parse_action_play_music(payload: &str) -> Option<PlayMusicRequest> {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (raw_query, platform) = if let Some((left, right)) = trimmed.rsplit_once(':') {
        if let Some(p) = MusicPlatform::from_token(right) {
            (left, p)
        } else {
            (trimmed, MusicPlatform::YouTubeMusic)
        }
    } else {
        (trimmed, MusicPlatform::YouTubeMusic)
    };

    let query = clean_music_query(raw_query);
    if query.is_empty() {
        return None;
    }

    Some(PlayMusicRequest { query, platform })
}

pub fn is_valid_youtube_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Scans a raw HTML/JSON byte window for the first valid `"videoRenderer":{"videoId":"XXXXXXXXXXX"`.
pub fn extract_first_youtube_video_id_bytes(haystack: &[u8]) -> Option<String> {
    const RENDERER_MARKER: &[u8] = b"\"videoRenderer\":{";
    const VIDEO_ID_MARKER: &[u8] = b"\"videoId\":\"";

    let mut offset = 0usize;
    while let Some(rel_pos) = find_subslice(&haystack[offset..], RENDERER_MARKER) {
        let obj_start = offset + rel_pos + RENDERER_MARKER.len();
        let search_limit = (obj_start + 128).min(haystack.len());

        // Restrict scan to the top of this videoRenderer object (stop if it closes first)
        let mut obj_end = obj_start;
        let mut depth = 1usize;
        while obj_end < search_limit {
            match haystack[obj_end] {
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            obj_end += 1;
        }

        if let Some(id_rel) = find_subslice(&haystack[obj_start..obj_end], VIDEO_ID_MARKER) {
            let id_start = obj_start + id_rel + VIDEO_ID_MARKER.len();
            if id_start + 12 <= haystack.len() && haystack[id_start + 11] == b'"' {
                if let Ok(candidate) = std::str::from_utf8(&haystack[id_start..id_start + 11]) {
                    if is_valid_youtube_video_id(candidate) {
                        return Some(candidate.to_string());
                    }
                }
            }
        }

        offset = obj_start;
    }
    None
}

/// Streams YouTube search HTML over WinHTTP and returns immediately once the first valid 11-char videoId is seen.
pub fn search_youtube_first_video_id(query: &str) -> Option<String> {
    let clean = query.trim();
    if clean.is_empty() {
        return None;
    }

    let encoded = url_encode_query(clean);
    let path = format!("/results?search_query={}", encoded);
    let path_w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let headers_w: Vec<u16> = "Accept-Language: en-US,en;q=0.9\r\n".encode_utf16().collect();
    let user_agent =
        w!("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120.0.0.0 Safari/537.36");

    unsafe {
        let session = WinHttpOpen(
            user_agent,
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        );
        if session.is_null() {
            return None;
        }
        let _ = WinHttpSetTimeouts(session, 5000, 5000, 8000, 8000);

        let connect = WinHttpConnect(session, w!("www.youtube.com"), 443, 0);
        if connect.is_null() {
            let _ = WinHttpCloseHandle(session);
            return None;
        }

        let request = WinHttpOpenRequest(
            connect,
            w!("GET"),
            PCWSTR(path_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        );
        if request.is_null() {
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return None;
        }

        let _ = WinHttpSetTimeouts(request, 5000, 5000, 8000, 8000);
        let _ = WinHttpAddRequestHeaders(
            request,
            &headers_w,
            WINHTTP_ADDREQ_FLAG_ADD | WINHTTP_ADDREQ_FLAG_REPLACE,
        );

        let mut found_id: Option<String> = None;
        if WinHttpSendRequest(request, None, None, 0, 0, 0).is_ok()
            && WinHttpReceiveResponse(request, std::ptr::null_mut()).is_ok()
        {
            const MAX_YT_HTML_BYTES: usize = 2 * 1024 * 1024;
            const CHUNK_SIZE: usize = 16 * 1024;
            const TAIL_OVERLAP: usize = 224;

            let mut chunk = [0u8; CHUNK_SIZE];
            let mut window: Vec<u8> = Vec::with_capacity(CHUNK_SIZE + TAIL_OVERLAP);
            let mut total_read = 0usize;

            while total_read < MAX_YT_HTML_BYTES {
                let to_read = CHUNK_SIZE.min(MAX_YT_HTML_BYTES - total_read) as u32;
                let mut downloaded = 0u32;
                if WinHttpReadData(request, chunk.as_mut_ptr() as *mut _, to_read, &mut downloaded)
                    .is_err()
                    || downloaded == 0
                {
                    break;
                }
                total_read += downloaded as usize;
                window.extend_from_slice(&chunk[..downloaded as usize]);

                if let Some(id) = extract_first_youtube_video_id_bytes(&window) {
                    found_id = Some(id);
                    break;
                }

                if window.len() > TAIL_OVERLAP {
                    let drain_len = window.len() - TAIL_OVERLAP;
                    window.drain(..drain_len);
                }
            }
        }

        let _ = WinHttpCloseHandle(request);
        let _ = WinHttpCloseHandle(connect);
        let _ = WinHttpCloseHandle(session);
        found_id
    }
}

pub fn build_music_target_url(
    query: &str,
    platform: MusicPlatform,
    resolved_video_id: Option<&str>,
) -> String {
    let valid_id = resolved_video_id.filter(|id| is_valid_youtube_video_id(id));
    match platform {
        MusicPlatform::Spotify => format!("spotify:search:{}", url_encode_query(query)),
        MusicPlatform::YouTubeMusic => {
            if let Some(id) = valid_id {
                format!("https://music.youtube.com/watch?v={}", id)
            } else {
                format!("https://music.youtube.com/search?q={}", url_encode_query(query))
            }
        }
        MusicPlatform::YouTube => {
            if let Some(id) = valid_id {
                format!("https://www.youtube.com/watch?v={}", id)
            } else {
                format!(
                    "https://www.youtube.com/results?search_query={}",
                    url_encode_query(query)
                )
            }
        }
    }
}

fn music_player_config_path() -> Option<PathBuf> {
    if let Ok(appdata) = std::env::var("APPDATA") {
        let dir = PathBuf::from(appdata).join("NotDynamicIsland");
        return Some(dir.join("music_player.txt"));
    }
    std::env::var("USERPROFILE").ok().map(|home| {
        PathBuf::from(home)
            .join(".not_dynamic_island")
            .join("music_player.txt")
    })
}

fn load_music_browser_mode_from(path: &Path) -> MusicBrowserMode {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| MusicBrowserMode::parse_config(&s))
        .unwrap_or(MusicBrowserMode::Default)
}

fn save_music_browser_mode_to(path: &Path, mode: &MusicBrowserMode) -> bool {
    if matches!(mode, MusicBrowserMode::Default) {
        if path.exists() {
            return std::fs::remove_file(path).is_ok();
        }
        return true;
    }
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    std::fs::write(path, mode.to_config_string()).is_ok()
}

pub fn load_music_browser_mode() -> MusicBrowserMode {
    music_player_config_path()
        .map(|p| load_music_browser_mode_from(&p))
        .unwrap_or(MusicBrowserMode::Default)
}

pub fn save_music_browser_mode(mode: &MusicBrowserMode) -> bool {
    music_player_config_path()
        .map(|p| save_music_browser_mode_to(&p, mode))
        .unwrap_or(false)
}

/// Extracts natural-language music browser configuration commands (e.g., "set musik edge app").
pub fn extract_music_player_config_intent(prompt: &str) -> Option<MusicBrowserMode> {
    let trimmed = strip_trailing_polite_and_punct(prompt.trim());
    let lower = trimmed.to_lowercase();
    if matches!(
        lower.as_str(),
        "set musik default"
            | "set music default"
            | "reset musik"
            | "reset music"
            | "musik default"
            | "music default"
    ) {
        return Some(MusicBrowserMode::Default);
    }

    const CONFIG_PREFIXES: &[&str] = &[
        "set pemutar musik ",
        "ganti pemutar musik ",
        "set music browser ",
        "music browser ",
        "atur musik ",
        "ganti musik ",
        "set musik ",
        "set music ",
        "musik pakai ",
    ];

    let mut body = None;
    for prefix in CONFIG_PREFIXES {
        if let Some(rest) = strip_ascii_prefix_ci(trimmed, prefix) {
            body = Some(rest);
            break;
        }
    }
    let body = strip_trailing_polite_and_punct(body?.trim());
    if body.is_empty() {
        return None;
    }

    let lower_body = body.to_lowercase();
    let (raw_mode, raw_profile) = if let Some(idx) = lower_body.find(" profil ") {
        (&body[..idx], Some(&body[idx + 8..]))
    } else if let Some(idx) = lower_body.find(" profile ") {
        (&body[..idx], Some(&body[idx + 9..]))
    } else {
        (body, None)
    };

    let normalized_mode_words: Vec<String> = raw_mode
        .split_whitespace()
        .map(|w| {
            if w.eq_ignore_ascii_case("pwa") {
                "app".to_string()
            } else {
                w.to_ascii_lowercase()
            }
        })
        .collect();
    let mode_token = normalized_mode_words.join("-");
    let combined = match raw_profile.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => format!("{}:{}", mode_token, p),
        None => mode_token,
    };

    MusicBrowserMode::parse_config(&combined)
}

/// Identifies dedicated standalone Chromium `--app` / PWA YouTube windows (strictly excludes full multi-tab browsers).
pub fn is_youtube_standalone_app_title(title: &str) -> bool {
    let lower = title.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }
    if lower.contains("microsoft edge")
        || lower.contains("microsoft\u{200b} edge")
        || lower.contains(" - brave")
        || lower.contains(" - google chrome")
        || lower.contains(" - chromium")
    {
        return false;
    }
    lower == "youtube"
        || lower == "youtube music"
        || lower.ends_with(" - youtube")
        || lower.ends_with(" - youtube music")
}

fn find_youtube_standalone_app_windows(browser: ChromiumBrowser) -> Vec<HWND> {
    struct Ctx {
        target_suffix: String,
        matches: Vec<HWND>,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            let ctx = &mut *(lparam.0 as *mut Ctx);
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
            let style = GetWindowLongW(hwnd, GWL_STYLE);
            if (style & WS_CHILD.0 as i32) != 0 {
                return BOOL(1);
            }

            let mut class_buf = [0u16; 64];
            let class_len = GetClassNameW(hwnd, &mut class_buf);
            if class_len <= 0 {
                return BOOL(1);
            }
            let class_name = String::from_utf16_lossy(&class_buf[..class_len as usize]);
            if class_name != "Chrome_WidgetWin_1" {
                return BOOL(1);
            }

            let mut title_buf = [0u16; 512];
            let title_len = GetWindowTextW(hwnd, &mut title_buf);
            if title_len <= 0 {
                return BOOL(1);
            }
            let title = String::from_utf16_lossy(&title_buf[..title_len as usize]);
            if !is_youtube_standalone_app_title(&title) {
                return BOOL(1);
            }

            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 {
                return BOOL(1);
            }
            if let Ok(proc_handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut exe_buf = [0u16; 512];
                let mut size = exe_buf.len() as u32;
                let ok = QueryFullProcessImageNameW(
                    proc_handle,
                    PROCESS_NAME_FORMAT(0),
                    PWSTR(exe_buf.as_mut_ptr()),
                    &mut size,
                )
                .is_ok();
                let _ = CloseHandle(proc_handle);
                if ok && size > 0 {
                    let exe_path =
                        String::from_utf16_lossy(&exe_buf[..size as usize]).to_ascii_lowercase();
                    if exe_path.ends_with(&ctx.target_suffix) {
                        ctx.matches.push(hwnd);
                    }
                }
            }
        }
        BOOL(1)
    }

    let mut ctx = Ctx {
        target_suffix: format!("\\{}", browser.exe_name()),
        matches: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut ctx as *mut Ctx as isize));
    }
    ctx.matches
}

fn open_shell_uri(uri: &str) -> bool {
    let wide: Vec<u16> = uri.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let res = ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
        (res.0 as usize) > 32
    }
}

fn open_music_url_with_mode(url: &str, mode: &MusicBrowserMode) -> bool {
    match mode {
        MusicBrowserMode::Default => open_shell_uri(url),
        MusicBrowserMode::Browser {
            browser,
            app_mode,
            profile,
        } => {
            let old_hwnds = if *app_mode {
                find_youtube_standalone_app_windows(*browser)
            } else {
                Vec::new()
            };

            let exe_path = browser.resolve_executable();
            let exe_w: Vec<u16> = exe_path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();

            let mut args = String::new();
            if let Some(p) = profile {
                let clean_p = sanitize_browser_profile(p);
                args.push_str(&format!("--profile-directory=\"{}\" ", clean_p));
            }
            if *app_mode {
                args.push_str(&format!("--app=\"{}\"", url));
            } else {
                args.push_str(&format!("\"{}\"", url));
            }
            let args_w: Vec<u16> = args.encode_utf16().chain(std::iter::once(0)).collect();

            let launched = unsafe {
                let res = ShellExecuteW(
                    None,
                    w!("open"),
                    PCWSTR(exe_w.as_ptr()),
                    PCWSTR(args_w.as_ptr()),
                    None,
                    SW_SHOWNORMAL,
                );
                (res.0 as usize) > 32
            };

            if launched {
                if *app_mode && !old_hwnds.is_empty() {
                    // Wait briefly for the new standalone app window to spawn before closing previous one
                    std::thread::sleep(Duration::from_millis(350));
                    for old_hwnd in old_hwnds {
                        unsafe {
                            if GetWindowThreadProcessId(old_hwnd, None) != 0 {
                                let _ = PostMessageW(old_hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                            }
                        }
                    }
                }
                true
            } else {
                open_shell_uri(url)
            }
        }
    }
}

/// Resolves and launches music playback asynchronously off the UI thread, optionally posting final status back to `notify_hwnd`.
pub fn play_music_async(req: PlayMusicRequest, notify_hwnd: Option<HWND>) {
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    let notify_raw = notify_hwnd.map(|h| h.0 as isize);
    std::thread::Builder::new()
        .name("MusicPlayWorker".to_string())
        .spawn(move || {
            let mode = load_music_browser_mode();
            let (message, is_error) = match req.platform {
                MusicPlatform::Spotify => {
                    let target_url = build_music_target_url(&req.query, MusicPlatform::Spotify, None);
                    if open_shell_uri(&target_url) {
                        (
                            format!("Pencarian \"{}\" berhasil dibuka di Spotify.", req.query),
                            false,
                        )
                    } else {
                        (
                            "Gagal membuka Spotify. Pastikan aplikasi Spotify terpasang.".to_string(),
                            true,
                        )
                    }
                }
                MusicPlatform::YouTubeMusic | MusicPlatform::YouTube => {
                    let video_id = search_youtube_first_video_id(&req.query);
                    let target_url =
                        build_music_target_url(&req.query, req.platform, video_id.as_deref());
                    let launched = open_music_url_with_mode(&target_url, &mode);
                    let mode_tag = if matches!(mode, MusicBrowserMode::Default) {
                        String::new()
                    } else {
                        format!(" via {}", mode.display_label())
                    };
                    if launched {
                        if video_id.is_some() {
                            (
                                format!(
                                    "Berhasil memutar \"{}\" di {}{}.",
                                    req.query,
                                    req.platform.display_name(),
                                    mode_tag
                                ),
                                false,
                            )
                        } else {
                            (
                                format!(
                                    "Hasil pencarian \"{}\" dibuka di {}{}.",
                                    req.query,
                                    req.platform.display_name(),
                                    mode_tag
                                ),
                                false,
                            )
                        }
                    } else {
                        (
                            format!(
                                "Gagal membuka {} untuk \"{}\".",
                                req.platform.display_name(),
                                req.query
                            ),
                            true,
                        )
                    }
                }
            };

            if let Some(raw_hwnd) = notify_raw {
                let ptr = Box::into_raw(Box::new(MusicStatusPayload { message, is_error }));
                let posted = unsafe {
                    PostMessageW(
                        HWND(raw_hwnd as *mut _),
                        WM_APP_MUSIC_STATUS,
                        WPARAM(0),
                        LPARAM(ptr as isize),
                    )
                };
                if posted.is_err() {
                    unsafe {
                        drop(Box::from_raw(ptr));
                    }
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_play_music_intent_variants() {
        assert_eq!(
            extract_play_music_intent("play Midnight Serenade dari Nova Band"),
            Some(PlayMusicRequest {
                query: "Midnight Serenade Nova Band".to_string(),
                platform: MusicPlatform::YouTubeMusic,
            })
        );

        assert_eq!(
            extract_play_music_intent("tolong putarin lagu Starlight Echoes by Neon Choir dong!"),
            Some(PlayMusicRequest {
                query: "Starlight Echoes Neon Choir".to_string(),
                platform: MusicPlatform::YouTubeMusic,
            })
        );

        assert_eq!(
            extract_play_music_intent("setel Nocturne in Blue di youtube please"),
            Some(PlayMusicRequest {
                query: "Nocturne in Blue".to_string(),
                platform: MusicPlatform::YouTube,
            })
        );

        assert_eq!(
            extract_play_music_intent("putar Spring Breeze di yt music"),
            Some(PlayMusicRequest {
                query: "Spring Breeze".to_string(),
                platform: MusicPlatform::YouTubeMusic,
            })
        );

        assert_eq!(
            extract_play_music_intent("play Velvet Horizon di spotify dong"),
            Some(PlayMusicRequest {
                query: "Velvet Horizon".to_string(),
                platform: MusicPlatform::Spotify,
            })
        );

        assert_eq!(
            extract_play_music_intent("tolong putar di spotify lagu \"Golden Hour\""),
            Some(PlayMusicRequest {
                query: "Golden Hour".to_string(),
                platform: MusicPlatform::Spotify,
            })
        );

        // Non-ASCII / CJK song titles must never panic or corrupt UTF-8 slices
        assert_eq!(
            extract_play_music_intent("putar 夜に駆ける dari YOASOBI di yt music"),
            Some(PlayMusicRequest {
                query: "夜に駆ける YOASOBI".to_string(),
                platform: MusicPlatform::YouTubeMusic,
            })
        );

        // "Dari" as first word of a song title is preserved
        assert_eq!(
            extract_play_music_intent("putar lagu Dari Timur ke Barat"),
            Some(PlayMusicRequest {
                query: "Dari Timur ke Barat".to_string(),
                platform: MusicPlatform::YouTubeMusic,
            })
        );
    }

    #[test]
    fn test_extract_play_music_intent_rejects_non_music() {
        for prompt in [
            "buka spotify",
            "buka folder music",
            "setel timer 10 menit",
            "nyalakan mic",
            "play",
            "play pause",
            "play game chess",
            "apa lagu terbaik?",
            "putar spotify",
            "mainkan catur",
        ] {
            assert_eq!(
                extract_play_music_intent(prompt),
                None,
                "Should not match prompt '{}'",
                prompt
            );
        }
    }

    #[test]
    fn test_parse_action_play_music() {
        assert_eq!(
            parse_action_play_music("Midnight Serenade Nova Band"),
            Some(PlayMusicRequest {
                query: "Midnight Serenade Nova Band".to_string(),
                platform: MusicPlatform::YouTubeMusic,
            })
        );
        assert_eq!(
            parse_action_play_music("Midnight Serenade:youtube"),
            Some(PlayMusicRequest {
                query: "Midnight Serenade".to_string(),
                platform: MusicPlatform::YouTube,
            })
        );
        assert_eq!(
            parse_action_play_music("Midnight Serenade:spotify"),
            Some(PlayMusicRequest {
                query: "Midnight Serenade".to_string(),
                platform: MusicPlatform::Spotify,
            })
        );
        assert_eq!(
            parse_action_play_music("Suite: Part 1:ytmusic"),
            Some(PlayMusicRequest {
                query: "Suite: Part 1".to_string(),
                platform: MusicPlatform::YouTubeMusic,
            })
        );
        // Colon inside title without trailing platform token stays intact
        assert_eq!(
            parse_action_play_music("Suite: Part 1"),
            Some(PlayMusicRequest {
                query: "Suite: Part 1".to_string(),
                platform: MusicPlatform::YouTubeMusic,
            })
        );
    }

    #[test]
    fn test_extract_first_youtube_video_id_bytes() {
        let html = br#"<html><script>var ytInitialData = {"contents":{"adSlotRenderer":{"videoId":"ADVIDEO1234"},"videoRenderer":{"videoId":"bad"},"videoRenderer":{"videoId":"dQw4w9WgXcQ","title":{"runs":[{"text":"Sample Track"}]}}}};</script></html>"#;
        assert_eq!(
            extract_first_youtube_video_id_bytes(html).as_deref(),
            Some("dQw4w9WgXcQ")
        );

        // Simulate 16 KB chunk split right inside `"videoRenderer":{"videoId":"AbC_123-xyZ"`
        let chunk1 = br#"prefix noise "videoRenderer":{"videoId"#;
        let chunk2 = br#"":"AbC_123-xyZ","title":"Sample"}"#;
        let mut window = Vec::new();
        window.extend_from_slice(chunk1);
        assert_eq!(extract_first_youtube_video_id_bytes(&window), None);
        let tail = window.len().saturating_sub(224);
        window.drain(..tail);
        window.extend_from_slice(chunk2);
        assert_eq!(
            extract_first_youtube_video_id_bytes(&window).as_deref(),
            Some("AbC_123-xyZ")
        );
    }

    #[test]
    fn test_build_music_target_url() {
        assert_eq!(
            build_music_target_url("Sample Track", MusicPlatform::YouTubeMusic, Some("dQw4w9WgXcQ")),
            "https://music.youtube.com/watch?v=dQw4w9WgXcQ"
        );
        assert_eq!(
            build_music_target_url("Sample Track", MusicPlatform::YouTube, Some("dQw4w9WgXcQ")),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        );
        // Invalid ID falls back to search URL safely
        assert_eq!(
            build_music_target_url("Sample Track", MusicPlatform::YouTubeMusic, Some("bad&inject=1")),
            "https://music.youtube.com/search?q=Sample%20Track"
        );
        assert_eq!(
            build_music_target_url("Sample Track", MusicPlatform::YouTube, None),
            "https://www.youtube.com/results?search_query=Sample%20Track"
        );
        assert_eq!(
            build_music_target_url("Sample Track", MusicPlatform::Spotify, Some("dQw4w9WgXcQ")),
            "spotify:search:Sample%20Track"
        );
    }

    #[test]
    fn test_music_browser_mode_parse_and_roundtrip() {
        assert_eq!(
            MusicBrowserMode::parse_config("edge-app"),
            Some(MusicBrowserMode::Browser {
                browser: ChromiumBrowser::Edge,
                app_mode: true,
                profile: None,
            })
        );
        assert_eq!(
            MusicBrowserMode::parse_config("edge-app:Default"),
            Some(MusicBrowserMode::Browser {
                browser: ChromiumBrowser::Edge,
                app_mode: true,
                profile: Some("Default".to_string()),
            })
        );
        assert_eq!(
            MusicBrowserMode::parse_config("brave"),
            Some(MusicBrowserMode::Browser {
                browser: ChromiumBrowser::Brave,
                app_mode: false,
                profile: None,
            })
        );
        assert_eq!(
            MusicBrowserMode::parse_config("chrome-app"),
            Some(MusicBrowserMode::Browser {
                browser: ChromiumBrowser::Chrome,
                app_mode: true,
                profile: None,
            })
        );
        assert_eq!(
            MusicBrowserMode::parse_config("default"),
            Some(MusicBrowserMode::Default)
        );
        assert_eq!(MusicBrowserMode::parse_config("unknown-browser"), None);

        let cfg_path = std::env::temp_dir().join(format!(
            "ndi_music_cfg_test_{}.txt",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&cfg_path);

        let edge_mode = MusicBrowserMode::Browser {
            browser: ChromiumBrowser::Edge,
            app_mode: true,
            profile: Some("Default".to_string()),
        };
        assert!(save_music_browser_mode_to(&cfg_path, &edge_mode));
        assert_eq!(load_music_browser_mode_from(&cfg_path), edge_mode);

        assert!(save_music_browser_mode_to(&cfg_path, &MusicBrowserMode::Default));
        assert!(!cfg_path.exists());
        assert_eq!(load_music_browser_mode_from(&cfg_path), MusicBrowserMode::Default);
    }

    #[test]
    fn test_extract_music_player_config_intent() {
        assert_eq!(
            extract_music_player_config_intent("set musik edge app"),
            Some(MusicBrowserMode::Browser {
                browser: ChromiumBrowser::Edge,
                app_mode: true,
                profile: None,
            })
        );
        assert_eq!(
            extract_music_player_config_intent("set musik edge pwa dong"),
            Some(MusicBrowserMode::Browser {
                browser: ChromiumBrowser::Edge,
                app_mode: true,
                profile: None,
            })
        );
        assert_eq!(
            extract_music_player_config_intent("set musik edge app profil Work"),
            Some(MusicBrowserMode::Browser {
                browser: ChromiumBrowser::Edge,
                app_mode: true,
                profile: Some("Work".to_string()),
            })
        );
        assert_eq!(
            extract_music_player_config_intent("set musik default ya"),
            Some(MusicBrowserMode::Default)
        );
        assert_eq!(
            extract_music_player_config_intent("reset musik"),
            Some(MusicBrowserMode::Default)
        );
        assert_eq!(
            extract_music_player_config_intent("play Midnight Serenade di youtube"),
            None
        );
    }

    #[test]
    fn test_is_youtube_standalone_app_title() {
        assert!(is_youtube_standalone_app_title("Sample Song - YouTube"));
        assert!(is_youtube_standalone_app_title("Sample Song - YouTube Music"));
        assert!(is_youtube_standalone_app_title("YouTube"));
        assert!(is_youtube_standalone_app_title("YouTube Music"));

        // Must never match regular multi-tab browser windows
        assert!(!is_youtube_standalone_app_title("Sample Song - YouTube - Personal - Microsoft Edge"));
        assert!(!is_youtube_standalone_app_title("Sample Song - YouTube - Personal - Microsoft\u{200b} Edge"));
        assert!(!is_youtube_standalone_app_title("Sample Song - YouTube - Brave"));
        assert!(!is_youtube_standalone_app_title("Sample Song - YouTube - Google Chrome"));
        assert!(!is_youtube_standalone_app_title("Untitled - Notepad"));
        assert!(!is_youtube_standalone_app_title(""));
    }
}
