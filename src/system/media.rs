//! Universal Media Controller
//! Hybrid architecture: WinRT GSMTC + Spotify/Window Title Fallback + Global Media Keys.
//! Excludes self process and non-media windows.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::system::artwork::ArtworkManager;
use crate::system::lyrics::{LyricsManager, SyncedLyrics};

use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession,
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    keybd_event, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VK_MEDIA_NEXT_TRACK,
    VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    PostMessageW,
};

pub const WM_APP_MEDIA_CHANGED: u32 = 0x8002;

use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA, IWICBitmapScaler, IWICFormatConverter,
    IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant,
    WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::CLSCTX_INPROC_SERVER;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaTrackInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub is_playing: bool,
    pub has_media: bool,
    pub position_secs: u32,
    pub duration_secs: u32,
    pub artwork_pixels: Option<Vec<u32>>,
    pub artwork_w: usize,
    pub artwork_h: usize,
}

pub(crate) fn decode_image_wic(buffer: &[u8], target_w: usize, target_h: usize) -> Option<(Vec<u32>, usize, usize)> {
    unsafe {
        let factory: IWICImagingFactory =
            windows::Win32::System::Com::CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .ok()?;
        let stream = factory.CreateStream().ok()?;
        stream.InitializeFromMemory(buffer).ok()?;

        let decoder = factory
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;

        let scaler: IWICBitmapScaler = factory.CreateBitmapScaler().ok()?;
        scaler
            .Initialize(&frame, target_w as u32, target_h as u32, WICBitmapInterpolationModeFant)
            .ok()?;

        let converter: IWICFormatConverter = factory.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &scaler,
                &GUID_WICPixelFormat32bppBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;

        let mut pixels = vec![0u32; target_w * target_h];
        let stride = (target_w * 4) as u32;
        converter
            .CopyPixels(
                std::ptr::null(),
                stride,
                std::slice::from_raw_parts_mut(pixels.as_mut_ptr() as *mut u8, pixels.len() * 4),
            )
            .ok()?;

        Some((pixels, target_w, target_h))
    }
}

fn extract_artwork_from_props(
    props: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties,
) -> Option<(Vec<u32>, usize, usize)> {
    let thumb_ref = props.Thumbnail().ok()?;
    let stream = thumb_ref.OpenReadAsync().ok()?.get().ok()?;
    let size = stream.Size().ok()? as usize;
    if size == 0 || size > 15 * 1024 * 1024 {
        return None;
    }
    let reader = windows::Storage::Streams::DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size as u32).ok()?.get().ok()?;
    let mut buffer = vec![0u8; size];
    reader.ReadBytes(&mut buffer).ok()?;
    decode_image_wic(&buffer, 48, 48)
}

pub struct MediaController {
    hwnd: isize,
    current_info: Arc<Mutex<MediaTrackInfo>>,
    session: Arc<Mutex<Option<GlobalSystemMediaTransportControlsSession>>>,
    running: Arc<AtomicBool>,
    lyrics_mgr: Arc<LyricsManager>,
    artwork_mgr: Arc<ArtworkManager>,
}

impl MediaController {
    pub fn new(hwnd: HWND) -> Self {
        Self {
            hwnd: hwnd.0 as isize,
            current_info: Arc::new(Mutex::new(MediaTrackInfo::default())),
            session: Arc::new(Mutex::new(None)),
            running: Arc::new(AtomicBool::new(true)),
            lyrics_mgr: Arc::new(LyricsManager::new()),
            artwork_mgr: Arc::new(ArtworkManager::new()),
        }
    }

    pub fn start(&self) {
        let hwnd_raw = self.hwnd;
        let info_arc = self.current_info.clone();
        let session_arc = self.session.clone();
        let running = self.running.clone();
        let lyrics_mgr = self.lyrics_mgr.clone();
        let artwork_mgr = self.artwork_mgr.clone();

        thread::spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }

            let mut last_info = MediaTrackInfo::default();
            let mut cached_artwork: Option<(String, String, Option<Vec<u32>>, usize, usize)> = None;
            let mut gsmtc_manager: Option<GlobalSystemMediaTransportControlsSessionManager> =
                GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                    .ok()
                    .and_then(|op| op.get().ok());

            while running.load(Ordering::Relaxed) {
                let mut detected = MediaTrackInfo::default();
                let mut found_session: Option<GlobalSystemMediaTransportControlsSession> = None;

                if gsmtc_manager.is_none() {
                    gsmtc_manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                        .ok()
                        .and_then(|op| op.get().ok());
                }

                // 1. Try WinRT GSMTC
                if let Some(manager) = &gsmtc_manager {
                    if let Ok(sessions) = manager.GetSessions() {
                        for s in sessions {
                            if let Ok(info) = s.GetPlaybackInfo() {
                                let is_playing = info.PlaybackStatus()
                                    == Ok(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing);

                                if let Ok(props_op) = s.TryGetMediaPropertiesAsync() {
                                    if let Ok(props) = props_op.get() {
                                        let title = props.Title().map(|t| t.to_string_lossy()).unwrap_or_default();
                                        let artist = props.Artist().map(|a| a.to_string_lossy()).unwrap_or_default();
                                        let album = props.AlbumTitle().map(|a| a.to_string_lossy()).unwrap_or_default();

                                        let mut pos_secs = 0u32;
                                        let mut dur_secs = 0u32;
                                        if let Ok(tl) = s.GetTimelineProperties() {
                                            if let Ok(pos) = tl.Position() {
                                                pos_secs = (pos.Duration / 10_000_000) as u32;
                                            }
                                            if let Ok(end) = tl.EndTime() {
                                                dur_secs = (end.Duration / 10_000_000) as u32;
                                            }
                                        }

                                        if !title.is_empty() {
                                            let (artwork_pixels, artwork_w, artwork_h) = match &cached_artwork {
                                                Some((t, a, Some(art), w, h)) if t == &title && a == &artist => {
                                                    (Some(art.clone()), *w, *h)
                                                }
                                                _ => {
                                                    let decoded = extract_artwork_from_props(&props);
                                                    if let Some((p, w, h)) = decoded {
                                                        cached_artwork = Some((title.clone(), artist.clone(), Some(p.clone()), w, h));
                                                        (Some(p), w, h)
                                                    } else if let Some((p, w, h)) = artwork_mgr.get_cached(&title) {
                                                        cached_artwork = Some((title.clone(), artist.clone(), Some(p.clone()), w, h));
                                                        (Some(p), w, h)
                                                    } else {
                                                        artwork_mgr.fetch_for_media(hwnd_raw, &title, &artist, info_arc.clone());
                                                        cached_artwork = Some((title.clone(), artist.clone(), None, 0, 0));
                                                        (None, 0, 0)
                                                    }
                                                }
                                            };

                                            detected = MediaTrackInfo {
                                                title,
                                                artist,
                                                album,
                                                is_playing,
                                                has_media: true,
                                                position_secs: pos_secs,
                                                duration_secs: dur_secs,
                                                artwork_pixels,
                                                artwork_w,
                                                artwork_h,
                                            };
                                            found_session = Some(s.clone());
                                            if is_playing {
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Fallback to GetCurrentSession
                    if !detected.has_media {
                        if let Ok(cur) = manager.GetCurrentSession() {
                            let is_playing = cur
                                .GetPlaybackInfo()
                                .and_then(|i| i.PlaybackStatus())
                                .map(|st| st == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
                                .unwrap_or(false);

                            if let Ok(props_op) = cur.TryGetMediaPropertiesAsync() {
                                if let Ok(props) = props_op.get() {
                                    let title = props.Title().map(|t| t.to_string_lossy()).unwrap_or_default();
                                    let artist = props.Artist().map(|a| a.to_string_lossy()).unwrap_or_default();
                                    let album = props.AlbumTitle().map(|a| a.to_string_lossy()).unwrap_or_default();

                                    let mut pos_secs = 0u32;
                                    let mut dur_secs = 0u32;
                                    if let Ok(tl) = cur.GetTimelineProperties() {
                                        if let Ok(pos) = tl.Position() {
                                            pos_secs = (pos.Duration / 10_000_000) as u32;
                                        }
                                        if let Ok(end) = tl.EndTime() {
                                            dur_secs = (end.Duration / 10_000_000) as u32;
                                        }
                                    }

                                    if !title.is_empty() {
                                        let (artwork_pixels, artwork_w, artwork_h) = match &cached_artwork {
                                            Some((t, a, Some(art), w, h)) if t == &title && a == &artist => {
                                                (Some(art.clone()), *w, *h)
                                            }
                                            _ => {
                                                let decoded = extract_artwork_from_props(&props);
                                                if let Some((p, w, h)) = decoded {
                                                    cached_artwork = Some((title.clone(), artist.clone(), Some(p.clone()), w, h));
                                                    (Some(p), w, h)
                                                } else if let Some((p, w, h)) = artwork_mgr.get_cached(&title) {
                                                    cached_artwork = Some((title.clone(), artist.clone(), Some(p.clone()), w, h));
                                                    (Some(p), w, h)
                                                } else {
                                                    artwork_mgr.fetch_for_media(hwnd_raw, &title, &artist, info_arc.clone());
                                                    cached_artwork = Some((title.clone(), artist.clone(), None, 0, 0));
                                                    (None, 0, 0)
                                                }
                                            }
                                        };

                                        detected = MediaTrackInfo {
                                            title,
                                            artist,
                                            album,
                                            is_playing,
                                            has_media: true,
                                            position_secs: pos_secs,
                                            duration_secs: dur_secs,
                                            artwork_pixels,
                                            artwork_w,
                                            artwork_h,
                                        };
                                        found_session = Some(cur);
                                    }
                                }
                            }
                        }
                    }
                }

                // 2. Fallback: Spotify Window Title scan (strictly excluding our own process and non-media windows!)
                if !detected.has_media {
                    if let Some(win_track) = scan_media_windows() {
                        detected = win_track;
                    }
                }

                // Update session reference
                if let Ok(mut s_lock) = session_arc.lock() {
                    *s_lock = found_session;
                }

                // If media title or artist changed, trigger lyrics and artwork fetch
                if detected.has_media && (!detected.title.is_empty() && (detected.title != last_info.title || detected.artist != last_info.artist)) {
                    lyrics_mgr.fetch_for_track(&detected.title, &detected.artist);
                    if detected.artwork_pixels.is_none() {
                        artwork_mgr.fetch_for_media(hwnd_raw, &detected.title, &detected.artist, info_arc.clone());
                    }
                }

                // If changed, update and notify window
                if detected != last_info {
                    last_info = detected.clone();
                    if let Ok(mut lock) = info_arc.lock() {
                        *lock = detected;
                    }
                    unsafe {
                        let _ = PostMessageW(
                            HWND(hwnd_raw as *mut _),
                            WM_APP_MEDIA_CHANGED,
                            WPARAM(0),
                            LPARAM(0),
                        );
                    }
                }

                thread::sleep(Duration::from_millis(1000));
            }
        });
    }

    pub fn get_info(&self) -> MediaTrackInfo {
        self.current_info
            .lock()
            .map(|l| l.clone())
            .unwrap_or_default()
    }

    pub fn get_lyrics(&self) -> SyncedLyrics {
        self.lyrics_mgr.get_lyrics()
    }

    pub fn toggle_play_pause(&self) {
        if let Ok(lock) = self.session.lock() {
            if let Some(sess) = &*lock {
                let _ = sess.TryTogglePlayPauseAsync();
            }
        }
        unsafe {
            keybd_event(VK_MEDIA_PLAY_PAUSE.0 as u8, 0, KEYEVENTF_EXTENDEDKEY, 0);
            keybd_event(VK_MEDIA_PLAY_PAUSE.0 as u8, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, 0);
        }
    }

    pub fn skip_next(&self) {
        if let Ok(lock) = self.session.lock() {
            if let Some(sess) = &*lock {
                let _ = sess.TrySkipNextAsync();
            }
        }
        unsafe {
            keybd_event(VK_MEDIA_NEXT_TRACK.0 as u8, 0, KEYEVENTF_EXTENDEDKEY, 0);
            keybd_event(VK_MEDIA_NEXT_TRACK.0 as u8, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, 0);
        }
    }

    pub fn skip_previous(&self) {
        if let Ok(lock) = self.session.lock() {
            if let Some(sess) = &*lock {
                let _ = sess.TrySkipPreviousAsync();
            }
        }
        unsafe {
            keybd_event(VK_MEDIA_PREV_TRACK.0 as u8, 0, KEYEVENTF_EXTENDEDKEY, 0);
            keybd_event(VK_MEDIA_PREV_TRACK.0 as u8, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, 0);
        }
    }

    pub fn seek_to(&self, position_secs: u32) {
        if let Ok(lock) = self.session.lock() {
            if let Some(sess) = &*lock {
                let pos_nanos = (position_secs as i64) * 10_000_000;
                let _ = sess.TryChangePlaybackPositionAsync(pos_nanos);
            }
        }
        if let Ok(mut lock) = self.current_info.lock() {
            lock.position_secs = position_secs;
        }
    }
}

impl Drop for MediaController {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

/// Scans top-level windows specifically for Spotify and active YouTube tabs.
/// Strictly excludes our own process and standard application windows (VSCode, Word, Browser tabs without media).
fn scan_media_windows() -> Option<MediaTrackInfo> {
    struct ScanResult {
        track: Option<MediaTrackInfo>,
        my_pid: u32,
    }

    let my_pid = unsafe { GetCurrentProcessId() };
    let mut result = ScanResult { track: None, my_pid };

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let res_ptr = lparam.0 as *mut ScanResult;
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }

            // Exclude our own window!
            let mut wnd_pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut wnd_pid));
            if wnd_pid == (*res_ptr).my_pid {
                return BOOL(1);
            }

            // Get Window Class Name
            let mut class_buf = [0u16; 256];
            let class_len = GetClassNameW(hwnd, &mut class_buf);
            let class_name = String::from_utf16_lossy(&class_buf[..class_len as usize]);

            let mut title_buf = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut title_buf);
            if len > 0 {
                let title = String::from_utf16_lossy(&title_buf[..len as usize]);
                let lower = title.to_lowercase();

                // 1. Spotify Window: class Chrome_WidgetWin_0
                if class_name.contains("Chrome_WidgetWin_0") {
                    if lower == "spotify" || lower.starts_with("spotify free") || lower.starts_with("spotify premium") {
                        // Spotify is open but stopped/idle - not playing any song!
                        return BOOL(1);
                    }
                    if title.contains(" - ") {
                        let parts: Vec<&str> = title.splitn(2, " - ").collect();
                        if parts.len() == 2 {
                            let artist = parts[0].trim().to_string();
                            let song = parts[1].trim().to_string();
                            if !song.is_empty() && !artist.is_empty() {
                                (*res_ptr).track = Some(MediaTrackInfo {
                                    title: song,
                                    artist,
                                    album: String::new(),
                                    is_playing: true,
                                    has_media: true,
                                    position_secs: 0,
                                    duration_secs: 0,
                                    artwork_pixels: None,
                                    artwork_w: 0,
                                    artwork_h: 0,
                                });
                                return BOOL(0); // Found Spotify!
                            }
                        }
                    }
                }

                // 2. YouTube in browser: ends with "- YouTube"
                if title.ends_with("- YouTube") {
                    let clean_title = title.trim_end_matches("- YouTube").trim();
                    if clean_title.contains(" - ") {
                        let parts: Vec<&str> = clean_title.splitn(2, " - ").collect();
                        if parts.len() == 2 {
                            (*res_ptr).track = Some(MediaTrackInfo {
                                title: parts[1].trim().to_string(),
                                artist: parts[0].trim().to_string(),
                                album: "YouTube".to_string(),
                                is_playing: true,
                                has_media: true,
                                position_secs: 0,
                                duration_secs: 0,
                                artwork_pixels: None,
                                artwork_w: 0,
                                artwork_h: 0,
                            });
                            return BOOL(0);
                        }
                    } else if !clean_title.is_empty() {
                        (*res_ptr).track = Some(MediaTrackInfo {
                            title: clean_title.to_string(),
                            artist: "YouTube".to_string(),
                            album: String::new(),
                            is_playing: true,
                            has_media: true,
                            position_secs: 0,
                            duration_secs: 0,
                            artwork_pixels: None,
                            artwork_w: 0,
                            artwork_h: 0,
                        });
                        return BOOL(0);
                    }
                }
            }
            BOOL(1)
        }
    }

    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut result as *mut _ as isize));
    }

    result.track
}
