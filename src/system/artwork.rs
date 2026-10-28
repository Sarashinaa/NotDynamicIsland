//! Online Artwork Fetcher & Cacher
//! Fetches posters/thumbnails for video streaming (e.g. Netflix, Disney+, Prime Video, movies & TV series)
//! using 100% free, public APIs (TVMaze & Wikipedia) via native WinHTTP and Windows Imaging Component (WIC).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::thread;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
    WINHTTP_FLAG_SECURE,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::system::media::{decode_image_wic, MediaTrackInfo, WM_APP_MEDIA_CHANGED};

/// Global artwork manager for streaming media (Netflix, etc.)
pub struct ArtworkManager {
    cache: Arc<Mutex<HashMap<String, Option<(Vec<u32>, usize, usize)>>>>,
    in_flight: Arc<Mutex<HashSet<String>>>,
}

impl ArtworkManager {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            in_flight: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Checks if artwork for the given title is already cached in memory
    pub fn get_cached(&self, title: &str) -> Option<(Vec<u32>, usize, usize)> {
        let clean = clean_media_title(title);
        if clean.is_empty() {
            return None;
        }
        let cache = self.cache.lock().ok()?;
        cache.get(&clean).cloned().flatten()
    }

    /// Spawns a background thread to fetch online artwork if not already in-flight or cached
    pub fn fetch_for_media(
        &self,
        hwnd: isize,
        raw_title: &str,
        raw_artist: &str,
        info_arc: Arc<Mutex<MediaTrackInfo>>,
    ) {
        let clean = clean_media_title(raw_title);
        if clean.is_empty() {
            return;
        }

        // Avoid re-fetching if already cached or in-flight
        {
            if let Ok(cache) = self.cache.lock() {
                if cache.contains_key(&clean) {
                    return;
                }
            }
            let mut in_flight = match self.in_flight.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            if in_flight.contains(&clean) {
                return;
            }
            in_flight.insert(clean.clone());
        }

        let cache_arc = self.cache.clone();
        let in_flight_arc = self.in_flight.clone();
        let target_title = raw_title.to_string();
        let target_artist = raw_artist.to_string();

        thread::spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }

            let result = fetch_artwork_online(&clean, &target_artist);

            // Store in cache
            if let Ok(mut cache) = cache_arc.lock() {
                cache.insert(clean.clone(), result.clone());
            }

            // Remove from in-flight
            if let Ok(mut in_flight) = in_flight_arc.lock() {
                in_flight.remove(&clean);
            }

            // If artwork was found, update current MediaTrackInfo and notify window
            if let Some((pixels, w, h)) = result {
                let mut updated = false;
                if let Ok(mut info) = info_arc.lock() {
                    // Only update if the media title hasn't changed to another show in the meantime
                    if info.title == target_title {
                        info.artwork_pixels = Some(pixels);
                        info.artwork_w = w;
                        info.artwork_h = h;
                        updated = true;
                    }
                }

                if updated {
                    unsafe {
                        let _ = PostMessageW(
                            HWND(hwnd as *mut _),
                            WM_APP_MEDIA_CHANGED,
                            WPARAM(0),
                            LPARAM(0),
                        );
                    }
                }
            }
        });
    }
}

/// Cleans raw streaming title to extract the core show or movie name.
/// Examples:
/// - "Stranger Things: Season 4: Chapter One - Netflix" -> "Stranger Things"
/// - "Squid Game: Season 1" -> "Squid Game"
/// - "Glass Onion: A Knives Out Mystery - Netflix" -> "Glass Onion: A Knives Out Mystery"
/// - "Watch Extraction 2 | Netflix" -> "Extraction 2"
/// - "Avatar: The Way of Water (2022)" -> "Avatar: The Way of Water"
pub fn clean_media_title(raw: &str) -> String {
    let mut title = raw.trim().to_string();

    // 1. Strip common streaming site suffixes
    let suffixes = [
        " - Netflix", " | Netflix", " - Watch on Netflix", " | Watch on Netflix",
        " - YouTube", " | YouTube", " - Disney+", " | Disney+",
        " - Prime Video", " | Prime Video", " - HBO Max", " | HBO Max",
        " - Apple TV", " | Apple TV", " - Hulu", " | Hulu",
    ];
    for suf in &suffixes {
        if let Some(idx) = title.to_lowercase().rfind(&suf.to_lowercase()) {
            title = title[..idx].trim().to_string();
        }
    }

    // 2. Strip browser prefixes like "Watch "
    if title.to_lowercase().starts_with("watch ") {
        title = title[6..].trim().to_string();
    }

    // 3. Strip trailing 4-digit year in parentheses: e.g. " (2022)" or " (1999)"
    if title.ends_with(')') {
        if let Some(open) = title.rfind('(') {
            let inner = &title[open + 1..title.len() - 1].trim();
            if inner.len() == 4 && inner.chars().all(|c| c.is_ascii_digit()) {
                title = title[..open].trim().to_string();
            }
        }
    }

    // 4. Strip season / episode markers if present
    let season_markers = [
        ": Season", " - Season", ": season", " - season",
        ": Series", " - Series",
        ": Part", " - Part",
        ": Chapter", " - Chapter",
        ": Episode", " - Episode",
        " - S0", " - S1", " - S2", " - S3", " - S4", " - S5",
        ": S0", ": S1", ": S2", ": S3", ": S4", ": S5",
        " S0", " S1", " S2", " S3", " S4", " S5",
    ];

    for marker in &season_markers {
        if let Some(pos) = title.find(marker) {
            let candidate = title[..pos].trim();
            // Don't strip if it leaves title empty or too short
            if candidate.len() >= 2 {
                title = candidate.to_string();
                break;
            }
        }
    }

    // Reject generic names that are not real movie/show titles
    let lower = title.to_lowercase();
    if lower == "netflix" || lower == "chrome" || lower == "msedge" || lower == "firefox" || lower == "opera" || lower.is_empty() {
        return String::new();
    }

    title
}

/// Searches TVMaze and Wikipedia for artwork and returns decoded 48x48 BGRA pixels
fn fetch_artwork_online(title: &str, _artist: &str) -> Option<(Vec<u32>, usize, usize)> {
    if title.is_empty() {
        return None;
    }

    // --- Strategy 1: TVMaze API (Best for TV shows, anime & series) ---
    let tvmaze_url = format!(
        "https://api.tvmaze.com/singlesearch/shows?q={}",
        url_encode(title)
    );
    if let Some(body) = winhttp_get_bytes(&tvmaze_url) {
        if let Ok(json) = std::str::from_utf8(&body) {
            if let Some(img_url) = extract_tvmaze_image(json) {
                if let Some(img_bytes) = winhttp_get_bytes(&img_url) {
                    if let Some(decoded) = decode_image_wic(&img_bytes, 48, 48) {
                        return Some(decoded);
                    }
                }
            }
        }
    }

    // --- Strategy 2: Wikipedia REST API Summary (Best for movies & global series) ---
    let wiki_title = title.replace(' ', "_");
    let wiki_url = format!(
        "https://en.wikipedia.org/api/rest_v1/page/summary/{}",
        url_encode(&wiki_title)
    );
    if let Some(body) = winhttp_get_bytes(&wiki_url) {
        if let Ok(json) = std::str::from_utf8(&body) {
            if let Some(img_url) = extract_wikipedia_thumbnail(json) {
                if let Some(img_bytes) = winhttp_get_bytes(&img_url) {
                    if let Some(decoded) = decode_image_wic(&img_bytes, 48, 48) {
                        return Some(decoded);
                    }
                }
            }
        }
    }

    // --- Strategy 3: Wikipedia Opensearch Fallback (for slight spelling or subtitle differences) ---
    let search_url = format!(
        "https://en.wikipedia.org/w/api.php?action=opensearch&search={}&limit=1&format=json",
        url_encode(title)
    );
    if let Some(body) = winhttp_get_bytes(&search_url) {
        if let Ok(json) = std::str::from_utf8(&body) {
            if let Some(best_title) = extract_wikipedia_opensearch_title(json) {
                let best_wiki = best_title.replace(' ', "_");
                let sum_url = format!(
                    "https://en.wikipedia.org/api/rest_v1/page/summary/{}",
                    url_encode(&best_wiki)
                );
                if let Some(sum_body) = winhttp_get_bytes(&sum_url) {
                    if let Ok(sum_json) = std::str::from_utf8(&sum_body) {
                        if let Some(img_url) = extract_wikipedia_thumbnail(sum_json) {
                            if let Some(img_bytes) = winhttp_get_bytes(&img_url) {
                                if let Some(decoded) = decode_image_wic(&img_bytes, 48, 48) {
                                    return Some(decoded);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

/// Parses https://<host>/<path> into (&str, &str)
fn parse_https_url(url: &str) -> Option<(&str, &str)> {
    let stripped = url.strip_prefix("https://")?;
    let slash_pos = stripped.find('/')?;
    let host = &stripped[..slash_pos];
    let path = &stripped[slash_pos..];
    Some((host, path))
}

/// Performs a secure GET request via native WinHTTP and returns raw response body bytes
fn winhttp_get_bytes(url: &str) -> Option<Vec<u8>> {
    let (host, path) = parse_https_url(url)?;
    let host_wide: Vec<u16> = host.encode_utf16().chain(std::iter::once(0)).collect();
    let path_wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let session = WinHttpOpen(
            w!("NotDynamicIsland/1.0"),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            None,
            None,
            0,
        );
        if session.is_null() {
            return None;
        }

        let connect = WinHttpConnect(session, PCWSTR(host_wide.as_ptr()), 443, 0);
        if connect.is_null() {
            let _ = WinHttpCloseHandle(session);
            return None;
        }

        let request = WinHttpOpenRequest(
            connect,
            w!("GET"),
            PCWSTR(path_wide.as_ptr()),
            None,
            None,
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        );

        let mut body = Vec::new();
        if !request.is_null() {
            if WinHttpSendRequest(request, None, None, 0, 0, 0).is_ok() {
                if WinHttpReceiveResponse(request, std::ptr::null_mut()).is_ok() {
                    let mut buf = [0u8; 8192];
                    let mut read_bytes = 0u32;
                    while WinHttpReadData(
                        request,
                        buf.as_mut_ptr() as *mut _,
                        buf.len() as u32,
                        &mut read_bytes,
                    )
                    .is_ok()
                    {
                        if read_bytes == 0 {
                            break;
                        }
                        body.extend_from_slice(&buf[..read_bytes as usize]);
                        // Safeguard max 5 MB for poster image
                        if body.len() > 5 * 1024 * 1024 {
                            break;
                        }
                    }
                }
            }
            let _ = WinHttpCloseHandle(request);
        }

        let _ = WinHttpCloseHandle(connect);
        let _ = WinHttpCloseHandle(session);

        if !body.is_empty() {
            Some(body)
        } else {
            None
        }
    }
}

/// Extracts image URL from TVMaze JSON response
fn extract_tvmaze_image(json: &str) -> Option<String> {
    let img_pos = json.find("\"image\":")?;
    let sub = &json[img_pos..];
    if let Some(m_pos) = sub.find("\"medium\":\"") {
        let url_part = &sub[m_pos + 10..];
        let end = url_part.find('"')?;
        return Some(url_part[..end].replace("\\/", "/"));
    }
    if let Some(o_pos) = sub.find("\"original\":\"") {
        let url_part = &sub[o_pos + 12..];
        let end = url_part.find('"')?;
        return Some(url_part[..end].replace("\\/", "/"));
    }
    None
}

/// Extracts thumbnail source URL from Wikipedia Summary JSON response
fn extract_wikipedia_thumbnail(json: &str) -> Option<String> {
    let thumb_pos = json.find("\"thumbnail\":")?;
    let sub = &json[thumb_pos..];
    let src_pos = sub.find("\"source\":\"")?;
    let url_part = &sub[src_pos + 10..];
    let end = url_part.find('"')?;
    Some(url_part[..end].replace("\\/", "/"))
}

/// Extracts top title from Wikipedia Opensearch JSON response `["query",["Title"],...]`
fn extract_wikipedia_opensearch_title(json: &str) -> Option<String> {
    let first_comma = json.find(",[")?;
    let titles_sub = &json[first_comma + 2..];
    let start_quote = titles_sub.find('"')?;
    let title_part = &titles_sub[start_quote + 1..];
    let end_quote = title_part.find('"')?;
    let title = &title_part[..end_quote];
    if !title.is_empty() {
        Some(title.to_string())
    } else {
        None
    }
}

/// Lightweight URL encoder for query parameters
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_media_title() {
        assert_eq!(
            clean_media_title("Stranger Things: Season 4: Chapter One - Netflix"),
            "Stranger Things"
        );
        assert_eq!(
            clean_media_title("Squid Game: Season 1"),
            "Squid Game"
        );
        assert_eq!(
            clean_media_title("Glass Onion: A Knives Out Mystery - Netflix"),
            "Glass Onion: A Knives Out Mystery"
        );
        assert_eq!(
            clean_media_title("Watch Extraction 2 | Netflix"),
            "Extraction 2"
        );
        assert_eq!(
            clean_media_title("Avatar: The Way of Water (2022)"),
            "Avatar: The Way of Water"
        );
    }
}
