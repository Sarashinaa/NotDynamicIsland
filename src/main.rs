//! Project Nova: High-Performance Dynamic Island for Windows 11
//! Architecture: 100% Native Rust, mimalloc, CPU Software Renderer, Event-Driven, Zero Polling.
//! Follows design specification in follow_this_design (Resting, Closed, Music, Lyrics, Shelf, To-do, Notifications)

#![windows_subsystem = "windows"]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

mod physics;
mod render;
mod system;
mod ui;
mod window;

use physics::SpringRect;
use render::{BackdropBlur, PixelBuffer, TextRenderer};
use system::{
    enable_eco_qos, trim_working_set, AssistantManager, BrightnessListener, DisplayManager,
    DisplayTarget, KeyboardHook, MediaController, NetworkListener, NotificationListener,
    PowerListener, PrivacyListener, ScreenshotWatcher, ToastNotificationPayload, VolumeListener,
    WeatherManager, clear_transition_pending, update_display_hook, WM_APP_ASSISTANT_REPLY,
    WM_APP_BRIGHTNESS_CHANGED, WM_APP_CALENDAR_UPDATED, WM_APP_CAPSLOCK_CHANGED,
    WM_APP_DISPLAY_CURSOR_MOVED, WM_APP_MEDIA_CHANGED, WM_APP_NETWORK_CHANGED,
    WM_APP_NETWORK_TOPOLOGY_CHANGED, WM_APP_NOTIFICATION_ARRIVED, WM_APP_POWER_CHANGED,
    WM_APP_PRIVACY_CHANGED, WM_APP_SHELF_ITEM_READY, WM_APP_TIME_TICK, WM_APP_TODO_UPDATED,
    WM_APP_VOLUME_CHANGED, WM_APP_WEATHER_UPDATED,
};
use ui::{ClickAction, ClipboardKind, ExpandedPage, IslandMode, IslandState, LayoutManager, Rect};
use window::{Win32LayeredWindow, CANVAS_H, CANVAS_W, TIMER_ANIMATION_ID};

use windows::Win32::Foundation::{
    GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, GetClipboardData, IsClipboardFormatAvailable,
    OpenClipboard, RemoveClipboardFormatListener,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::UI::Shell::{DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, BringWindowToTop, CreatePopupMenu, DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW,
    FindWindowW, GetCursorPos, GetMessageW, IsIconic, PostQuitMessage, RegisterShellHookWindow, RegisterWindowMessageW,
    SetForegroundWindow, SetWindowPos, ShowWindow, SetTimer, TrackPopupMenu, TranslateMessage,
    HTCLIENT, HTTRANSPARENT, HWND_BOTTOM, HWND_NOTOPMOST, HWND_TOPMOST, MF_POPUP, MF_SEPARATOR, MF_STRING,
    MSG, SC_MINIMIZE, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNORMAL, SW_SHOWNOACTIVATE,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOWPOS, WM_CHAR, WM_DESTROY, WM_DROPFILES,
    WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCHITTEST, WM_POWERBROADCAST, WM_RBUTTONUP,
    WM_SETCURSOR, WM_SYSCOMMAND, WM_TIMER, WM_WINDOWPOSCHANGING,
};

const WM_MOUSEWHEEL: u32 = 0x020A;
const WM_MOUSEHWHEEL: u32 = 0x020E;
const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_CLIPBOARDUPDATE: u32 = 0x031D;
const WM_HOTKEY: u32 = 0x0312;
const HOTKEY_AI_ASSISTANT_ID: i32 = 1001;

#[link(name = "winmm")]
unsafe extern "system" {
    fn timeBeginPeriod(uPeriod: u32) -> u32;
    fn timeEndPeriod(uPeriod: u32) -> u32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBeep(uType: u32) -> windows::Win32::Foundation::BOOL;
}

#[repr(C)]
struct PowerBroadcastSetting {
    power_setting: windows::core::GUID,
    data_length: u32,
    data: [u8; 1],
}

struct App {
    window: Win32LayeredWindow,
    springs: SpringRect,
    island_state: IslandState,
    layout: LayoutManager,
    text_renderer: TextRenderer,
    pixel_buffer: PixelBuffer,
    backdrop: BackdropBlur,
    last_backdrop_capture: Instant,
    last_foreground_hwnd: HWND,
    media_ctrl: Option<MediaController>,
    _volume_listener: Option<VolumeListener>,
    _power_listener: Option<PowerListener>,
    _brightness_listener: Option<BrightnessListener>,
    _notification_listener: Option<NotificationListener>,
    _keyboard_hook: Option<KeyboardHook>,
    _privacy_listener: Option<system::privacy::PrivacyListener>,
    _network_listener: Option<system::network::NetworkListener>,
    _screenshot_watcher: Option<ScreenshotWatcher>,
    weather_manager: Option<WeatherManager>,
    assistant_manager: AssistantManager,
    pub display_manager: DisplayManager,
    is_animating: bool,
    last_tick: Instant,
    settled_since: Option<Instant>,
    last_trim: Instant,
}

unsafe impl Send for App {}

static APP: Mutex<Option<App>> = Mutex::new(None);
static SHELL_HOOK_MSG: AtomicU32 = AtomicU32::new(0);
static IS_MENU_ACTIVE: AtomicBool = AtomicBool::new(false);

// Lock-free atomic hit-testing bounds for 0-latency WM_NCHITTEST
static HIT_MIN_X: AtomicI32 = AtomicI32::new(0);
static HIT_MAX_X: AtomicI32 = AtomicI32::new(0);
static HIT_MIN_Y: AtomicI32 = AtomicI32::new(0);
static HIT_MAX_Y: AtomicI32 = AtomicI32::new(0);
static SUB_MIN_X: AtomicI32 = AtomicI32::new(0);
static SUB_MAX_X: AtomicI32 = AtomicI32::new(0);
static SUB_MIN_Y: AtomicI32 = AtomicI32::new(0);
static SUB_MAX_Y: AtomicI32 = AtomicI32::new(0);
static IS_DRAGGING_SLIDER: AtomicBool = AtomicBool::new(false);

fn wake_animation(hwnd: HWND, app: &mut App) {
    let now = Instant::now();
    // Self-healing: if the shell ever attempted to minimize or demote the notch, restore and re-pin to topmost (unless context menu is active)
    if !IS_MENU_ACTIVE.load(Ordering::Relaxed) {
        unsafe {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
    app.settled_since = None;
    let anim_interval = system::display::get_optimal_animation_interval_for_window(hwnd);
    unsafe {
        let _ = SetTimer(hwnd, TIMER_ANIMATION_ID, anim_interval, None);
    }
    if !app.is_animating {
        app.is_animating = true;
        app.last_tick = now;
    }
}

fn enable_dark_popup_menus(hwnd: HWND) {
    unsafe {
        use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        use windows::core::{w, PCSTR};

        if let Ok(h_uxtheme) = LoadLibraryW(w!("uxtheme.dll")) {
            // Ordinal 135: SetPreferredAppMode (2 = ForceDark)
            if let Some(set_fn) = GetProcAddress(h_uxtheme, PCSTR(135 as _)) {
                type FnSetPreferredAppMode = unsafe extern "system" fn(i32) -> i32;
                let set_mode: FnSetPreferredAppMode = std::mem::transmute(set_fn);
                set_mode(2);
            }

            // Ordinal 133: AllowDarkModeForWindow(hwnd, true)
            if let Some(allow_fn) = GetProcAddress(h_uxtheme, PCSTR(133 as _)) {
                type FnAllowDarkMode = unsafe extern "system" fn(HWND, windows::Win32::Foundation::BOOL) -> windows::Win32::Foundation::BOOL;
                let allow_mode: FnAllowDarkMode = std::mem::transmute(allow_fn);
                let _ = allow_mode(hwnd, true.into());
            }

            // Ordinal 136: FlushMenuThemes
            if let Some(flush_fn) = GetProcAddress(h_uxtheme, PCSTR(136 as _)) {
                type FnFlush = unsafe extern "system" fn();
                let flush: FnFlush = std::mem::transmute(flush_fn);
                flush();
            }
        }
    }
}

fn extract_todo_intent(lower: &str, original: &str) -> Option<String> {
    let mut cleaned_lower = lower.trim();
    for polite in ["tolong ", "bisa ", "coba ", "minta tolong "] {
        if cleaned_lower.starts_with(polite) {
            cleaned_lower = &cleaned_lower[polite.len()..];
        }
    }

    let prefixes = [
        "tambahin to-do list ",
        "tambah to-do list ",
        "tambahin todo list ",
        "tambah todo list ",
        "tambahin to-do ",
        "tambah to-do ",
        "tambahin todo ",
        "tambah todo ",
        "tambahin tugas ",
        "tambah tugas ",
        "catat to-do ",
        "catat todo ",
        "catat tugas ",
        "catat: ",
        "ingetin saya untuk ",
        "ingetin saya ",
        "ingatkan saya untuk ",
        "ingatkan saya ",
        "ingetin ",
        "ingatkan ",
        "add todo ",
        "add task ",
        "todo: ",
        "todo ",
        "to-do ",
    ];

    for prefix in prefixes {
        if cleaned_lower.starts_with(prefix) {
            let offset = lower.len() - cleaned_lower.len() + prefix.len();
            if offset <= original.len() {
                let mut task = original[offset..].trim().to_string();
                for suffix in [" dong", " ya", " yaa", " pls", " please"] {
                    if task.to_lowercase().ends_with(suffix) {
                        task.truncate(task.len() - suffix.len());
                        task = task.trim().to_string();
                    }
                }
                if !task.is_empty() {
                    let mut chars = task.chars();
                    let capitalized = match chars.next() {
                        None => String::new(),
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    };
                    return Some(capitalized);
                }
            }
        }
    }
    None
}

fn extract_timer_intent(lower: &str) -> Option<u32> {
    if !lower.contains("timer") && !lower.contains("fokus") && !lower.contains("pomodoro") {
        return None;
    }
    let is_command = lower.starts_with("timer")
        || lower.contains("pasang")
        || lower.contains("setel")
        || lower.contains("set ")
        || lower.contains("bikin")
        || lower.contains("buat")
        || lower.contains("mulai")
        || lower.contains("nyalakan");

    if !is_command {
        return None;
    }

    let words: Vec<&str> = lower.split_whitespace().collect();
    for w in words {
        let clean: String = w.chars().filter(|c| c.is_ascii_digit()).collect();
        if let Ok(num) = clean.parse::<u32>() {
            if num >= 1 && num <= 180 {
                return Some(num);
            }
        }
    }
    None
}

fn copy_shelf_item_to_clipboard(item: &system::shelf::ShelfItem, state: &mut IslandState) {
    match &item.kind {
        system::shelf::ShelfItemKind::File(path) => {
            if system::shelf::is_image_file(path) {
                system::shelf::copy_image_file_to_clipboard(path);
                state.show_banner(
                    "Shelf".to_string(),
                    "Gambar Disalin ke Clipboard".to_string(),
                    item.name.clone(),
                );
            } else {
                system::shelf::copy_file_to_clipboard_hdrop(path);
                state.show_banner(
                    "Shelf".to_string(),
                    "File Disalin ke Clipboard".to_string(),
                    item.name.clone(),
                );
            }
        }
        system::shelf::ShelfItemKind::Text(text) => {
            system::shelf::copy_text_to_clipboard(text);
            state.show_banner(
                "Shelf".to_string(),
                "Teks Disalin ke Clipboard".to_string(),
                item.name.clone(),
            );
        }
    }
}

fn handle_assistant_local_command(prompt: &str, hwnd: HWND, app: &mut App) -> bool {
    handle_assistant_local_command_inner(prompt, hwnd, app)
}

fn handle_assistant_local_command_inner(prompt: &str, hwnd: HWND, app: &mut App) -> bool {
    let lower = prompt.trim().to_lowercase();
    if lower == "clear" || lower == "bersihkan" || lower == "hapus chat" {
        app.island_state.assistant.clear();
        return true;
    }

    // Music player browser/app mode configuration (e.g. "set musik edge app", "set musik default")
    if let Some(mode) = system::music::extract_music_player_config_intent(prompt) {
        let saved = system::music::save_music_browser_mode(&mode);
        if saved {
            let reply = match &mode {
                system::music::MusicBrowserMode::Default => {
                    "Pemutar musik dikembalikan ke browser default sistem.".to_string()
                }
                system::music::MusicBrowserMode::Browser { app_mode: true, .. } => format!(
                    "Pemutar musik diatur ke {} (1 jendela, tanpa tab menumpuk).",
                    mode.display_label()
                ),
                system::music::MusicBrowserMode::Browser {
                    app_mode: false, ..
                } => format!("Pemutar musik diatur ke {}.", mode.display_label()),
            };
            app.island_state.assistant.set_action_done(reply, 5);
        } else {
            app.island_state.assistant.set_action_error(
                "[!] Gagal menyimpan konfigurasi pemutar musik.".to_string(),
                6,
            );
        }
        return true;
    }

    // Music playback command (YouTube Music / YouTube auto-play, or Spotify search).
    // Checked BEFORE app_launcher so "lagu" / "music" does not open Explorer's Music folder.
    if let Some(req) = system::music::extract_play_music_intent(prompt) {
        let status_label = match req.platform {
            system::music::MusicPlatform::Spotify => {
                format!("Membuka \"{}\" di Spotify", req.query)
            }
            _ => format!("Mencari \"{}\" di {}", req.query, req.platform.display_name()),
        };
        app.island_state
            .assistant
            .set_thinking_status(status_label);
        system::music::play_music_async(req, Some(hwnd));
        return true;
    }

    // Fast 0ms offline Windows automation with smart typo & alias tolerance
    if let Some(intent) = system::app_launcher::extract_launch_intent(prompt) {
        match intent {
            system::app_launcher::AppLaunchIntent::OpenApp { name, friendly_name } => {
                system::app_launcher::launch_or_activate_app_async(name);
                app.island_state
                    .assistant
                    .set_action_done(format!("Berhasil membuka {}.", friendly_name), 5);
                return true;
            }
            system::app_launcher::AppLaunchIntent::OpenBrowserProfile { browser, profile } => {
                system::app_launcher::launch_browser_profile_async(browser.clone(), profile.clone());
                app.island_state.assistant.set_action_done(
                    format!("Berhasil membuka {} (Profil: \"{}\").", browser, profile),
                    5,
                );
                return true;
            }
            system::app_launcher::AppLaunchIntent::OpenExplorer { path } => {
                let resolved = system::app_launcher::resolve_folder_path(&path);
                if resolved.exists() {
                    let friendly = match path.to_lowercase().as_str() {
                        "downloads" | "download" | "unduhan" => "Downloads".to_string(),
                        "documents" | "dokumen" => "Documents".to_string(),
                        "desktop" => "Desktop".to_string(),
                        "pictures" | "gambar" => "Pictures".to_string(),
                        "videos" | "video" => "Videos".to_string(),
                        "music" | "musik" => "Music".to_string(),
                        "screenshots" | "screenshot" | "tangkapan layar" => "Screenshots".to_string(),
                        _ => resolved.file_name().and_then(|n| n.to_str()).map(|s| s.to_string()).unwrap_or_else(|| path.clone()),
                    };
                    system::app_launcher::open_explorer_path_async(resolved.to_string_lossy().to_string());
                    app.island_state
                        .assistant
                        .set_action_done(format!("Folder {} dibuka di File Explorer.", friendly), 5);
                } else {
                    app.island_state.assistant.set_action_error(
                        format!("[!] Folder \"{}\" tidak ditemukan di PC.", path),
                        6,
                    );
                }
                return true;
            }
            system::app_launcher::AppLaunchIntent::CreateNewFile { folder, filename } => {
                match system::app_launcher::create_and_open_new_text_file(&folder, Some(&filename)) {
                    Ok(p) => {
                        app.island_state.assistant.set_action_done(
                            format!("File baru dibuat dan dibuka di Notepad:\n\"{}\"", p.display()),
                            5,
                        );
                    }
                    Err(e) => {
                        app.island_state
                            .assistant
                            .set_action_error(format!("[!] Gagal membuat file: {}", e), 6);
                    }
                }
                return true;
            }
            system::app_launcher::AppLaunchIntent::LockWorkstation => {
                system::app_launcher::lock_workstation();
                app.island_state
                    .assistant
                    .set_action_done("Workstation Windows dikunci.".to_string(), 5);
                return true;
            }
        }
    }

    // Screenshot / Snip
    if lower == "snip"
        || lower == "screenshot"
        || lower.contains("screenshot")
        || lower.contains("tangkapan layar")
        || lower.starts_with("ambil ss")
        || lower.starts_with("buka snip")
    {
        system::snip::trigger_screen_snip();
        app.island_state
            .assistant
            .set_action_done("Snipping Tool berhasil dibuka!".to_string(), 5);
        return true;
    }

    // Microphone / Mute
    if lower == "mute"
        || lower == "mic"
        || lower == "unmute"
        || lower.contains("matiin mic")
        || lower.contains("matikan mic")
        || lower.contains("matikan mikrofon")
        || lower.contains("buka mic")
        || lower.contains("nyalakan mic")
        || lower.contains("mute mikrofon")
    {
        if let Some(muted) = system::privacy::toggle_default_mic_mute() {
            app.island_state.privacy.mic_muted = muted;
            let status = if muted { "Dimatikan (Muted)" } else { "Dinyalakan (Unmuted)" };
            app.island_state
                .assistant
                .set_action_done(format!("Mikrofon {}", status), 5);
            return true;
        }
    }

    // Natural To-Do Intent
    if let Some(task) = extract_todo_intent(&lower, prompt) {
        if !task.is_empty() {
            app.island_state.todo.add(task.clone());
            app.island_state.assistant.set_action_done(
                format!("Tugas dicatat ke To-Do List:\n\"{}\"", task),
                5,
            );
            return true;
        }
    }

    // Natural Timer Intent
    if let Some(mins) = extract_timer_intent(&lower) {
        let clamped = mins.clamp(1, 180);
        app.island_state.focus_timer.set_duration_mins(clamped);
        if !app.island_state.focus_timer.is_running {
            app.island_state.focus_timer.toggle();
        }
        app.island_state.assistant.set_action_done(
            format!("Focus Timer dinyalakan selama {} menit!", clamped),
            5,
        );
        return true;
    }

    // Weather Location Pin / Auto Reset
    let trimmed_prompt = prompt.trim();
    if lower == "lokasi auto"
        || lower == "reset lokasi"
        || lower == "lokasi otomatis"
        || lower == "auto lokasi"
        || lower == "weather auto"
    {
        if let Some(wm) = &app.weather_manager {
            wm.clear_manual_location();
        }
        app.island_state.show_banner(
            "Weather".to_string(),
            "Auto Location Enabled".to_string(),
            "Detecting weather for current location...".to_string(),
        );
        app.island_state
            .assistant
            .set_action_done("Lokasi cuaca dikembalikan ke deteksi otomatis.".to_string(), 5);
        return true;
    }
    for prefix in [
        "set lokasi ",
        "ganti lokasi ",
        "ubah lokasi ",
        "pin lokasi ",
        "lokasi cuaca ",
        "set city ",
        "weather city ",
    ] {
        if trimmed_prompt
            .get(..prefix.len())
            .map(|s| s.eq_ignore_ascii_case(prefix))
            .unwrap_or(false)
        {
            let target = trimmed_prompt[prefix.len()..].trim();
            if !target.is_empty() {
                if system::weather::is_auto_location_keyword(target) {
                    if let Some(wm) = &app.weather_manager {
                        wm.clear_manual_location();
                    }
                    app.island_state.show_banner(
                        "Weather".to_string(),
                        "Auto Location Enabled".to_string(),
                        "Detecting weather for current location...".to_string(),
                    );
                    app.island_state
                        .assistant
                        .set_action_done("Lokasi cuaca dikembalikan ke deteksi otomatis.".to_string(), 5);
                } else {
                    if let Some(wm) = &app.weather_manager {
                        wm.set_manual_location(target);
                    }
                    app.island_state.show_banner(
                        "Weather".to_string(),
                        "Location Pinned".to_string(),
                        format!("Fetching weather for {}...", target),
                    );
                    app.island_state.assistant.set_action_done(
                        format!("Lokasi cuaca diatur ke {} dan sedang diperbarui.", target),
                        5,
                    );
                }
                return true;
            }
        }
    }

    // Weather
    if lower == "weather"
        || lower == "cuaca"
        || lower.contains("cek cuaca")
        || lower.contains("cuaca hari ini")
        || lower.contains("suhu hari ini")
        || lower.starts_with("info cuaca")
    {
        if let Some(wm) = &app.weather_manager {
            wm.trigger_refresh();
        }
        let temp = app.island_state.weather.temperature.round() as i32;
        let cond = app.island_state.weather.condition_text();
        let city = &app.island_state.weather.city;
        app.island_state.assistant.set_action_done(
            format!("Cuaca saat ini {}°C, {} di {}.", temp, cond, city),
            0,
        );
        return true;
    }

    false
}

fn process_assistant_actions(response: &str, app: &mut App) -> String {
    let mut clean_text = response.to_string();

    // 1. [ACTION:TODO:...]
    if let Some(start) = clean_text.find("[ACTION:TODO:") {
        if let Some(end) = clean_text[start..].find(']') {
            let task = clean_text[start + 13..start + end].trim().to_string();
            if !task.is_empty() {
                app.island_state.todo.add(task.clone());
                app.island_state.show_banner(
                    "To-Do List".to_string(),
                    "Tugas Dicatat ke List".to_string(),
                    task,
                );
            }
            clean_text.replace_range(start..start + end + 1, "");
        }
    }

    // 2. [ACTION:TIMER:...]
    if let Some(start) = clean_text.find("[ACTION:TIMER:") {
        if let Some(end) = clean_text[start..].find(']') {
            let mins_str = clean_text[start + 14..start + end].trim();
            if let Ok(mins) = mins_str.parse::<u32>() {
                let clamped = mins.clamp(1, 180);
                app.island_state.focus_timer.set_duration_mins(clamped);
                if !app.island_state.focus_timer.is_running {
                    app.island_state.focus_timer.toggle();
                }
                app.island_state.show_banner(
                    "Focus Timer".to_string(),
                    "Timer Dinyalakan".to_string(),
                    format!("{} Menit", clamped),
                );
            }
            clean_text.replace_range(start..start + end + 1, "");
        }
    }

    // 3. [ACTION:SNIP]
    if let Some(start) = clean_text.find("[ACTION:SNIP]") {
        system::snip::trigger_screen_snip();
        clean_text.replace_range(start..start + 13, "");
    }

    // 4. [ACTION:MUTE]
    if let Some(start) = clean_text.find("[ACTION:MUTE]") {
        if let Some(muted) = system::privacy::toggle_default_mic_mute() {
            app.island_state.privacy.mic_muted = muted;
        }
        clean_text.replace_range(start..start + 13, "");
    }

    // 4b. [ACTION:WEATHER_CITY:city_or_auto]
    if let Some(start) = clean_text.find("[ACTION:WEATHER_CITY:") {
        if let Some(end) = clean_text[start..].find(']') {
            let target = clean_text[start + 21..start + end].trim().to_string();
            if !target.is_empty() {
                if let Some(wm) = &app.weather_manager {
                    if system::weather::is_auto_location_keyword(&target) {
                        wm.clear_manual_location();
                    } else {
                        wm.set_manual_location(&target);
                    }
                }
            }
            clean_text.replace_range(start..start + end + 1, "");
        }
    }

    // 5. [ACTION:WEATHER]
    if let Some(start) = clean_text.find("[ACTION:WEATHER]") {
        if let Some(wm) = &app.weather_manager {
            wm.trigger_refresh();
        }
        clean_text.replace_range(start..start + 16, "");
    }

    // 6. [ACTION:OPEN:app_name]
    if let Some(start) = clean_text.find("[ACTION:OPEN:") {
        if let Some(end) = clean_text[start..].find(']') {
            let target = clean_text[start + 13..start + end].trim().to_string();
            if !target.is_empty() {
                let p = std::path::PathBuf::from(&target);
                if p.exists() || target.contains(":\\") || target.contains(":/") {
                    system::app_launcher::open_explorer_path_async(target);
                } else {
                    system::app_launcher::launch_or_activate_app_async(target);
                }
            }
            clean_text.replace_range(start..start + end + 1, "");
        }
    }

    // 7. [ACTION:BROWSER:browser:profile]
    if let Some(start) = clean_text.find("[ACTION:BROWSER:") {
        if let Some(end) = clean_text[start..].find(']') {
            let inner = clean_text[start + 16..start + end].trim();
            if let Some((browser, profile)) = inner.split_once(':') {
                system::app_launcher::launch_browser_profile_async(browser.trim().to_string(), profile.trim().to_string());
            } else {
                system::app_launcher::launch_browser_profile_async("brave".to_string(), inner.to_string());
            }
            clean_text.replace_range(start..start + end + 1, "");
        }
    }

    // 8. [ACTION:EXPLORER:folder_or_path]
    if let Some(start) = clean_text.find("[ACTION:EXPLORER:") {
        if let Some(end) = clean_text[start..].find(']') {
            let path = clean_text[start + 17..start + end].trim().to_string();
            if !path.is_empty() {
                system::app_launcher::open_explorer_path_async(path);
            }
            clean_text.replace_range(start..start + end + 1, "");
        }
    }

    // 9. [ACTION:FILE_NEW:folder:filename]
    if let Some(start) = clean_text.find("[ACTION:FILE_NEW:") {
        if let Some(end) = clean_text[start..].find(']') {
            let inner = clean_text[start + 17..start + end].trim();
            let (folder, filename) = if let Some((f, n)) = inner.split_once(':') {
                (f.trim(), Some(n.trim()))
            } else {
                (inner, None)
            };
            let _ = system::app_launcher::create_and_open_new_text_file(folder, filename);
            clean_text.replace_range(start..start + end + 1, "");
        }
    }

    // 10. [ACTION:MUSIC_PLAYER:mode]
    if let Some(start) = clean_text.find("[ACTION:MUSIC_PLAYER:") {
        if let Some(end) = clean_text[start..].find(']') {
            let raw_mode = clean_text[start + 21..start + end].trim();
            if let Some(mode) = system::music::MusicBrowserMode::parse_config(raw_mode) {
                let _ = system::music::save_music_browser_mode(&mode);
            }
            clean_text.replace_range(start..start + end + 1, "");
        }
    }

    // 11. [ACTION:PLAY_MUSIC:query] or [ACTION:PLAY_MUSIC:query:platform]
    if let Some(start) = clean_text.find("[ACTION:PLAY_MUSIC:") {
        if let Some(end) = clean_text[start..].find(']') {
            let payload = clean_text[start + 19..start + end].trim();
            if let Some(req) = system::music::parse_action_play_music(payload) {
                system::music::play_music_async(req, None);
            }
            clean_text.replace_range(start..start + end + 1, "");
        }
    }

    // 12. [ACTION:LOCK] or [ACTION:SYSTEM:LOCK]
    if let Some(start) = clean_text.find("[ACTION:SYSTEM:LOCK]").or_else(|| clean_text.find("[ACTION:LOCK]")) {
        let tag_len = if clean_text[start..].starts_with("[ACTION:SYSTEM:LOCK]") { 20 } else { 13 };
        system::app_launcher::lock_workstation();
        clean_text.replace_range(start..start + tag_len, "");
    }

    clean_text.trim().to_string()
}

unsafe fn read_wide_from_hglobal(hglobal: windows::Win32::Foundation::HGLOBAL) -> Option<String> {
    let byte_len = unsafe { windows::Win32::System::Memory::GlobalSize(hglobal) };
    if byte_len < 2 {
        return None;
    }
    let max_units = (byte_len / 2).min(65536);
    let ptr = unsafe { windows::Win32::System::Memory::GlobalLock(hglobal) } as *const u16;
    if ptr.is_null() {
        return None;
    }
    let mut len = 0usize;
    while len < max_units && unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ptr, len) })
        .trim()
        .to_string();
    let _ = unsafe { windows::Win32::System::Memory::GlobalUnlock(hglobal) };
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

unsafe fn is_clipboard_sensitive(hwnd: HWND) -> bool {
    use std::sync::OnceLock;
    use windows::Win32::System::DataExchange::RegisterClipboardFormatW;

    static CF_EXCLUDE: OnceLock<u32> = OnceLock::new();
    static CF_IGNORE: OnceLock<u32> = OnceLock::new();
    static CF_HISTORY: OnceLock<u32> = OnceLock::new();

    let cf_excl = *CF_EXCLUDE.get_or_init(|| unsafe {
        RegisterClipboardFormatW(windows::core::w!("ExcludeClipboardContentFromMonitorProcessing"))
    });
    if cf_excl != 0 && unsafe { IsClipboardFormatAvailable(cf_excl) }.is_ok() {
        return true;
    }

    let cf_ign = *CF_IGNORE.get_or_init(|| unsafe {
        RegisterClipboardFormatW(windows::core::w!("Clipboard Viewer Ignore"))
    });
    if cf_ign != 0 && unsafe { IsClipboardFormatAvailable(cf_ign) }.is_ok() {
        return true;
    }

    let cf_hist = *CF_HISTORY.get_or_init(|| unsafe {
        RegisterClipboardFormatW(windows::core::w!("CanIncludeInClipboardHistory"))
    });
    if cf_hist != 0 && unsafe { IsClipboardFormatAvailable(cf_hist) }.is_ok() {
        if unsafe { OpenClipboard(hwnd) }.is_ok() {
            let mut blocked = false;
            if let Ok(hdata) = unsafe { GetClipboardData(cf_hist) } {
                let hglobal = windows::Win32::Foundation::HGLOBAL(hdata.0);
                if unsafe { windows::Win32::System::Memory::GlobalSize(hglobal) } >= 4 {
                    let ptr = unsafe { windows::Win32::System::Memory::GlobalLock(hglobal) } as *const u32;
                    if !ptr.is_null() {
                        if unsafe { *ptr } == 0 {
                            blocked = true;
                        }
                        let _ = unsafe { windows::Win32::System::Memory::GlobalUnlock(hglobal) };
                    }
                }
            }
            let _ = unsafe { CloseClipboard() };
            if blocked {
                return true;
            }
        }
    }
    false
}

fn read_clipboard_unicode_text(hwnd: HWND) -> Option<String> {
    unsafe {
        if OpenClipboard(hwnd).is_ok() {
            let mut result = None;
            if let Ok(hdata) = GetClipboardData(13) {
                result = read_wide_from_hglobal(windows::Win32::Foundation::HGLOBAL(hdata.0));
            }
            let _ = CloseClipboard();
            result
        } else {
            None
        }
    }
}

/// Brings the application corresponding to `app_name` to the foreground (e.g. WhatsApp, Antigravity, Spotify).
/// This runs asynchronously on a background worker thread to completely decouple from the UI thread,
/// preventing any re-entrant mutex deadlocks or Win32 message stalls.
pub fn activate_application(app_name: &str) {
    system::app_launcher::launch_or_activate_app(app_name);
}

pub unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let shell_msg = SHELL_HOOK_MSG.load(Ordering::Relaxed);
    if shell_msg != 0 && msg == shell_msg {
        let code = wparam.0 as u32;
        // HSHELL_WINDOWACTIVATED (4) | HSHELL_RUDEAPPACTIVATED (0x8004) | HSHELL_WINDOWCREATED (1) | HSHELL_WINDOWDESTROYED (2)
        if code == 4 || code == 0x8004 || code == 1 || code == 2 {
            // Re-assert topmost Z-order so Dynamic Island is never pushed behind newly active windows (unless context menu is active)
            if !IS_MENU_ACTIVE.load(Ordering::Relaxed) {
                unsafe {
                    if IsIconic(hwnd).as_bool() {
                        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                    }
                    let _ = SetWindowPos(
                        hwnd,
                        HWND_TOPMOST,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                }
            }
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    let cur_fg = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
                    if cur_fg != app.last_foreground_hwnd {
                        app.last_foreground_hwnd = cur_fg;

                        // In FollowCursor mode: if user focused an app on another monitor, relocate island immediately
                        if app.display_manager.target == DisplayTarget::FollowCursor {
                            let mut cursor_pt = POINT::default();
                            let _ = unsafe { GetCursorPos(&mut cursor_pt) };
                            if let Some(rect) = app.display_manager.get_target_rect(Some(cursor_pt)) {
                                let monitor_w = rect.right - rect.left;
                                let target_x = rect.left + (monitor_w - CANVAS_W as i32) / 2;
                                let target_y = rect.top;
                                app.window.move_to(target_x, target_y);
                            }
                        }

                        app.backdrop.capture(app.window.win_x, app.window.win_y);
                        app.last_backdrop_capture = Instant::now();
                        wake_animation(hwnd, app);
                    }
                }
            }
        }
        return LRESULT(0);
    }

    match msg {
        WM_HOTKEY => {
            if (wparam.0 as i32) == HOTKEY_AI_ASSISTANT_ID {
                if let Ok(mut lock) = APP.lock() {
                    if let Some(app) = lock.as_mut() {
                        let is_already_on_assistant = app.island_state.mode == IslandMode::Expanded(ExpandedPage::Assistant);
                        if is_already_on_assistant && app.island_state.is_pinned_open {
                            // Toggle closed if hotkey is pressed while already pinned open
                            app.island_state.is_pinned_open = false;
                            app.island_state.assistant.is_input_active = false;
                            app.island_state.restore_previous_mode();
                        } else {
                            // Expand directly to Groq AI Assistant and auto-focus prompt input
                            app.island_state.select_page(ExpandedPage::Assistant);
                            app.island_state.assistant.is_input_active = true;
                            app.island_state.is_pinned_open = true;
                            app.island_state.unhover_deadline = None;
                            unsafe {
                                let _ = BringWindowToTop(hwnd);
                                let _ = SetForegroundWindow(hwnd);
                                let _ = SetFocus(hwnd);
                            }
                        }
                        wake_animation(hwnd, app);
                    }
                }
            }
            LRESULT(0)
        }
        // 1. Defend against "Show Desktop" (Win+D, 3-finger swipe down) minimization
        WM_SYSCOMMAND => {
            if (wparam.0 & 0xFFF0) == SC_MINIMIZE as usize {
                // Top notch is a permanent hardware HUD, refuse shell minimize commands
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }

        // 2. Defend against Z-order demotion & shell forced hiding
        WM_WINDOWPOSCHANGING => {
            let wp = unsafe { &mut *(lparam.0 as *mut WINDOWPOS) };
            // Clear SWP_HIDEWINDOW so Windows Explorer cannot force the notch to disappear
            wp.flags &= !SWP_HIDEWINDOW;
            // Only defend if the shell is attempting to demote the notch to the bottom of the Z-stack.
            // Never re-route if a popup menu or other topmost window is legitimately positioned above it!
            if wp.hwndInsertAfter == HWND_BOTTOM || wp.hwndInsertAfter == HWND_NOTOPMOST {
                wp.hwndInsertAfter = HWND_TOPMOST;
            }
            LRESULT(0)
        }
        WM_NCHITTEST => {
            // Lock-free sub-microsecond hit-testing: completely bypasses APP.lock()
            if IS_DRAGGING_SLIDER.load(Ordering::Relaxed) {
                return LRESULT(HTCLIENT as isize);
            }

            let screen_x = (lparam.0 & 0xFFFF) as i16 as i32;
            let screen_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;

            let mut pt = POINT {
                x: screen_x,
                y: screen_y,
            };
            unsafe {
                let _ = ScreenToClient(hwnd, &mut pt);
            }

            let min_x = HIT_MIN_X.load(Ordering::Relaxed);
            let max_x = HIT_MAX_X.load(Ordering::Relaxed);
            let min_y = HIT_MIN_Y.load(Ordering::Relaxed);
            let max_y = HIT_MAX_Y.load(Ordering::Relaxed);

            if pt.x >= min_x && pt.x <= max_x && pt.y >= min_y && pt.y <= max_y {
                return LRESULT(HTCLIENT as isize);
            }

            // Check sub-notification pill hit zone if active
            let s_min_x = SUB_MIN_X.load(Ordering::Relaxed);
            let s_max_x = SUB_MAX_X.load(Ordering::Relaxed);
            if s_min_x != s_max_x {
                let s_min_y = SUB_MIN_Y.load(Ordering::Relaxed);
                let s_max_y = SUB_MAX_Y.load(Ordering::Relaxed);
                if pt.x >= s_min_x && pt.x <= s_max_x && pt.y >= s_min_y && pt.y <= s_max_y {
                    return LRESULT(HTCLIENT as isize);
                }
            }

            // Pass clicks through to whatever application is behind the transparent canvas!
            LRESULT(HTTRANSPARENT as isize)
        }

        WM_SETCURSOR => {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetCursor(
                    windows::Win32::UI::WindowsAndMessaging::LoadCursorW(
                        None,
                        windows::Win32::UI::WindowsAndMessaging::IDC_ARROW,
                    )
                    .unwrap_or_default(),
                );
            }
            LRESULT(1)
        }

        WM_LBUTTONDOWN => {
            let cx = (lparam.0 & 0xFFFF) as i16 as f32;
            let cy = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;

            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    let cur_x = app.springs.x.current;
                    let cur_y = app.springs.y.current;
                    let cur_w = app.springs.width.current;

                    // 1. Check if user clicked/dragged on the media seek bar
                    match app.island_state.mode {
                        IslandMode::Expanded(ExpandedPage::Music) => {
                            let progress_x = cur_x + 56.0;
                            let progress_w = cur_w - 112.0;
                            let bar_y = cur_y + 108.0;
                            let bar_rect = Rect {
                                x: progress_x - 8.0,
                                y: bar_y - 10.0,
                                w: progress_w + 16.0,
                                h: 20.0,
                            };
                            if bar_rect.contains(cx, cy) && app.island_state.media_info.duration_secs > 0 {
                                app.island_state.is_dragging_slider = true;
                                let frac = ((cx - progress_x) / progress_w).clamp(0.0, 1.0);
                                let seek_secs = (frac * app.island_state.media_info.duration_secs as f32) as u32;
                                app.island_state.drag_seek_secs = Some(seek_secs);
                                unsafe {
                                    let _ = SetCapture(hwnd);
                                }
                                wake_animation(hwnd, app);
                                return LRESULT(0);
                            }
                        }
                        IslandMode::Lyrics => {
                            let progress_x = cur_x + 50.0;
                            let progress_w = 110.0;
                            let bar_y = cur_y + 78.0;
                            let bar_rect = Rect {
                                x: progress_x - 6.0,
                                y: bar_y - 10.0,
                                w: progress_w + 12.0,
                                h: 20.0,
                            };
                            if bar_rect.contains(cx, cy) && app.island_state.media_info.duration_secs > 0 {
                                app.island_state.is_dragging_slider = true;
                                let frac = ((cx - progress_x) / progress_w).clamp(0.0, 1.0);
                                let seek_secs = (frac * app.island_state.media_info.duration_secs as f32) as u32;
                                app.island_state.drag_seek_secs = Some(seek_secs);
                                unsafe {
                                    let _ = SetCapture(hwnd);
                                }
                                wake_animation(hwnd, app);
                                return LRESULT(0);
                            }
                        }
                        _ => {}
                    }

                    // 2. Check if user clicked a card on Shelf for drag-and-drop
                    if let IslandMode::Expanded(ExpandedPage::Shelf) = app.island_state.mode {
                        let cur_x = app.springs.x.current;
                        let cur_w = app.springs.width.current;
                        let item_count = app.island_state.shelf.items.len();
                        for i in 0..item_count {
                            let card_rect = app.layout.get_shelf_card_rect(i, item_count, cur_x, cur_y, cur_w);
                            let close_rect = Rect {
                                x: card_rect.x + card_rect.w - 14.0,
                                y: card_rect.y + 2.0,
                                w: 12.0,
                                h: 12.0,
                            };
                            if card_rect.contains(cx, cy) && !close_rect.contains(cx, cy) {
                                app.island_state.pending_shelf_drag = Some((i, cx, cy));
                                unsafe {
                                    let _ = SetCapture(hwnd);
                                }
                                break;
                            }
                        }
                    }

                    // 3. If ToDo is in input mode and user clicks outside input pill, commit task
                    if app.island_state.todo.is_input_active {
                        let right_x = cur_x + 288.0;
                        let input_rect = Rect { x: right_x, y: cur_y + 92.0, w: 146.0, h: 26.0 };
                        if !input_rect.contains(cx, cy) {
                            app.island_state.todo.commit_input();
                            wake_animation(hwnd, app);
                        }
                    }
                }
            }
            LRESULT(0)
        }

        WM_MOUSEMOVE => {
            let cx = (lparam.0 & 0xFFFF) as i16 as f32;
            let cy = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;

            // Non-blocking try_lock avoids stalling the OS mouse input thread
            let mut shelf_drag_item: Option<system::shelf::ShelfItem> = None;
            if let Ok(mut lock) = APP.try_lock() {
                if let Some(app) = lock.as_mut() {
                    // Check if pending shelf drag exceeded drag threshold (4px)
                    if let Some((idx, sx, sy)) = app.island_state.pending_shelf_drag {
                        let dist = ((cx - sx).powi(2) + (cy - sy).powi(2)).sqrt();
                        if dist > 4.0 {
                            app.island_state.pending_shelf_drag = None;
                            unsafe {
                                let _ = ReleaseCapture();
                            }
                            if let Some(item) = app.island_state.shelf.items.get(idx) {
                                shelf_drag_item = Some(item.clone());
                            }
                        }
                    }

                    if shelf_drag_item.is_none() {
                        if app.island_state.is_dragging_slider {
                            let cur_x = app.springs.x.current;
                            let cur_w = app.springs.width.current;
                            let (progress_x, progress_w) = match app.island_state.mode {
                                IslandMode::Lyrics => (cur_x + 50.0, 110.0),
                                _ => (cur_x + 56.0, cur_w - 112.0),
                            };
                            let frac = ((cx - progress_x) / progress_w).clamp(0.0, 1.0);
                            let seek_secs = (frac * app.island_state.media_info.duration_secs as f32) as u32;
                            app.island_state.drag_seek_secs = Some(seek_secs);
                            wake_animation(hwnd, app);
                        } else if !app.island_state.is_hovered {
                            app.island_state.on_mouse_enter();
                            wake_animation(hwnd, app);

                            let mut tme = TRACKMOUSEEVENT {
                                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                                dwFlags: TME_LEAVE,
                                hwndTrack: hwnd,
                                dwHoverTime: 0,
                            };
                            unsafe {
                                let _ = TrackMouseEvent(&mut tme);
                            }
                        } else if !app.is_animating {
                            // While hovering, awaken animation if settled for interactive button hover highlighting
                            wake_animation(hwnd, app);
                        }
                    }
                }
            }

            if let Some(item) = shelf_drag_item {
                system::shelf::start_drag_drop(&item);
                if let Ok(mut lock) = APP.try_lock() {
                    if let Some(app) = lock.as_mut() {
                        wake_animation(hwnd, app);
                    }
                }
            }
            LRESULT(0)
        }

        WM_MOUSELEAVE => {
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    if !app.island_state.is_dragging_slider && !app.island_state.todo.is_input_active {
                        app.island_state.assistant.is_input_active = false;
                        app.island_state.on_mouse_leave();
                        wake_animation(hwnd, app);
                    }
                }
            }
            LRESULT(0)
        }

        WM_LBUTTONUP => {
            let cx = (lparam.0 & 0xFFFF) as i16 as f32;
            let cy = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
            let mut app_to_activate: Option<String> = None;
            let mut open_screenshot_markup: Option<Option<std::path::PathBuf>> = None;

            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    // If user was pending shelf drag, but released without dragging -> copy to clipboard!
                    if let Some((idx, _, _)) = app.island_state.pending_shelf_drag.take() {
                        unsafe {
                            let _ = ReleaseCapture();
                        }
                        if let Some(item) = app.island_state.shelf.items.get(idx).cloned() {
                            copy_shelf_item_to_clipboard(&item, &mut app.island_state);
                        }
                        wake_animation(hwnd, app);
                        return LRESULT(0);
                    }

                    // Check if we were dragging the media slider
                    if app.island_state.is_dragging_slider {
                        unsafe {
                            let _ = ReleaseCapture();
                        }
                        app.island_state.is_dragging_slider = false;
                        if let Some(seek_secs) = app.island_state.drag_seek_secs.take() {
                            if let Some(mc) = &app.media_ctrl {
                                mc.seek_to(seek_secs);
                            }
                            app.island_state.media_info.position_secs = seek_secs;
                        }
                        wake_animation(hwnd, app);
                        return LRESULT(0);
                    }

                    // Check if eyedropper color picker was active
                    if app.island_state.is_color_picker_active {
                        app.island_state.is_color_picker_active = false;
                        unsafe {
                            let _ = ReleaseCapture();
                        }
                        if let Some(c) = system::color_picker::ScreenColor::pick_at_cursor() {
                            c.copy_to_clipboard();
                            app.island_state.show_notification(
                                ui::NotificationKind::ColorPicked { hex: c.hex(), rgb: c.rgb_string() },
                                3500,
                            );
                        }
                        wake_animation(hwnd, app);
                        return LRESULT(0);
                    }

                    let action = app.layout.handle_click(cx, cy);

                    // If assistant input is active and click is not focusing or submitting it, unfocus input
                    if app.island_state.assistant.is_input_active
                        && !matches!(action, Some(ClickAction::AssistantFocusInput | ClickAction::AssistantSubmitPrompt))
                    {
                        app.island_state.assistant.is_input_active = false;
                        wake_animation(hwnd, app);
                    }

                    match action {
                        Some(ClickAction::PrevPage) => {
                            app.island_state.cycle_page(false);
                            if app.island_state.active_page == ExpandedPage::Weather && app.island_state.weather.is_stale() {
                                if let Some(wm) = &app.weather_manager {
                                    wm.trigger_refresh();
                                }
                            }
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::NextPage) => {
                            app.island_state.cycle_page(true);
                            if app.island_state.active_page == ExpandedPage::Weather && app.island_state.weather.is_stale() {
                                if let Some(wm) = &app.weather_manager {
                                    wm.trigger_refresh();
                                }
                            }
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::SelectPage(page)) => {
                            if page == ExpandedPage::Weather && app.island_state.weather.is_stale() {
                                if let Some(wm) = &app.weather_manager {
                                    wm.trigger_refresh();
                                }
                            }
                            app.island_state.select_page(page);
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::ToggleLyrics) => {
                            app.island_state.toggle_lyrics();
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::ToggleRomaji) => {
                            app.island_state.show_romaji = !app.island_state.show_romaji;
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::ToggleGlassTheme) => {
                            app.island_state.toggle_glass_theme();
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::SeekRelative(delta)) => {
                            let cur_pos = app.island_state.media_info.position_secs;
                            let dur = app.island_state.media_info.duration_secs;
                            let new_pos = if delta < 0 {
                                cur_pos.saturating_sub((-delta) as u32)
                            } else if dur > 0 {
                                (cur_pos + delta as u32).min(dur)
                            } else {
                                cur_pos + delta as u32
                            };
                            if let Some(mc) = &app.media_ctrl {
                                mc.seek_to(new_pos);
                            }
                            app.island_state.media_info.position_secs = new_pos;
                            app.island_state.media_progress_accum = 0.0;
                            app.island_state.reset_lyrics_manual_scroll();
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::TogglePlayPause) => {
                            if let Some(mc) = &app.media_ctrl {
                                mc.toggle_play_pause();
                            }
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::SkipNext) => {
                            if let Some(mc) = &app.media_ctrl {
                                mc.skip_next();
                            }
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::SkipPrev) => {
                            if let Some(mc) = &app.media_ctrl {
                                mc.skip_previous();
                            }
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::SeekMedia(secs)) => {
                            if let Some(mc) = &app.media_ctrl {
                                mc.seek_to(secs);
                            }
                            app.island_state.media_info.position_secs = secs;
                            app.island_state.media_progress_accum = 0.0;
                            app.island_state.reset_lyrics_manual_scroll();
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::OpenShelfItem(idx)) => {
                            if let Some(item) = app.island_state.shelf.items.get(idx).cloned() {
                                copy_shelf_item_to_clipboard(&item, &mut app.island_state);
                                wake_animation(hwnd, app);
                            }
                        }
                        Some(ClickAction::RemoveShelfItem(idx)) => {
                            app.island_state.shelf.remove_at(idx);
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::ToggleTodo(id)) => {
                            app.island_state.todo.toggle(id);
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::PromptAddTask) => {
                            app.island_state.todo.start_input();
                            unsafe {
                                let _ = SetFocus(hwnd);
                            }
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::CommitAddTask) => {
                            app.island_state.todo.commit_input();
                            app.island_state.todo.is_input_active = false;
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::CancelAddTask) => {
                            app.island_state.todo.cancel_input();
                            app.island_state.todo.is_input_active = false;
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::ClearDoneTodo) => {
                            app.island_state.todo.clear_done();
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::ToggleTimer) => {
                            app.island_state.focus_timer.toggle();
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::ResetTimer) => {
                            app.island_state.focus_timer.reset();
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::SetTimerMode(mode)) => {
                            app.island_state.focus_timer.set_mode(mode);
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::AdjustTimerMins(delta)) => {
                            let cur_mins = app.island_state.focus_timer.total_seconds / 60;
                            let new_mins = if delta < 0 {
                                cur_mins.saturating_sub((-delta) as u32).max(1)
                            } else {
                                (cur_mins + delta as u32).min(180)
                            };
                            app.island_state.focus_timer.set_duration_mins(new_mins);
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::CalendarPrevMonth) => {
                            app.island_state.calendar.prev_month();
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::CalendarNextMonth) => {
                            app.island_state.calendar.next_month();
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::CalendarJumpToToday) => {
                            app.island_state.calendar.jump_to_today();
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::CalendarSelectDay(d)) => {
                            app.island_state.calendar.select_day(d);
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::CalendarPromptAddEvent) => {
                            app.island_state.calendar.start_input();
                            unsafe {
                                let _ = SetFocus(hwnd);
                            }
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::CalendarCommitAddEvent) => {
                            app.island_state.calendar.commit_input();
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::CalendarCancelAddEvent) => {
                            app.island_state.calendar.cancel_input();
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::CalendarDeleteEvent(id)) => {
                            app.island_state.calendar.delete_event(id);
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::CalendarSyncGoogle) => {
                            app.island_state.calendar.trigger_sync(hwnd);
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::ToggleMicMute) => {
                            if let Some(muted) = system::privacy::toggle_default_mic_mute() {
                                app.island_state.privacy.mic_muted = muted;
                                wake_animation(hwnd, app);
                            }
                        }
                        Some(ClickAction::TriggerSnip) => {
                            system::snip::trigger_screen_snip();
                        }
                        Some(ClickAction::StartColorPicker) => {
                            app.island_state.is_color_picker_active = true;
                            unsafe {
                                let _ = SetCapture(hwnd);
                            }
                            app.island_state.show_banner(
                                "Eyedropper".to_string(),
                                "Color Picker Active".to_string(),
                                "Click anywhere on screen to copy hex color".to_string(),
                            );
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::RefreshWeather) => {
                            if let Some(wm) = &app.weather_manager {
                                wm.trigger_refresh();
                                app.island_state.show_banner(
                                    "Weather".to_string(),
                                    "Refreshing Weather".to_string(),
                                    "Fetching live forecast for current location...".to_string(),
                                );
                                wake_animation(hwnd, app);
                            }
                        }
                        Some(ClickAction::AssistantFocusInput) => {
                            app.island_state.assistant.start_input();
                            let is_shift = unsafe { (windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x10) as i16) < 0 };
                            let cur_x = app.springs.x.current;
                            let text_start_x = LayoutManager::assistant_input_text_start_x(cur_x);
                            app.island_state.assistant.input.set_cursor_from_x(
                                cx,
                                text_start_x,
                                &app.text_renderer,
                                LayoutManager::ASSISTANT_INPUT_FONT_SIZE,
                                false,
                                is_shift,
                            );
                            let cur_w = app.springs.width.current;
                            let visible_w = LayoutManager::assistant_input_visible_w(cur_w);
                            app.island_state.assistant.input.update_scroll(
                                &app.text_renderer,
                                LayoutManager::ASSISTANT_INPUT_FONT_SIZE,
                                false,
                                visible_w,
                                16.0,
                            );
                            unsafe {
                                let _ = SetFocus(hwnd);
                            }
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::TodoFocusInput) => {
                            let is_shift = unsafe { (windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x10) as i16) < 0 };
                            let cur_x = app.springs.x.current;
                            let text_start_x = LayoutManager::todo_input_text_start_x(cur_x);
                            app.island_state.todo.input.set_cursor_from_x(
                                cx,
                                text_start_x,
                                &app.text_renderer,
                                10.0,
                                false,
                                is_shift,
                            );
                            let visible_w = LayoutManager::todo_input_visible_w();
                            app.island_state.todo.input.update_scroll(&app.text_renderer, 10.0, false, visible_w, 12.0);
                            unsafe {
                                let _ = SetFocus(hwnd);
                            }
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::CalendarFocusInput) => {
                            let is_shift = unsafe { (windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x10) as i16) < 0 };
                            let cur_x = app.springs.x.current;
                            let text_start_x = LayoutManager::calendar_input_text_start_x(cur_x);
                            app.island_state.calendar.input.set_cursor_from_x(
                                cx,
                                text_start_x,
                                &app.text_renderer,
                                10.0,
                                false,
                                is_shift,
                            );
                            let cur_w = app.springs.width.current;
                            let visible_w = LayoutManager::calendar_input_visible_w(cur_w);
                            app.island_state.calendar.input.update_scroll(&app.text_renderer, 10.0, false, visible_w, 12.0);
                            unsafe {
                                let _ = SetFocus(hwnd);
                            }
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::AssistantSubmitPrompt) => {
                            if let Some(prompt) = app.island_state.assistant.commit_prompt() {
                                if !handle_assistant_local_command(&prompt, hwnd, app) {
                                    let history: Vec<system::assistant::ChatMessage> = if app.island_state.assistant.messages.len() > 1 {
                                        let all = &app.island_state.assistant.messages;
                                        let count = all.len() - 1;
                                        let start = count.saturating_sub(6);
                                        all[start..count].to_vec()
                                    } else {
                                        Vec::new()
                                    };
                                    app.assistant_manager.submit_prompt(prompt, history, hwnd);
                                }
                            }
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::AssistantClear) => {
                            app.island_state.assistant.clear();
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::AssistantCopyResponse) => {
                            let text = app.island_state.assistant.last_response.clone();
                            if !text.is_empty() {
                                system::shelf::copy_text_to_clipboard(&text);
                                app.island_state.show_banner(
                                    "Gemini Assistant".to_string(),
                                    "Response Copied".to_string(),
                                    "AI response copied to clipboard".to_string(),
                                    );
                                wake_animation(hwnd, app);
                            }
                        }
                        Some(ClickAction::ActivateApp { app_name }) => {
                            app.island_state.dismiss_sub_notification();
                            if matches!(app.island_state.mode, IslandMode::Notification(_)) {
                                app.island_state.restore_previous_mode();
                            }
                            wake_animation(hwnd, app);
                            app_to_activate = Some(app_name);
                        }
                        Some(ClickAction::OpenLatestScreenshotMarkup) => {
                            app.island_state.dismiss_sub_notification();
                            if matches!(app.island_state.mode, IslandMode::Notification(_)) {
                                app.island_state.restore_previous_mode();
                            }
                            let shelf_hint = app.island_state.shelf.latest_image_path();
                            wake_animation(hwnd, app);
                            open_screenshot_markup = Some(shelf_hint);
                        }
                        None => {
                            // If user clicks anywhere on Resting or Closed notch, expand to active page!
                            match app.island_state.mode {
                                IslandMode::Resting | IslandMode::Closed => {
                                    app.island_state.on_mouse_enter();
                                    wake_animation(hwnd, app);
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

            if let Some(app_name) = app_to_activate {
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(
                        windows::Win32::UI::WindowsAndMessaging::ASFW_ANY,
                    );
                }
                std::thread::spawn(move || {
                    activate_application(&app_name);
                });
            }

            if let Some(shelf_hint) = open_screenshot_markup {
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(
                        windows::Win32::UI::WindowsAndMessaging::ASFW_ANY,
                    );
                }
                system::snip::open_latest_screenshot_markup_async(shelf_hint);
            }

            LRESULT(0)
        }

        WM_RBUTTONUP => {
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    if app.island_state.is_color_picker_active {
                        app.island_state.is_color_picker_active = false;
                        unsafe {
                            let _ = ReleaseCapture();
                        }
                        wake_animation(hwnd, app);
                        return LRESULT(0);
                    }
                }
            }

            // Right-click context menu (Modern Fluent Dark Mode)
            let mut pt = POINT::default();
            unsafe {
                if GetCursorPos(&mut pt).is_ok() {
                    if let Ok(hmenu) = CreatePopupMenu() {
                        let is_expanded = if let Ok(lock) = APP.lock() {
                            lock.as_ref()
                                .map(|a| matches!(a.island_state.mode, IslandMode::Expanded(_) | IslandMode::Lyrics))
                                .unwrap_or(false)
                        } else {
                            false
                        };

                        let expand_label = if is_expanded {
                            windows::core::w!("🏝️  Collapse to Notch")
                        } else {
                            windows::core::w!("🏝️  Expand Dynamic Island")
                        };
                        let _ = AppendMenuW(hmenu, MF_STRING, 1000, expand_label);

                        // Submenu: Switch Page
                        if let Ok(h_pages) = CreatePopupMenu() {
                            let _ = AppendMenuW(h_pages, MF_STRING, 1001, windows::core::w!("🎵  Music Player"));
                            let _ = AppendMenuW(h_pages, MF_STRING, 1029, windows::core::w!("🌤️  Live Weather"));
                            let _ = AppendMenuW(h_pages, MF_STRING, 1002, windows::core::w!("📁  File Shelf"));
                            let _ = AppendMenuW(h_pages, MF_STRING, 1028, windows::core::w!("📅  Calendar & Agenda"));
                            let _ = AppendMenuW(h_pages, MF_STRING, 1003, windows::core::w!("📋  To-Do List"));
                            let _ = AppendMenuW(h_pages, MF_STRING, 1022, windows::core::w!("🛠️  Quick Tools"));
                            let _ = AppendMenuW(h_pages, MF_STRING, 1015, windows::core::w!("⏱️  Focus Timer"));
                            let _ = AppendMenuW(h_pages, MF_STRING, 1034, windows::core::w!("✦  Gemini AI Assistant"));
                            let _ = AppendMenuW(h_pages, MF_SEPARATOR, 0, None);
                            let _ = AppendMenuW(h_pages, MF_STRING, 1004, windows::core::w!("💬  Toggle Synced Lyrics"));
                            let _ = AppendMenuW(hmenu, MF_POPUP, h_pages.0 as usize, windows::core::w!("📄  Switch Page"));
                        }

                        // Submenu: Quick Utilities
                        if let Ok(h_tools) = CreatePopupMenu() {
                            let _ = AppendMenuW(h_tools, MF_STRING, 1017, windows::core::w!("✂️  Screen Snip (Snipping Tool)"));
                            let _ = AppendMenuW(h_tools, MF_STRING, 1016, windows::core::w!("🎯  Pick Screen Color (Eyedropper)"));
                            let _ = AppendMenuW(h_tools, MF_STRING, 1018, windows::core::w!("🎙️  Toggle Microphone Mute"));
                            let _ = AppendMenuW(hmenu, MF_POPUP, h_tools.0 as usize, windows::core::w!("⚡  Quick Tools"));
                        }

                        // Submenu: Universal Multi-Monitor Target
                        if let Ok(h_display) = CreatePopupMenu() {
                            let (target, monitors) = if let Ok(lock) = APP.lock() {
                                if let Some(app) = lock.as_ref() {
                                    (app.display_manager.target, app.display_manager.monitors.clone())
                                } else {
                                    (DisplayTarget::FollowCursor, Vec::new())
                                }
                            } else {
                                (DisplayTarget::FollowCursor, Vec::new())
                            };

                            let is_follow = target == DisplayTarget::FollowCursor;
                            let follow_str = if is_follow { "●  Follow Active Cursor" } else { "    Follow Active Cursor" };
                            let follow_w: Vec<u16> = follow_str.encode_utf16().chain(std::iter::once(0)).collect();
                            let _ = AppendMenuW(h_display, MF_STRING, 2000, windows::core::PCWSTR(follow_w.as_ptr()));
                            let _ = AppendMenuW(h_display, MF_SEPARATOR, 0, None);

                            for mon in &monitors {
                                let is_selected = target == DisplayTarget::Specific(mon.index);
                                let check = if is_selected { "●  " } else { "    " };
                                let item_str = format!("{}{}", check, mon.name);
                                let item_w: Vec<u16> = item_str.encode_utf16().chain(std::iter::once(0)).collect();
                                let _ = AppendMenuW(h_display, MF_STRING, 2000 + mon.index, windows::core::PCWSTR(item_w.as_ptr()));
                            }
                            let _ = AppendMenuW(hmenu, MF_POPUP, h_display.0 as usize, windows::core::w!("🖥️  Display Target"));
                        }

                        // Submenu: Island Shape (Top Notch vs Floating Pill)
                        if let Ok(h_shape) = CreatePopupMenu() {
                            let cur_shape = if let Ok(lock) = APP.lock() {
                                lock.as_ref().map(|a| a.island_state.shape).unwrap_or(ui::IslandShape::FloatingPill)
                            } else {
                                ui::IslandShape::FloatingPill
                            };
                            let is_notch = cur_shape == ui::IslandShape::TopNotch;
                            let notch_str = if is_notch { "●  Top Notch (Bezel Attached)" } else { "    Top Notch (Bezel Attached)" };
                            let notch_w: Vec<u16> = notch_str.encode_utf16().chain(std::iter::once(0)).collect();
                            let _ = AppendMenuW(h_shape, MF_STRING, 1040, windows::core::PCWSTR(notch_w.as_ptr()));

                            let is_pill = cur_shape == ui::IslandShape::FloatingPill;
                            let pill_str = if is_pill { "●  Floating Pill (Capsule)" } else { "    Floating Pill (Capsule)" };
                            let pill_w: Vec<u16> = pill_str.encode_utf16().chain(std::iter::once(0)).collect();
                            let _ = AppendMenuW(h_shape, MF_STRING, 1041, windows::core::PCWSTR(pill_w.as_ptr()));

                            let _ = AppendMenuW(hmenu, MF_POPUP, h_shape.0 as usize, windows::core::w!("🎛️  Island Shape"));
                        }

                        // Theme Switcher & Weather Location Status
                        let (is_dark, is_manual_loc) = if let Ok(lock) = APP.lock() {
                            lock.as_ref()
                                .map(|a| {
                                    (
                                        a.island_state.glass_theme == ui::GlassTheme::DarkObsidian,
                                        a.weather_manager
                                            .as_ref()
                                            .map(|wm| wm.is_manual_location())
                                            .unwrap_or(false),
                                    )
                                })
                                .unwrap_or((false, false))
                        } else {
                            (false, false)
                        };
                        let theme_label = if is_dark {
                            windows::core::w!("☀️  Switch to Light Glass")
                        } else {
                            windows::core::w!("🌙  Switch to Dark Obsidian")
                        };
                        let _ = AppendMenuW(hmenu, MF_STRING, 1030, theme_label);
                        let _ = AppendMenuW(hmenu, MF_STRING, 1032, windows::core::w!("📸  Save Screenshot (PNG)"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1033, windows::core::w!("🌤️  Refresh Weather Data"));
                        if is_manual_loc {
                            let _ = AppendMenuW(hmenu, MF_STRING, 1035, windows::core::w!("📍  Use Auto Weather Location"));
                        }

                        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
                        let _ = AppendMenuW(hmenu, MF_STRING, 1031, windows::core::w!("🌐  GitHub Repository"));
                        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
                        let _ = AppendMenuW(hmenu, MF_STRING, 1099, windows::core::w!("❌  Exit NotDynamicIsland"));

                        let _ = SetForegroundWindow(hwnd);
                        IS_MENU_ACTIVE.store(true, Ordering::Relaxed);
                        let cmd = TrackPopupMenu(
                            hmenu,
                            TPM_RETURNCMD | TPM_RIGHTBUTTON,
                            pt.x,
                            pt.y,
                            0,
                            hwnd,
                            None,
                        );
                        IS_MENU_ACTIVE.store(false, Ordering::Relaxed);
                        let _ = DestroyMenu(hmenu);

                        if cmd.0 == 1099 {
                            let _ = DestroyWindow(hwnd);
                        } else if cmd.0 == 1031 {
                            // Non-blocking browser launch detached from the UI thread and outside of APP lock
                            std::thread::spawn(|| {
                                let _ = windows::Win32::UI::Shell::ShellExecuteW(
                                    windows::Win32::Foundation::HWND::default(),
                                    windows::core::w!("open"),
                                    windows::core::w!("https://github.com/Sarashinaa/NotDynamicIsland"),
                                    None,
                                    None,
                                    SW_SHOWNORMAL,
                                );
                            });
                        } else if cmd.0 != 0 {
                            if let Ok(mut lock) = APP.lock() {
                                if let Some(app) = lock.as_mut() {
                                    match cmd.0 {
                                        1000 => {
                                            if matches!(app.island_state.mode, IslandMode::Expanded(_) | IslandMode::Lyrics) {
                                                app.island_state.mode = IslandMode::Closed;
                                            } else {
                                                app.island_state.mode = IslandMode::Expanded(app.island_state.active_page);
                                            }
                                        }
                                        1001 => app.island_state.select_page(ExpandedPage::Music),
                                        1029 => {
                                            if app.island_state.weather.is_stale() {
                                                if let Some(wm) = &app.weather_manager {
                                                    wm.trigger_refresh();
                                                }
                                            }
                                            app.island_state.select_page(ExpandedPage::Weather);
                                        }
                                        1002 => app.island_state.select_page(ExpandedPage::Shelf),
                                        1028 => app.island_state.select_page(ExpandedPage::Calendar),
                                        1003 => app.island_state.select_page(ExpandedPage::ToDo),
                                        1022 => app.island_state.select_page(ExpandedPage::Tools),
                                        1015 => app.island_state.select_page(ExpandedPage::Timer),
                                        1034 => app.island_state.select_page(ExpandedPage::Assistant),
                                        1004 => app.island_state.toggle_lyrics(),
                                        1016 => {
                                            app.island_state.is_color_picker_active = true;
                                            let _ = SetCapture(hwnd);
                                            app.island_state.show_banner(
                                                "Eyedropper".to_string(),
                                                "Color Picker Active".to_string(),
                                                "Click anywhere on screen to copy hex color".to_string(),
                                            );
                                        }
                                        1017 => {
                                            system::snip::trigger_screen_snip();
                                        }
                                        1018 => {
                                            if let Some(muted) = system::privacy::toggle_default_mic_mute() {
                                                app.island_state.privacy.mic_muted = muted;
                                            }
                                        }
                                        1030 => {
                                            app.island_state.toggle_glass_theme();
                                        }
                                        1032 => {
                                            let pixels = app.pixel_buffer.data.clone();
                                            let w = app.pixel_buffer.width;
                                            let h = app.pixel_buffer.height;
                                            let hwnd_val = hwnd.0 as usize;
                                            std::thread::spawn(move || {
                                                if let Some(path) = system::screenshot::save_island_screenshot(&pixels, w, h) {
                                                    system::shelf::enqueue_shelf_image_file(
                                                        HWND(hwnd_val as *mut _),
                                                        path,
                                                        false,
                                                    );
                                                }
                                            });
                                        }
                                        1033 => {
                                            if let Some(wm) = &app.weather_manager {
                                                wm.trigger_refresh();
                                                app.island_state.show_banner(
                                                    "Weather".to_string(),
                                                    "Refreshing Weather".to_string(),
                                                    "Fetching live forecast for current location...".to_string(),
                                                );
                                            }
                                        }
                                        1035 => {
                                            if let Some(wm) = &app.weather_manager {
                                                wm.clear_manual_location();
                                                app.island_state.show_banner(
                                                    "Weather".to_string(),
                                                    "Auto Location Enabled".to_string(),
                                                    "Detecting weather for current location...".to_string(),
                                                );
                                            }
                                        }
                                        1040 => {
                                            app.island_state.set_shape(ui::IslandShape::TopNotch);
                                            app.settled_since = None;
                                            wake_animation(hwnd, app);
                                        }
                                        1041 => {
                                            app.island_state.set_shape(ui::IslandShape::FloatingPill);
                                            app.settled_since = None;
                                            wake_animation(hwnd, app);
                                        }
                                        2000 => {
                                            app.display_manager.set_target(DisplayTarget::FollowCursor);
                                            update_display_hook(hwnd, true);
                                            let mut cursor_pt = POINT::default();
                                            let _ = GetCursorPos(&mut cursor_pt);
                                            if let Some(rect) = app.display_manager.get_target_rect(Some(cursor_pt)) {
                                                let monitor_w = rect.right - rect.left;
                                                let target_x = rect.left + (monitor_w - CANVAS_W as i32) / 2;
                                                let target_y = rect.top;
                                                app.window.move_to(target_x, target_y);
                                                app.backdrop.capture(target_x, target_y);
                                                app.last_backdrop_capture = Instant::now();
                                            }
                                        }
                                        val if val > 2000 && val <= 2099 => {
                                            let target_idx = (val - 2000) as usize;
                                            app.display_manager.set_target(DisplayTarget::Specific(target_idx));
                                            update_display_hook(hwnd, false);
                                            if let Some(rect) = app.display_manager.get_target_rect(None) {
                                                let monitor_w = rect.right - rect.left;
                                                let target_x = rect.left + (monitor_w - CANVAS_W as i32) / 2;
                                                let target_y = rect.top;
                                                app.window.move_to(target_x, target_y);
                                                app.backdrop.capture(target_x, target_y);
                                                app.last_backdrop_capture = Instant::now();
                                            }
                                        }
                                        _ => {}
                                    }
                                    wake_animation(hwnd, app);
                                }
                            }
                        }
                    }
                }
            }
            LRESULT(0)
        }

        WM_DROPFILES => {
            let hdrop = HDROP(wparam.0 as *mut _);
            unsafe {
                let count = DragQueryFileW(hdrop, 0xFFFFFFFF, None);
                let mut added_any = false;
                for i in 0..count {
                    let mut buf = vec![0u16; 1024];
                    let len = DragQueryFileW(hdrop, i, Some(&mut buf));
                    if len > 0 {
                        let path_str = String::from_utf16_lossy(&buf[..len as usize]);
                        let path = PathBuf::from(path_str);

                        if let Some(file) = system::shelf::ShelfFile::from_path(path) {
                            if let Ok(mut lock) = APP.lock() {
                                if let Some(app) = lock.as_mut() {
                                    app.island_state.shelf.add(file);
                                    app.island_state.select_page(ExpandedPage::Shelf);
                                    added_any = true;
                                }
                            }
                        }
                    }
                }
                DragFinish(hdrop);
                if added_any {
                    if let Ok(mut lock) = APP.lock() {
                        if let Some(app) = lock.as_mut() {
                            wake_animation(hwnd, app);
                        }
                    }
                }
            }
            LRESULT(0)
        }

        WM_CHAR => {
            let code = (wparam.0 & 0xFFFF) as u16;
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    if app.island_state.todo.is_input_active {
                        if code == 13 {
                            // Enter: commit task
                            app.island_state.todo.commit_input();
                            wake_animation(hwnd, app);
                        } else if code == 27 {
                            // Esc: cancel input
                            app.island_state.todo.cancel_input();
                            wake_animation(hwnd, app);
                        } else if code == 1 {
                            // Ctrl+A: Select All
                            app.island_state.todo.input.select_all();
                            wake_animation(hwnd, app);
                        } else if code == 3 {
                            // Ctrl+C: Copy selected
                            if let Some(clip) = app.island_state.todo.input.selected_text() {
                                system::shelf::copy_text_to_clipboard(&clip);
                            }
                            wake_animation(hwnd, app);
                        } else if code == 24 {
                            // Ctrl+X: Cut selected
                            if let Some(clip) = app.island_state.todo.input.selected_text() {
                                system::shelf::copy_text_to_clipboard(&clip);
                                app.island_state.todo.input.delete_selection();
                                let visible_w = LayoutManager::todo_input_visible_w();
                                app.island_state.todo.input.update_scroll(&app.text_renderer, 10.0, false, visible_w, 12.0);
                                wake_animation(hwnd, app);
                            }
                        } else if code == 22 {
                            // Ctrl+V: Paste from clipboard
                            if let Some(clip) = read_clipboard_unicode_text(hwnd) {
                                app.island_state.todo.input.insert_text(&clip);
                                let visible_w = LayoutManager::todo_input_visible_w();
                                app.island_state.todo.input.update_scroll(&app.text_renderer, 10.0, false, visible_w, 12.0);
                                wake_animation(hwnd, app);
                            }
                        } else if code == 8 {
                            // Backspace
                            app.island_state.todo.input.backspace();
                            let visible_w = LayoutManager::todo_input_visible_w();
                            app.island_state.todo.input.update_scroll(&app.text_renderer, 10.0, false, visible_w, 12.0);
                            wake_animation(hwnd, app);
                        } else if let Some(ch) = char::from_u32(code as u32) {
                            if !ch.is_control() {
                                app.island_state.todo.input.insert_char(ch);
                                let visible_w = LayoutManager::todo_input_visible_w();
                                app.island_state.todo.input.update_scroll(&app.text_renderer, 10.0, false, visible_w, 12.0);
                                wake_animation(hwnd, app);
                            }
                        }
                    } else if app.island_state.calendar.is_input_active {
                        let cur_w = app.springs.width.current;
                        let visible_w = LayoutManager::calendar_input_visible_w(cur_w);
                        if code == 13 {
                            // Enter: commit calendar event
                            app.island_state.calendar.commit_input();
                            wake_animation(hwnd, app);
                        } else if code == 27 {
                            // Esc: cancel calendar input
                            app.island_state.calendar.cancel_input();
                            wake_animation(hwnd, app);
                        } else if code == 1 {
                            // Ctrl+A: Select All
                            app.island_state.calendar.input.select_all();
                            wake_animation(hwnd, app);
                        } else if code == 3 {
                            // Ctrl+C: Copy selected
                            if let Some(clip) = app.island_state.calendar.input.selected_text() {
                                system::shelf::copy_text_to_clipboard(&clip);
                            }
                            wake_animation(hwnd, app);
                        } else if code == 24 {
                            // Ctrl+X: Cut selected
                            if let Some(clip) = app.island_state.calendar.input.selected_text() {
                                system::shelf::copy_text_to_clipboard(&clip);
                                app.island_state.calendar.input.delete_selection();
                                app.island_state.calendar.input.update_scroll(&app.text_renderer, 10.0, false, visible_w, 12.0);
                                wake_animation(hwnd, app);
                            }
                        } else if code == 22 {
                            // Ctrl+V: Paste from clipboard
                            if let Some(clip) = read_clipboard_unicode_text(hwnd) {
                                app.island_state.calendar.input.insert_text(&clip);
                                app.island_state.calendar.input.update_scroll(&app.text_renderer, 10.0, false, visible_w, 12.0);
                                wake_animation(hwnd, app);
                            }
                        } else if code == 8 {
                            // Backspace
                            app.island_state.calendar.input.backspace();
                            app.island_state.calendar.input.update_scroll(&app.text_renderer, 10.0, false, visible_w, 12.0);
                            wake_animation(hwnd, app);
                        } else if let Some(ch) = char::from_u32(code as u32) {
                            if !ch.is_control() {
                                app.island_state.calendar.input.insert_char(ch);
                                app.island_state.calendar.input.update_scroll(&app.text_renderer, 10.0, false, visible_w, 12.0);
                                wake_animation(hwnd, app);
                            }
                        }
                    } else if app.island_state.assistant.is_input_active {
                        let cur_w = app.springs.width.current;
                        let visible_w = LayoutManager::assistant_input_visible_w(cur_w);
                        if code == 13 {
                            // Enter: commit prompt
                            if let Some(prompt) = app.island_state.assistant.commit_prompt() {
                                if !handle_assistant_local_command(&prompt, hwnd, app) {
                                    let history: Vec<system::assistant::ChatMessage> = if app.island_state.assistant.messages.len() > 1 {
                                        let all = &app.island_state.assistant.messages;
                                        let count = all.len() - 1;
                                        let start = count.saturating_sub(6);
                                        all[start..count].to_vec()
                                    } else {
                                        Vec::new()
                                    };
                                    app.assistant_manager.submit_prompt(prompt, history, hwnd);
                                }
                            }
                            wake_animation(hwnd, app);
                        } else if code == 27 {
                            // Esc: cancel / unfocus input & unpin
                            app.island_state.assistant.is_input_active = false;
                            if app.island_state.is_pinned_open {
                                app.island_state.is_pinned_open = false;
                                app.island_state.restore_previous_mode();
                            }
                            wake_animation(hwnd, app);
                        } else if code == 1 {
                            // Ctrl+A: Select All
                            app.island_state.assistant.input.select_all();
                            wake_animation(hwnd, app);
                        } else if code == 3 {
                            // Ctrl+C: Copy selected
                            if let Some(clip) = app.island_state.assistant.input.selected_text() {
                                system::shelf::copy_text_to_clipboard(&clip);
                            }
                            wake_animation(hwnd, app);
                        } else if code == 24 {
                            // Ctrl+X: Cut selected
                            if let Some(clip) = app.island_state.assistant.input.selected_text() {
                                system::shelf::copy_text_to_clipboard(&clip);
                                app.island_state.assistant.input.delete_selection();
                                app.island_state.assistant.input.update_scroll(&app.text_renderer, LayoutManager::ASSISTANT_INPUT_FONT_SIZE, false, visible_w, 16.0);
                                wake_animation(hwnd, app);
                            }
                        } else if code == 22 {
                            // Ctrl+V: Paste from clipboard
                            if let Some(clip) = read_clipboard_unicode_text(hwnd) {
                                app.island_state.assistant.input.insert_text(&clip);
                                app.island_state.assistant.input.update_scroll(&app.text_renderer, LayoutManager::ASSISTANT_INPUT_FONT_SIZE, false, visible_w, 16.0);
                                wake_animation(hwnd, app);
                            }
                        } else if code == 8 {
                            // Backspace
                            app.island_state.assistant.input.backspace();
                            app.island_state.assistant.input.update_scroll(&app.text_renderer, LayoutManager::ASSISTANT_INPUT_FONT_SIZE, false, visible_w, 16.0);
                            wake_animation(hwnd, app);
                        } else if let Some(ch) = char::from_u32(code as u32) {
                            if !ch.is_control() {
                                app.island_state.assistant.input.insert_char(ch);
                                app.island_state.assistant.input.update_scroll(&app.text_renderer, LayoutManager::ASSISTANT_INPUT_FONT_SIZE, false, visible_w, 16.0);
                                wake_animation(hwnd, app);
                            }
                        }
                    }
                }
            }
            LRESULT(0)
        }

        WM_KEYDOWN => {
            let vk = (wparam.0 & 0xFFFF) as u32;
            let is_shift = unsafe { (windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x10) as i16) < 0 };
            let is_ctrl = unsafe { (windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x11) as i16) < 0 };

            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    let cur_w = app.springs.width.current;
                    let (active_input, font_size, visible_w, scroll_margin) = if app.island_state.todo.is_input_active {
                        (Some(&mut app.island_state.todo.input), 10.0f32, LayoutManager::todo_input_visible_w(), 12.0f32)
                    } else if app.island_state.calendar.is_input_active {
                        (Some(&mut app.island_state.calendar.input), 10.0f32, LayoutManager::calendar_input_visible_w(cur_w), 12.0f32)
                    } else if app.island_state.assistant.is_input_active {
                        (Some(&mut app.island_state.assistant.input), LayoutManager::ASSISTANT_INPUT_FONT_SIZE, LayoutManager::assistant_input_visible_w(cur_w), 16.0f32)
                    } else {
                        (None, 10.0f32, 100.0f32, 12.0f32)
                    };

                    if let Some(input) = active_input {
                        match vk {
                            0x25 => {
                                // VK_LEFT
                                if is_ctrl {
                                    input.move_word_left(is_shift);
                                } else {
                                    input.move_left(is_shift);
                                }
                                input.update_scroll(&app.text_renderer, font_size, false, visible_w, scroll_margin);
                                wake_animation(hwnd, app);
                            }
                            0x27 => {
                                // VK_RIGHT
                                if is_ctrl {
                                    input.move_word_right(is_shift);
                                } else {
                                    input.move_right(is_shift);
                                }
                                input.update_scroll(&app.text_renderer, font_size, false, visible_w, scroll_margin);
                                wake_animation(hwnd, app);
                            }
                            0x24 => {
                                // VK_HOME
                                input.move_home(is_shift);
                                input.update_scroll(&app.text_renderer, font_size, false, visible_w, scroll_margin);
                                wake_animation(hwnd, app);
                            }
                            0x23 => {
                                // VK_END
                                input.move_end(is_shift);
                                input.update_scroll(&app.text_renderer, font_size, false, visible_w, scroll_margin);
                                wake_animation(hwnd, app);
                            }
                            0x2E => {
                                // VK_DELETE
                                input.delete();
                                input.update_scroll(&app.text_renderer, font_size, false, visible_w, scroll_margin);
                                wake_animation(hwnd, app);
                            }
                            0x1B => {
                                // VK_ESCAPE
                                if app.island_state.is_pinned_open || app.island_state.assistant.is_input_active {
                                    app.island_state.is_pinned_open = false;
                                    app.island_state.assistant.is_input_active = false;
                                    app.island_state.restore_previous_mode();
                                    wake_animation(hwnd, app);
                                }
                            }
                            _ => {}
                        }
                    } else if vk == 0x1B && app.island_state.is_pinned_open {
                        app.island_state.is_pinned_open = false;
                        app.island_state.assistant.is_input_active = false;
                        app.island_state.restore_previous_mode();
                        wake_animation(hwnd, app);
                    }
                }
            }
            LRESULT(0)
        }

        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
            let fw_keys = (wparam.0 & 0xFFFF) as u16;
            let is_shift = (fw_keys & 0x0004) != 0
                || unsafe { (windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x10) as i16) < 0 };
            let mut pt = POINT {
                x: (lparam.0 & 0xFFFF) as i16 as i32,
                y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
            };
            unsafe {
                let _ = ScreenToClient(hwnd, &mut pt);
            }
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    let cur_x = app.springs.x.current;
                    let cur_y = app.springs.y.current;
                    let cur_w = app.springs.width.current;
                    let mouse_x = pt.x as f32;
                    let mouse_y = pt.y as f32;

                    macro_rules! do_page_nav {
                        ($delta:expr) => {
                            app.island_state.vwheel_accum += $delta as i32;
                            if app.island_state.vwheel_accum.abs() >= 120 {
                                let forward = app.island_state.vwheel_accum < 0;
                                app.island_state.vwheel_accum = 0;
                                if app.island_state.mode == IslandMode::Resting
                                    || app.island_state.mode == IslandMode::Closed
                                {
                                    app.island_state.on_mouse_enter();
                                }
                                app.island_state.navigate_page_clamped(forward);
                                if app.island_state.active_page == ExpandedPage::Weather && app.island_state.weather.is_stale() {
                                    if let Some(wm) = &app.weather_manager {
                                        wm.trigger_refresh();
                                    }
                                }
                                wake_animation(hwnd, app);
                            }
                        };
                    }

                    // Universal Shift + MouseWheel: Always navigate pages horizontally across entire island!
                    if is_shift {
                        do_page_nav!(delta);
                    } else if let IslandMode::Expanded(ExpandedPage::ToDo) = app.island_state.mode {
                        // Smart Cursor Zone: Left side is task list, right side is menu navigation!
                        let is_over_task_list = mouse_x >= cur_x + 28.0
                            && mouse_x <= cur_x + 270.0
                            && mouse_y >= cur_y + 48.0
                            && mouse_y <= cur_y + 152.0;

                        if is_over_task_list {
                            app.island_state.vwheel_accum += delta as i32;
                            if app.island_state.vwheel_accum.abs() >= 60 {
                                if app.island_state.vwheel_accum > 0 {
                                    app.island_state.todo.scroll_up();
                                } else {
                                    app.island_state.todo.scroll_down();
                                }
                                app.island_state.vwheel_accum = 0;
                                wake_animation(hwnd, app);
                            }
                        } else {
                            do_page_nav!(delta);
                        }
                    } else if let IslandMode::Expanded(ExpandedPage::Calendar) = app.island_state.mode {
                        // Smart Cursor Zone: Only scroll month when hovered over month grid; otherwise switch pages!
                        let is_over_month_grid = mouse_x >= cur_x + 28.0
                            && mouse_x <= cur_x + 226.0
                            && mouse_y >= cur_y + 48.0
                            && mouse_y <= cur_y + 152.0;

                        if is_over_month_grid {
                            app.island_state.vwheel_accum += delta as i32;
                            if app.island_state.vwheel_accum.abs() >= 80 {
                                if app.island_state.vwheel_accum > 0 {
                                    app.island_state.calendar.prev_month();
                                } else {
                                    app.island_state.calendar.next_month();
                                }
                                app.island_state.vwheel_accum = 0;
                                wake_animation(hwnd, app);
                            }
                        } else {
                            do_page_nav!(delta);
                        }
                    } else if let IslandMode::Expanded(ExpandedPage::Assistant) = app.island_state.mode {
                        let input_rect = LayoutManager::assistant_input_rect(cur_x, cur_y, cur_w);
                        let response_rect = LayoutManager::assistant_card_rect(cur_x, cur_y, cur_w);

                        if input_rect.contains(mouse_x, mouse_y) {
                            // Over input field: scroll typed text horizontally
                            if delta > 0 {
                                app.island_state.assistant.scroll_input_left();
                            } else {
                                app.island_state.assistant.scroll_input_right();
                            }
                            wake_animation(hwnd, app);
                        } else if response_rect.contains(mouse_x, mouse_y) {
                            let max_text_w = LayoutManager::assistant_response_max_w(cur_w);
                            let lines = LayoutManager::wrap_text_lines(
                                &app.island_state.assistant.last_response,
                                max_text_w,
                                &app.text_renderer,
                                LayoutManager::ASSISTANT_BODY_FONT_SIZE,
                                160,
                            );
                            let max_s = lines.len().saturating_sub(LayoutManager::ASSISTANT_VISIBLE_LINES);
                            app.island_state.assistant.max_scroll = max_s;
                            if max_s > 0 {
                                // Over response card with scrollable text: scroll response lines
                                app.island_state.vwheel_accum += delta as i32;
                                if app.island_state.vwheel_accum.abs() >= 45 {
                                    if app.island_state.vwheel_accum > 0 {
                                        app.island_state.assistant.scroll_up();
                                    } else {
                                        app.island_state.assistant.scroll_down();
                                    }
                                    app.island_state.vwheel_accum = 0;
                                    wake_animation(hwnd, app);
                                }
                            } else {
                                // Short or empty response: switch pages!
                                do_page_nav!(delta);
                            }
                        } else {
                            // Outside inner cards (e.g. bottom dock, header, margins): switch pages!
                            do_page_nav!(delta);
                        }
                    } else if let IslandMode::Lyrics = app.island_state.mode {
                        // In Lyrics view: if cursor is on the right side (lyrics pane), scroll lyrics!
                        let div_x = cur_x + 230.0;
                        if mouse_x >= div_x {
                            app.island_state.scroll_lyrics(delta as f32);
                            wake_animation(hwnd, app);
                        } else {
                            do_page_nav!(delta);
                        }
                    } else {
                        // On other pages: accumulate until full mouse wheel click (|accum| >= 120) before switching
                        do_page_nav!(delta);
                    }
                }
            }
            LRESULT(0)
        }

        WM_MOUSEHWHEEL => {
            // Touchpad 2-finger horizontal swipe: clamped navigation ("mentok")
            let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    app.island_state.wheel_accum += delta as i32;
                    if app.island_state.wheel_accum >= 80 {
                        app.island_state.wheel_accum = 0;
                        if app.island_state.mode == IslandMode::Resting
                            || app.island_state.mode == IslandMode::Closed
                        {
                            app.island_state.on_mouse_enter();
                        }
                        // Natural touchpad swipe: positive delta cycles left/prev, clamped
                        app.island_state.navigate_page_clamped(false);
                        if app.island_state.active_page == ExpandedPage::Weather && app.island_state.weather.is_stale() {
                            if let Some(wm) = &app.weather_manager {
                                wm.trigger_refresh();
                            }
                        }
                        wake_animation(hwnd, app);
                    } else if app.island_state.wheel_accum <= -80 {
                        app.island_state.wheel_accum = 0;
                        if app.island_state.mode == IslandMode::Resting
                            || app.island_state.mode == IslandMode::Closed
                        {
                            app.island_state.on_mouse_enter();
                        }
                        // Negative delta cycles right/next, clamped
                        app.island_state.navigate_page_clamped(true);
                        if app.island_state.active_page == ExpandedPage::Weather && app.island_state.weather.is_stale() {
                            if let Some(wm) = &app.weather_manager {
                                wm.trigger_refresh();
                            }
                        }
                        wake_animation(hwnd, app);
                    }
                }
            }
            LRESULT(0)
        }

        0x0008 | 0x0006 => {
            // WM_KILLFOCUS (0x0008) | WM_ACTIVATE (0x0006): dismiss input & unpin if click occurred outside
            let is_deactivating = msg == 0x0008 || (msg == 0x0006 && (wparam.0 & 0xFFFF) == 0);
            if is_deactivating {
                if let Ok(mut lock) = APP.lock() {
                    if let Some(app) = lock.as_mut() {
                        if app.island_state.todo.is_input_active {
                            app.island_state.todo.commit_input();
                            app.island_state.todo.is_input_active = false;
                        }
                        app.island_state.assistant.is_input_active = false;
                        app.island_state.calendar.is_input_active = false;
                        let mut pt = POINT::default();
                        if unsafe { GetCursorPos(&mut pt) }.is_ok() {
                            unsafe {
                                let _ = ScreenToClient(hwnd, &mut pt);
                            }
                            let cur_x = app.springs.x.current;
                            let cur_y = app.springs.y.current;
                            let cur_w = app.springs.width.current;
                            let cur_h = app.springs.height.current;
                            let min_y = if cur_y > 2.0 { cur_y - 4.0 } else { 0.0 };
                            let in_island = (pt.x as f32) >= cur_x && (pt.x as f32) <= (cur_x + cur_w) && (pt.y as f32) >= min_y && (pt.y as f32) <= (cur_y + cur_h);
                            let in_sub = if let Some(sub) = &app.island_state.sub_notification {
                                let progress = app.island_state.sub_notif_spring.current;
                                if progress > 0.2 && !sub.is_dismissing {
                                    let sub_w = 340.0 + (400.0 - 340.0) * progress.min(1.0);
                                    let sub_h = 44.0;
                                    let sub_x = (CANVAS_W as f32 - sub_w) * 0.5;
                                    let sub_y = (cur_y + cur_h + 8.0) - (1.0 - progress.min(1.0)) * 18.0;
                                    (pt.x as f32) >= sub_x - 4.0 && (pt.x as f32) <= sub_x + sub_w + 4.0 && (pt.y as f32) >= sub_y - 4.0 && (pt.y as f32) <= sub_y + sub_h + 4.0
                                } else {
                                    false
                                }
                            } else {
                                false
                            };
                            if !in_island && !in_sub {
                                app.island_state.is_pinned_open = false;
                                app.island_state.is_hovered = false;
                                app.island_state.restore_previous_mode();
                            }
                        }
                        wake_animation(hwnd, app);
                    }
                }
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }

        WM_APP_TODO_UPDATED => {
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    app.island_state.todo.load();
                    wake_animation(hwnd, app);
                }
            }
            LRESULT(0)
        }

        WM_CLIPBOARDUPDATE => {
            // Ignore if copy was internally triggered by NotDynamicIsland to avoid feedback loops
            if system::shelf::IS_INTERNAL_CLIPBOARD_COPY.swap(false, std::sync::atomic::Ordering::SeqCst) {
                return LRESULT(0);
            }
            // Honor Windows password manager / privacy clipboard exclusion markers
            if unsafe { is_clipboard_sensitive(hwnd) } {
                return LRESULT(0);
            }

            let mut new_files = Vec::new();
            let mut new_text: Option<String> = None;
            let mut new_image: Option<(Vec<u8>, bool)> = None;

            unsafe {
                if IsClipboardFormatAvailable(15).is_ok() {
                    // CF_HDROP format 15: File(s) copied from Explorer
                    if OpenClipboard(hwnd).is_ok() {
                        if let Ok(hdata) = GetClipboardData(15) {
                            let hdrop = HDROP(hdata.0 as *mut _);
                            let count = DragQueryFileW(hdrop, 0xFFFFFFFF, None);
                            for i in 0..count {
                                let mut buf = vec![0u16; 1024];
                                let len = DragQueryFileW(hdrop, i, Some(&mut buf));
                                if len > 0 {
                                    let path_str = String::from_utf16_lossy(&buf[..len as usize]);
                                    new_files.push(PathBuf::from(path_str));
                                }
                            }
                        }
                        let _ = CloseClipboard();
                    }
                } else if IsClipboardFormatAvailable(13).is_ok() {
                    // CF_UNICODETEXT format 13: Text copied from WhatsApp, Discord, Browser, Notepad, etc.
                    if OpenClipboard(hwnd).is_ok() {
                        if let Ok(hdata) = GetClipboardData(13) {
                            new_text = read_wide_from_hglobal(windows::Win32::Foundation::HGLOBAL(hdata.0));
                        }
                        let _ = CloseClipboard();
                    }
                } else {
                    let cf_png = system::shelf::get_cf_png();
                    let capture_fmt = if cf_png != 0 && IsClipboardFormatAvailable(cf_png).is_ok() {
                        Some((cf_png, true))
                    } else if IsClipboardFormatAvailable(8).is_ok() {
                        // CF_DIB format 8: automatically synthesized by Windows for Snipping Tool / PrintScreen / Copy Image
                        Some((8, false))
                    } else {
                        None
                    };

                    if let Some((fmt, is_png)) = capture_fmt {
                        if OpenClipboard(hwnd).is_ok() {
                            if let Ok(hdata) = GetClipboardData(fmt) {
                                let hglobal = windows::Win32::Foundation::HGLOBAL(hdata.0);
                                let size = windows::Win32::System::Memory::GlobalSize(hglobal);
                                let max_bytes = if is_png { 25 * 1024 * 1024 } else { 48 * 1024 * 1024 };
                                if size >= 32 && size <= max_bytes {
                                    let ptr = windows::Win32::System::Memory::GlobalLock(hglobal) as *const u8;
                                    if !ptr.is_null() {
                                        let slice = std::slice::from_raw_parts(ptr, size);
                                        new_image = Some((slice.to_vec(), is_png));
                                        let _ = windows::Win32::System::Memory::GlobalUnlock(hglobal);
                                    }
                                }
                            }
                            let _ = CloseClipboard();
                        }
                    }
                }
            }

            if !new_files.is_empty() {
                if let Ok(mut lock) = APP.lock() {
                    if let Some(app) = lock.as_mut() {
                        let mut added = false;
                        let first_name = new_files
                            .first()
                            .and_then(|p| p.file_name())
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| "File".to_string());
                        let count = new_files.len();
                        for path in new_files {
                            if let Some(file) = system::shelf::ShelfItem::from_path(path) {
                                app.island_state.shelf.add(file);
                                added = true;
                            }
                        }
                        if added {
                            app.island_state.show_clipboard_pulse(first_name, count, ClipboardKind::File);
                            wake_animation(hwnd, app);
                        }
                    }
                }
            } else if let Some(text) = new_text {
                if let Ok(mut lock) = APP.lock() {
                    if let Some(app) = lock.as_mut() {
                        let clean_preview = text.replace('\n', " ").replace('\r', " ").trim().to_string();
                        let item = system::shelf::ShelfItem::from_text(text);
                        app.island_state.shelf.add(item);
                        app.island_state.show_clipboard_pulse(clean_preview, 1, ClipboardKind::Text);
                        wake_animation(hwnd, app);
                    }
                }
            } else if let Some((bytes, is_png)) = new_image {
                system::shelf::handle_clipboard_image_async(hwnd, bytes, is_png);
            }
            LRESULT(0)
        }

        WM_APP_VOLUME_CHANGED => {
            let level = wparam.0 as u32;
            let muted = lparam.0 == 1;
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    app.island_state.set_volume(level, muted);
                    wake_animation(hwnd, app);
                }
            }
            LRESULT(0)
        }

        WM_APP_MEDIA_CHANGED => {
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    if let Some(mc) = &app.media_ctrl {
                        let info = mc.get_info();
                        app.island_state.update_media(info);
                        wake_animation(hwnd, app);
                    }
                }
            }
            LRESULT(0)
        }

        WM_POWERBROADCAST => {
            // PBT_POWERSETTINGCHANGE = 0x8013
            if wparam.0 == 0x8013 && lparam.0 != 0 {
                let pbs = unsafe { &*(lparam.0 as *const PowerBroadcastSetting) };
                if pbs.power_setting == system::brightness::GUID_DEVICE_POWER_POLICY_VIDEO_BRIGHTNESS {
                    let level = if pbs.data_length >= 4 {
                        unsafe { *(pbs.data.as_ptr() as *const u32) }
                    } else if pbs.data_length >= 1 {
                        pbs.data[0] as u32
                    } else {
                        50
                    };
                    if let Ok(mut lock) = APP.lock() {
                        if let Some(app) = lock.as_mut() {
                            if !app.island_state.has_initialized_brightness {
                                // Silent initial registration broadcast from Windows power manager
                                app.island_state.brightness_level = level;
                                app.island_state.has_initialized_brightness = true;
                            } else if level != app.island_state.brightness_level {
                                app.island_state.set_brightness(level);
                                wake_animation(hwnd, app);
                            }
                        }
                    }
                } else if pbs.power_setting == system::power::GUID_ACDC_POWER_SOURCE
                    || pbs.power_setting == system::power::GUID_POWER_SAVING_STATUS
                    || pbs.power_setting == system::power::GUID_BATTERY_PERCENTAGE_REMAINING
                {
                    let status = PowerListener::query_power_status();
                    if let Ok(mut lock) = APP.lock() {
                        if let Some(app) = lock.as_mut() {
                            let changed = app.island_state.battery_percent != status.battery_percent
                                || app.island_state.is_charging != status.is_charging
                                || app.island_state.is_energy_saver != status.is_energy_saver;
                            if changed {
                                app.island_state.set_power_status(
                                    status.is_charging,
                                    status.is_energy_saver,
                                    status.battery_percent,
                                );
                                wake_animation(hwnd, app);
                            }
                        }
                    }
                }
            } else {
                let status = PowerListener::query_power_status();
                if let Ok(mut lock) = APP.lock() {
                    if let Some(app) = lock.as_mut() {
                        let changed = app.island_state.battery_percent != status.battery_percent
                            || app.island_state.is_charging != status.is_charging
                            || app.island_state.is_energy_saver != status.is_energy_saver;
                        if changed {
                            app.island_state.set_power_status(
                                status.is_charging,
                                status.is_energy_saver,
                                status.battery_percent,
                            );
                            wake_animation(hwnd, app);
                        }

                        // System resume from Sleep / Hibernate:
                        // PBT_APMRESUMEAUTOMATIC (0x0012) or PBT_APMRESUMESUSPEND (0x0007)
                        if wparam.0 == 0x0012 || wparam.0 == 0x0007 {
                            if let Some(wm) = &app.weather_manager {
                                wm.trigger_refresh();
                            }
                            app.island_state.calendar.trigger_sync(hwnd);
                        }
                    }
                }
            }
            LRESULT(0)
        }

        WM_APP_POWER_CHANGED => {
            let status = PowerListener::query_power_status();
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    let changed = app.island_state.battery_percent != status.battery_percent
                        || app.island_state.is_charging != status.is_charging
                        || app.island_state.is_energy_saver != status.is_energy_saver;
                    if changed {
                        app.island_state.set_power_status(
                            status.is_charging,
                            status.is_energy_saver,
                            status.battery_percent,
                        );
                        wake_animation(hwnd, app);
                    }
                }
            }
            LRESULT(0)
        }

        WM_APP_BRIGHTNESS_CHANGED => {
            let level = wparam.0 as u32;
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    app.island_state.set_brightness(level);
                    wake_animation(hwnd, app);
                }
            }
            LRESULT(0)
        }

        WM_APP_NOTIFICATION_ARRIVED => {
            let ptr = lparam.0 as *mut ToastNotificationPayload;
            if !ptr.is_null() {
                let payload = unsafe { Box::from_raw(ptr) };
                if let Ok(mut lock) = APP.lock() {
                    if let Some(app) = lock.as_mut() {
                        app.island_state.show_banner(payload.app_name, payload.title, payload.message);
                        wake_animation(hwnd, app);
                    }
                }
            }
            LRESULT(0)
        }

        WM_APP_CAPSLOCK_CHANGED => {
            let enabled = wparam.0 == 1;
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    app.island_state.set_caps_lock(enabled);
                    wake_animation(hwnd, app);
                }
            }
            LRESULT(0)
        }

        system::audio::WM_APP_HEADPHONES_CHANGED => {
            let ptr = lparam.0 as *mut (String, u32, bool);
            if !ptr.is_null() {
                let (name, battery, connected) = unsafe { *Box::from_raw(ptr) };
                if let Ok(mut lock) = APP.lock() {
                    if let Some(app) = lock.as_mut() {
                        app.island_state.set_headphones(name, battery, connected);
                        wake_animation(hwnd, app);
                    }
                }
            }
            LRESULT(0)
        }

        WM_APP_PRIVACY_CHANGED => {
            let status = system::privacy::PrivacyStatus::query();
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    let prev_cam = app.island_state.privacy.camera_active;
                    let prev_mic = app.island_state.privacy.mic_active;

                    app.island_state.privacy = status.clone();

                    // Resolve app name (including browser web calls like Instagram, WhatsApp Web, Google Meet)
                    let resolved_mic_app = if system::privacy::is_browser_app(&status.mic_app) {
                        system::privacy::detect_call_app_from_browser().unwrap_or(status.mic_app.clone())
                    } else {
                        status.mic_app.clone()
                    };

                    let resolved_cam_app = if system::privacy::is_browser_app(&status.camera_app) {
                        system::privacy::detect_call_app_from_browser().unwrap_or(status.camera_app.clone())
                    } else {
                        status.camera_app.clone()
                    };

                    // Automatic detection of active voice / video call in communication apps (Instagram, WhatsApp, Discord, Meet, etc.)
                    let is_call = (status.mic_active && system::privacy::is_call_app(&resolved_mic_app))
                        || (status.camera_active && system::privacy::is_call_app(&resolved_cam_app));

                    if is_call {
                        let is_video = status.camera_active;
                        let call_name = if !resolved_mic_app.is_empty() && system::privacy::is_call_app(&resolved_mic_app) {
                            resolved_mic_app.clone()
                        } else if !resolved_cam_app.is_empty() {
                            resolved_cam_app.clone()
                        } else {
                            "Call".to_string()
                        };
                        app.island_state.set_call_active(true, is_video, call_name);
                    } else if (!status.mic_active && !status.camera_active) && app.island_state.active_call.is_active {
                        app.island_state.set_call_active(false, false, String::new());
                    }

                    if status.camera_active && !prev_cam {
                        app.island_state.show_notification(
                            ui::NotificationKind::PrivacyAlert {
                                is_camera: true,
                                app_name: resolved_cam_app,
                            },
                            3500,
                        );
                    } else if status.mic_active && !prev_mic {
                        app.island_state.show_notification(
                            ui::NotificationKind::PrivacyAlert {
                                is_camera: false,
                                app_name: resolved_mic_app,
                            },
                            3500,
                        );
                    }
                    wake_animation(hwnd, app);
                }
            }
            LRESULT(0)
        }

        WM_APP_NETWORK_CHANGED => {
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    if let Some(nl) = &app._network_listener {
                        app.island_state.network_speed = nl.get_speed();
                        wake_animation(hwnd, app);
                    }
                    if let Some(wm) = &app.weather_manager {
                        if app.island_state.weather.is_offline {
                            wm.trigger_refresh();
                        }
                    }
                }
            }
            LRESULT(0)
        }

        WM_APP_NETWORK_TOPOLOGY_CHANGED => {
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    if let Some(wm) = &app.weather_manager {
                        wm.trigger_refresh();
                    }
                }
            }
            LRESULT(0)
        }

        WM_APP_TIME_TICK => {
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    let cur_minute = unsafe {
                        windows::Win32::System::SystemInformation::GetLocalTime().wMinute
                    };
                    if cur_minute % 20 == 0 {
                        app.island_state.calendar.trigger_sync(hwnd);
                    }
                    app.island_state.calendar.update_today();
                    wake_animation(hwnd, app);
                }
            }
            LRESULT(0)
        }

        WM_APP_CALENDAR_UPDATED => {
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    app.island_state.calendar.on_sync_completed();
                    wake_animation(hwnd, app);
                }
            }
            LRESULT(0)
        }

        WM_APP_WEATHER_UPDATED => {
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    if let Some(mgr) = app.weather_manager.as_ref() {
                        if let Some(w) = mgr.get_weather() {
                            app.island_state.weather = w;
                            wake_animation(hwnd, app);
                        }
                    }
                }
            }
            LRESULT(0)
        }

        WM_APP_ASSISTANT_REPLY => {
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    if let Some((resp, err)) = app.assistant_manager.take_reply() {
                        let clean_resp = process_assistant_actions(&resp, app);
                        app.island_state.assistant.on_reply(clean_resp, err);
                        wake_animation(hwnd, app);
                    }
                }
            }
            LRESULT(0)
        }

        system::music::WM_APP_MUSIC_STATUS => {
            let ptr = lparam.0 as *mut system::music::MusicStatusPayload;
            if !ptr.is_null() {
                let payload = unsafe { Box::from_raw(ptr) };
                if let Ok(mut lock) = APP.lock() {
                    if let Some(app) = lock.as_mut() {
                        if payload.is_error {
                            app.island_state.assistant.set_action_error(payload.message, 6);
                        } else {
                            app.island_state.assistant.set_action_done(payload.message, 5);
                        }
                        wake_animation(hwnd, app);
                    }
                }
            }
            LRESULT(0)
        }

        WM_APP_SHELF_ITEM_READY => {
            let pending_items = system::shelf::take_pending_shelf_items();
            let mut obsolete_temps = Vec::new();
            if !pending_items.is_empty() {
                if let Ok(mut lock) = APP.lock() {
                    if let Some(app) = lock.as_mut() {
                        for pending in pending_items {
                            if pending.upgrade_recent_temp {
                                if let Some(old_path) = app.island_state.shelf.upgrade_recent_temp_capture(pending.item) {
                                    obsolete_temps.push(old_path);
                                    wake_animation(hwnd, app);
                                }
                            } else {
                                let preview = if let Some((w, h)) = pending.dimensions {
                                    format!("{} · {}x{}", pending.item.name, w, h)
                                } else {
                                    pending.item.name.clone()
                                };
                                app.island_state.shelf.add(pending.item);
                                app.island_state.show_clipboard_pulse(preview, 1, ClipboardKind::Screenshot);
                                wake_animation(hwnd, app);
                            }
                        }
                    }
                }
            }
            if !obsolete_temps.is_empty() {
                std::thread::spawn(move || {
                    for old_path in obsolete_temps {
                        let _ = std::fs::remove_file(old_path);
                    }
                });
            }
            LRESULT(0)
        }

        WM_APP_DISPLAY_CURSOR_MOVED => {
            let mut cursor_pt = POINT::default();
            let _ = unsafe { GetCursorPos(&mut cursor_pt) };
            let mut move_coords: Option<(i32, i32)> = None;

            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    if app.display_manager.target == DisplayTarget::FollowCursor {
                        if let Some(rect) = app.display_manager.get_target_rect(Some(cursor_pt)) {
                            let monitor_w = rect.right - rect.left;
                            let target_x = rect.left + (monitor_w - CANVAS_W as i32) / 2;
                            let target_y = rect.top;

                            if app.window.win_x != target_x || app.window.win_y != target_y {
                                app.window.win_x = target_x;
                                app.window.win_y = target_y;
                                move_coords = Some((target_x, target_y));
                            }
                        }
                    }
                }
            }
            clear_transition_pending();

            if let Some((target_x, target_y)) = move_coords {
                // Relocate window OUTSIDE the lock to prevent re-entrant deadlock with WM_DPICHANGED
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                        hwnd,
                        windows::Win32::UI::WindowsAndMessaging::HWND_TOPMOST,
                        target_x,
                        target_y,
                        CANVAS_W as i32,
                        CANVAS_H as i32,
                        windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE
                            | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE,
                    );
                }

                if let Ok(mut lock) = APP.lock() {
                    if let Some(app) = lock.as_mut() {
                        app.backdrop.capture(target_x, target_y);
                        app.last_backdrop_capture = Instant::now();
                        wake_animation(hwnd, app);
                    }
                }
            }
            LRESULT(0)
        }

        windows::Win32::UI::WindowsAndMessaging::WM_DISPLAYCHANGE => {
            let mut cursor_pt = POINT::default();
            let _ = unsafe { GetCursorPos(&mut cursor_pt) };
            let mut move_coords: Option<(i32, i32)> = None;

            if let Ok(mut lock) = APP.try_lock() {
                if let Some(app) = lock.as_mut() {
                    app.display_manager.refresh();
                    if let Some(rect) = app.display_manager.get_target_rect(Some(cursor_pt)) {
                        let monitor_w = rect.right - rect.left;
                        let target_x = rect.left + (monitor_w - CANVAS_W as i32) / 2;
                        let target_y = rect.top;
                        if app.window.win_x != target_x || app.window.win_y != target_y {
                            app.window.win_x = target_x;
                            app.window.win_y = target_y;
                            move_coords = Some((target_x, target_y));
                        }
                    }
                }
            }

            if let Some((target_x, target_y)) = move_coords {
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                        hwnd,
                        windows::Win32::UI::WindowsAndMessaging::HWND_TOPMOST,
                        target_x,
                        target_y,
                        CANVAS_W as i32,
                        CANVAS_H as i32,
                        windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE
                            | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE,
                    );
                }

                if let Ok(mut lock) = APP.try_lock() {
                    if let Some(app) = lock.as_mut() {
                        app.backdrop.capture(target_x, target_y);
                        app.last_backdrop_capture = Instant::now();
                        wake_animation(hwnd, app);
                    }
                }
            } else {
                // Refresh rate or display resolution changed without position change (e.g. 60Hz <-> 180Hz)
                if let Ok(mut lock) = APP.try_lock() {
                    if let Some(app) = lock.as_mut() {
                        app.backdrop.capture(app.window.win_x, app.window.win_y);
                        app.last_backdrop_capture = Instant::now();
                        wake_animation(hwnd, app);
                    }
                }
            }
            LRESULT(0)
        }

        windows::Win32::UI::WindowsAndMessaging::WM_DPICHANGED => {
            // Must use try_lock to safely avoid deadlocks when SetWindowPos triggers WM_DPICHANGED synchronously
            if let Ok(mut lock) = APP.try_lock() {
                if let Some(app) = lock.as_mut() {
                    app.backdrop.capture(app.window.win_x, app.window.win_y);
                    app.last_backdrop_capture = Instant::now();
                    wake_animation(hwnd, app);
                }
            }
            LRESULT(0)
        }

        WM_TIMER => {
            if wparam.0 == TIMER_ANIMATION_ID {
                if let Ok(mut lock) = APP.lock() {
                    if let Some(app) = lock.as_mut() {
                        let now = Instant::now();
                        let dt = (now - app.last_tick).as_secs_f32().min(0.05);
                        app.last_tick = now;
                        app.island_state.anim_time += dt;

                        // Advance playback slider continuously in real-time while playing
                        if app.island_state.media_info.is_playing {
                            app.island_state.media_progress_accum += dt;
                            if app.island_state.media_progress_accum >= 1.0 {
                                let full_secs = app.island_state.media_progress_accum.floor() as u32;
                                app.island_state.media_progress_accum -= full_secs as f32;
                                if app.island_state.media_info.duration_secs > 0 {
                                    app.island_state.media_info.position_secs = (app.island_state.media_info.position_secs + full_secs).min(app.island_state.media_info.duration_secs);
                                }
                            }
                        }

                        // Advance active call timer if running
                        if app.island_state.active_call.is_active {
                            app.island_state.active_call.duration_accum += dt;
                            if app.island_state.active_call.duration_accum >= 1.0 {
                                let full_secs = app.island_state.active_call.duration_accum.floor() as u32;
                                app.island_state.active_call.duration_accum -= full_secs as f32;
                                app.island_state.active_call.duration_secs += full_secs;
                            }
                        }

                        // Advance focus timer simulation if running
                        if let Some(crate::system::timer::TimerEvent::Completed(mode)) = app.island_state.focus_timer.tick(now) {
                            let msg = match mode {
                                crate::system::timer::TimerMode::Focus => "Focus session complete! Take a break.".to_string(),
                                crate::system::timer::TimerMode::ShortBreak | crate::system::timer::TimerMode::LongBreak => "Break finished! Ready to focus.".to_string(),
                                crate::system::timer::TimerMode::Stopwatch => "Stopwatch stopped.".to_string(),
                            };
                            app.island_state.show_notification(ui::NotificationKind::TimerCompleted { message: msg }, 4500);
                            unsafe { let _ = MessageBeep(0x00000040); }
                        }

                        // Check timeout transitions (grace period unhover & notifications)
                        let timed_out = app.island_state.check_timeouts();
                        if timed_out {
                            app.settled_since = None;
                        }

                        // Compute target geometry anchored at top bezel (y = 0)
                        let (tx, ty, tw, th, tr) = app
                            .layout
                            .compute_target_geometry(CANVAS_W as f32, &app.island_state);
                        app.springs.set_target(tx, ty, tw, th, tr, 1.0);

                        // Advance spring simulation
                        let is_spring_moving = app.springs.update(dt);
                        let is_theme_moving = app.island_state.theme_spring.update(dt);
                        let is_sub_notif_moving = app.island_state.sub_notif_spring.update(dt);
                        let is_dock_moving = app.island_state.dock_pos.update(dt);

                        // Update lock-free atomic hit-testing bounds
                        let cur_x = app.springs.x.current;
                        let cur_y = app.springs.y.current;
                        let cur_w = app.springs.width.current;
                        let cur_h = app.springs.height.current;
                        let cur_r = app.springs.corner_radius.current;
                        let r_top = cur_r.min(cur_w * 0.5).min(cur_h * 0.5);
                        let hit_margin = 6.0;
                        let min_hit_y = if cur_y > 2.0 { cur_y - hit_margin } else { 0.0 };
                        HIT_MIN_X.store((cur_x - r_top - hit_margin) as i32, Ordering::Relaxed);
                        HIT_MAX_X.store((cur_x + cur_w + r_top + hit_margin) as i32, Ordering::Relaxed);
                        HIT_MIN_Y.store(min_hit_y as i32, Ordering::Relaxed);
                        HIT_MAX_Y.store((cur_y + cur_h + hit_margin) as i32, Ordering::Relaxed);

                        if let Some(sub) = &app.island_state.sub_notification {
                            let progress = app.island_state.sub_notif_spring.current;
                            if progress > 0.3 && !sub.is_dismissing {
                                let sub_w = 340.0 + (400.0 - 340.0) * progress.min(1.0);
                                let sub_h = 44.0;
                                let sub_x = (CANVAS_W as f32 - sub_w) * 0.5;
                                let sub_y = (cur_y + cur_h + 8.0) - (1.0 - progress.min(1.0)) * 18.0;
                                SUB_MIN_X.store((sub_x - hit_margin) as i32, Ordering::Relaxed);
                                SUB_MAX_X.store((sub_x + sub_w + hit_margin) as i32, Ordering::Relaxed);
                                SUB_MIN_Y.store((sub_y - hit_margin) as i32, Ordering::Relaxed);
                                SUB_MAX_Y.store((sub_y + sub_h + hit_margin) as i32, Ordering::Relaxed);
                            } else {
                                SUB_MIN_X.store(0, Ordering::Relaxed);
                                SUB_MAX_X.store(0, Ordering::Relaxed);
                            }
                        } else {
                            SUB_MIN_X.store(0, Ordering::Relaxed);
                            SUB_MAX_X.store(0, Ordering::Relaxed);
                        }

                        IS_DRAGGING_SLIDER.store(app.island_state.is_dragging_slider, Ordering::Relaxed);

                        // Get cursor position for hover states
                        let mut cursor_pt = POINT::default();
                        let cursor_pos = unsafe {
                            if GetCursorPos(&mut cursor_pt).is_ok() {
                                let _ = ScreenToClient(hwnd, &mut cursor_pt);
                                Some((cursor_pt.x as f32, cursor_pt.y as f32))
                            } else {
                                None
                            }
                        };

                        // Query active synced lyrics
                        let lyrics = app.media_ctrl.as_ref().map(|m| m.get_lyrics()).unwrap_or_default();

                        // Smooth lyrics manual scroll & recenter
                        if let Some(until) = app.island_state.lyrics_manual_until {
                            if now >= until {
                                app.island_state.lyrics_manual_until = None;
                            }
                        }
                        if app.island_state.lyrics_manual_until.is_none() {
                            app.island_state.lyrics_manual_scroll += (0.0 - app.island_state.lyrics_manual_scroll) * (dt * 6.0).min(1.0);
                        }

                        // High-resolution active index & bounded spring translation
                        let playback_time = app.island_state.get_current_playback_time();
                        let active_idx = lyrics.get_active_index(playback_time);
                        let lyric_layouts = LayoutManager::compute_lyric_layouts(&app.text_renderer, &lyrics, 304.0, app.island_state.show_romaji);
                        let active_y = LayoutManager::get_lyric_target_y(&lyric_layouts, active_idx);

                        // Strict boundary clamping: cannot scroll above line 0 or below the last line!
                        let max_scroll = lyric_layouts.last().map(|l| l.y_offset).unwrap_or(0.0);
                        let bounded_target = (active_y + app.island_state.lyrics_manual_scroll).clamp(0.0, max_scroll);
                        app.island_state.lyrics_manual_scroll = bounded_target - active_y;

                        let target_lyric_y = bounded_target;
                        let lyric_dist = (target_lyric_y - app.island_state.lyrics_scroll_y).abs();
                        app.island_state.lyrics_scroll_y += (target_lyric_y - app.island_state.lyrics_scroll_y) * (dt * 12.0).min(1.0);

                        // Real-time backdrop synchronization:
                        // 1. Instantaneous trigger if active foreground window changed (Alt-Tab, window switch)
                        let cur_fg = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
                        let fg_changed = cur_fg != app.last_foreground_hwnd;
                        if fg_changed {
                            app.last_foreground_hwnd = cur_fg;
                        }

                        // 2. High-precision refresh cadence:
                        // - Instant (0ms) on foreground window change (Alt-Tab, window switch)
                        // - While springs or themes are actively moving, NEVER capture mid-flight:
                        //   StretchBlt DWM readbacks over PCIe cause micro-stalls that disrupt VSync
                        //   frame pacing. Keeping spring frames pure CPU (<0.4ms) guarantees
                        //   rock-solid, locked 60-240 FPS buttery smoothness without a single dropped frame!
                        // - Fluid 30 FPS (~33ms) while open or hovered:
                        //   Delivers real-time synced optical blur behind the glass card for videos, scrolling, or motion.
                        // - Fluid 8 FPS (120ms) for background wallpaper luminance tracking when notch is idle/resting.
                        let is_open_or_hovered = app.island_state.is_hovered || app.island_state.is_pinned_open;
                        let is_animating_kinetics = is_spring_moving || is_theme_moving || is_sub_notif_moving || is_dock_moving;
                        let min_interval = if fg_changed {
                            std::time::Duration::from_millis(0)
                        } else if is_animating_kinetics {
                            std::time::Duration::from_secs(3600)
                        } else if is_open_or_hovered {
                            std::time::Duration::from_millis(33)
                        } else {
                            std::time::Duration::from_millis(120)
                        };

                        if now.duration_since(app.last_backdrop_capture) >= min_interval {
                            app.backdrop.capture(app.window.win_x, app.window.win_y);
                            app.last_backdrop_capture = now;
                        }

                        // Render frame into pixel buffer
                        app.pixel_buffer.clear();
                        app.layout.render(
                            &mut app.pixel_buffer,
                            &app.text_renderer,
                            &app.springs,
                            &app.island_state,
                            &lyrics,
                            cursor_pos,
                            Some(&app.backdrop),
                        );

                        // Blit to layered window DWM surface
                        app.window.update_layered_surface(&app.pixel_buffer.data);

                        // Is animation actively needed?
                        // Note: is_hovered and is_pinned_open intentionally settle once springs settle,
                        // avoiding 200 FPS continuous DWM surface updates when the card is static.
                        let is_busy = is_spring_moving
                            || is_theme_moving
                            || is_sub_notif_moving
                            || is_dock_moving
                            || app.island_state.sub_notification.is_some()
                            || app.island_state.is_dragging_slider
                            || app.island_state.pending_shelf_drag.is_some()
                            || app.island_state.todo.is_input_active
                            || app.island_state.calendar.is_input_active
                            || app.island_state.assistant.is_input_active
                            || app.island_state.assistant.is_thinking
                            || app.island_state.focus_timer.is_running
                            || app.island_state.active_call.is_active
                            || app.island_state.is_color_picker_active
                            || lyric_dist > 0.05
                            || app.island_state.lyrics_manual_scroll.abs() > 0.05
                            || app.island_state.unhover_deadline.is_some()
                            || matches!(app.island_state.mode, IslandMode::Notification(_))
                            || (app.island_state.media_info.is_playing
                                && matches!(app.island_state.mode, IslandMode::Closed | IslandMode::Expanded(ExpandedPage::Music) | IslandMode::Lyrics));

                        // Periodic working set flush every 10s even during continuous playback
                        if now.duration_since(app.last_trim) > std::time::Duration::from_secs(10) {
                            app.last_trim = now;
                            trim_working_set();
                        }

                        if !is_busy {
                            if app.settled_since.is_none() {
                                app.settled_since = Some(now);
                            } else if let Some(settled_time) = app.settled_since {
                                if now - settled_time > std::time::Duration::from_millis(500) && app.is_animating {
                                    app.is_animating = false;
                                    // Switch from monitor high-refresh animation timer to background heartbeat
                                    let idle_interval = if is_open_or_hovered { 33 } else { 120 };
                                    unsafe {
                                        let _ = SetTimer(hwnd, TIMER_ANIMATION_ID, idle_interval, None);
                                    }
                                    trim_working_set();
                                }
                            }
                        } else {
                            app.settled_since = None;
                        }
                    }
                }
            }
            LRESULT(0)
        }

        WM_DESTROY => {
            update_display_hook(HWND::default(), false);
            unsafe {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::UnregisterHotKey(hwnd, HOTKEY_AI_ASSISTANT_ID);
                let _ = RemoveClipboardFormatListener(hwnd);
                PostQuitMessage(0);
            }
            LRESULT(0)
        }

        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn main() {
    // Install emergency panic handler to capture any panics into %APPDATA%\NotDynamicIsland\crash.log
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("[PANIC] {}\n", info);
        eprintln!("{}", msg);
        let log_path = std::env::var_os("APPDATA")
            .map(|a| {
                let dir = std::path::PathBuf::from(a).join("NotDynamicIsland");
                let _ = std::fs::create_dir_all(&dir);
                dir.join("crash.log")
            })
            .unwrap_or_else(|| std::env::temp_dir().join("not_dynamic_island_crash.log"));
        let _ = std::fs::write(log_path, msg);
    }));

    // High-DPI Per-Monitor V2: Ensures pixel-perfect 1:1 crisp rendering on 100%, 125%, 150%, 200% displays!
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // Set 1ms high-precision multimedia timer resolution for rock-solid 60 FPS
        let _ = timeBeginPeriod(1);
    }

    // 0. Single-instance enforcement: prevent multiple instances from overlapping
    let mutex_name = windows::core::w!("Local\\NotDynamicIslandSingleInstanceMutex");
    unsafe {
        windows::Win32::Foundation::SetLastError(windows::Win32::Foundation::WIN32_ERROR(0));
    }
    let mutex_res = unsafe { CreateMutexW(None, true, mutex_name) };
    let last_err = unsafe { GetLastError() };
    if last_err == ERROR_ALREADY_EXISTS {
        println!("[Main] Another instance is already running. Exiting.");
        // App is already running! Wake up existing window and exit immediately.
        if let Ok(existing_hwnd) = unsafe { FindWindowW(windows::core::w!("NotDynamicIsland_Class"), None) } {
            if !existing_hwnd.is_invalid() {
                unsafe {
                    let _ = SetForegroundWindow(existing_hwnd);
                }
            }
        }
        return;
    }
    let _mutex = mutex_res.ok();

    // 1. Enable Windows EcoQoS: Execution scheduled on Efficiency Cores & deep C-state timer resolution
    enable_eco_qos();

    // 2. Initialize OLE for Drag-and-Drop, Clipboard, Core Audio, and GSMTC
    unsafe {
        let _ = windows::Win32::System::Ole::OleInitialize(None);
    }

    // 3. Create Win32 TopMost Layered ToolWindow anchored to target monitor top bezel
    let mut display_manager = DisplayManager::new();
    let mut init_pt = POINT::default();
    let _ = unsafe { GetCursorPos(&mut init_pt) };
    let target_rect = display_manager.get_target_rect(Some(init_pt)).unwrap_or(windows::Win32::Foundation::RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    });
    let monitor_w = target_rect.right - target_rect.left;
    let initial_win_x = target_rect.left + (monitor_w - CANVAS_W as i32) / 2;
    let initial_win_y = target_rect.top;

    let window = match Win32LayeredWindow::create("Project Nova - Dynamic Island", window_proc, initial_win_x, initial_win_y) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[Fatal] Failed to create window: {:?}", e);
            return;
        }
    };

    let hwnd = window.hwnd;
    enable_dark_popup_menus(hwnd);
    update_display_hook(hwnd, display_manager.target == DisplayTarget::FollowCursor);

    // Register clipboard listener to automatically capture copied files into File Shelf
    unsafe {
        let _ = AddClipboardFormatListener(hwnd);
        // Register global hotkey Ctrl+Shift+Space to summon AI Assistant
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::RegisterHotKey(
            hwnd,
            HOTKEY_AI_ASSISTANT_ID,
            windows::Win32::UI::Input::KeyboardAndMouse::MOD_CONTROL
                | windows::Win32::UI::Input::KeyboardAndMouse::MOD_SHIFT,
            0x20,
        );
    }

    // 4. Initialize state & saved preferences
    let mut island_state = IslandState::new();
    island_state.calendar.trigger_sync(hwnd);

    // 5. Initialize spring geometry at top center (Resting mode dimensions: 236x32, r:16)
    let initial_w = 236.0;
    let initial_h = 32.0;
    let initial_r = 16.0;
    let initial_x = (CANVAS_W as f32 - initial_w) * 0.5;
    let initial_y = match island_state.shape {
        ui::IslandShape::TopNotch => 0.0,
        ui::IslandShape::FloatingPill => 8.0,
    };
    let springs = SpringRect::new(initial_x, initial_y, initial_w, initial_h, initial_r);

    let r_top = initial_r.min(initial_w * 0.5).min(initial_h * 0.5);
    let hit_margin = 6.0;
    let min_hit_y = if initial_y > 2.0 { initial_y - hit_margin } else { 0.0 };
    HIT_MIN_X.store((initial_x - r_top - hit_margin) as i32, Ordering::Relaxed);
    HIT_MAX_X.store((initial_x + initial_w + r_top + hit_margin) as i32, Ordering::Relaxed);
    HIT_MIN_Y.store(min_hit_y as i32, Ordering::Relaxed);
    HIT_MAX_Y.store((initial_y + initial_h + hit_margin) as i32, Ordering::Relaxed);

    // 6. Initialize renderers
    let text_renderer = TextRenderer::new();
    let pixel_buffer = PixelBuffer::new(CANVAS_W, CANVAS_H);
    let layout = LayoutManager::new();

    // 6. Connect System Listeners
    let mut volume_listener = VolumeListener::new();
    if let Ok(()) = volume_listener.start(hwnd) {
        if let Some((vol, muted)) = volume_listener.get_current_volume() {
            island_state.volume_level = vol as f32;
            island_state.is_muted = muted;
        }
    }

    let media_ctrl = MediaController::new(hwnd);
    media_ctrl.start();

    let mut power_listener = PowerListener::new();
    power_listener.start(hwnd);

    let mut brightness_listener = BrightnessListener::new();
    brightness_listener.register(hwnd);

    let mut notification_listener = NotificationListener::new();
    notification_listener.start(hwnd);

    let mut keyboard_hook = KeyboardHook::new();
    keyboard_hook.start(hwnd);

    let mut privacy_listener = PrivacyListener::new();
    privacy_listener.start(hwnd);

    let mut network_listener = NetworkListener::new();
    network_listener.start(hwnd);

    let mut screenshot_watcher = ScreenshotWatcher::new();
    screenshot_watcher.start(hwnd);

    let weather_manager = WeatherManager::new(hwnd);
    let assistant_manager = AssistantManager::new();

    // 7. Store app state in global container
    {
        let mut backdrop = BackdropBlur::new(CANVAS_W, CANVAS_H);
        backdrop.capture(window.win_x, window.win_y);
        let cur_fg = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };

        let mut lock = APP.lock().unwrap();
        *lock = Some(App {
            window,
            springs,
            island_state,
            layout,
            text_renderer,
            pixel_buffer,
            backdrop,
            last_backdrop_capture: Instant::now(),
            last_foreground_hwnd: cur_fg,
            media_ctrl: Some(media_ctrl),
            _volume_listener: Some(volume_listener),
            _power_listener: Some(power_listener),
            _brightness_listener: Some(brightness_listener),
            _notification_listener: Some(notification_listener),
            _keyboard_hook: Some(keyboard_hook),
            _privacy_listener: Some(privacy_listener),
            _network_listener: Some(network_listener),
            _screenshot_watcher: Some(screenshot_watcher),
            weather_manager: Some(weather_manager),
            assistant_manager,
            display_manager,
            is_animating: true,
            last_tick: Instant::now(),
            settled_since: None,
            last_trim: Instant::now(),
        });
    }

    // 8. Register Shell Hook for instant 0ms notification when active window changes (Alt-Tab, window focus)
    unsafe {
        let shell_msg = RegisterWindowMessageW(windows::core::w!("SHELLHOOK"));
        SHELL_HOOK_MSG.store(shell_msg, Ordering::Relaxed);
        let _ = RegisterShellHookWindow(hwnd);
    }

    // 9. Start initial animation tick to render the first frame
    unsafe {
        let anim_interval = system::display::get_optimal_animation_interval_for_window(hwnd);
        let _ = SetTimer(hwnd, TIMER_ANIMATION_ID, anim_interval, None);
    }

    trim_working_set();

    println!("[Project Nova] Dynamic Island started with follow_this_design specifications.");

    // 9. Standard Windows Message Loop: Blocks in GetMessageW -> 0% CPU consumption at rest!
    unsafe {
        let mut msg = MSG::default();
        let mut res = GetMessageW(&mut msg, HWND::default(), 0, 0);
        while res.0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
            res = GetMessageW(&mut msg, HWND::default(), 0, 0);
        }
        println!("[Main] Message loop exited. res: {}, msg: 0x{:04X}, wparam: 0x{:X}", res.0, msg.message, msg.wParam.0);
        windows::Win32::System::Ole::OleUninitialize();
        let _ = timeEndPeriod(1);
    }
    system::shelf::cleanup_temp_shelf_snippet();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_natural_language_todo_extraction() {
        assert_eq!(
            extract_todo_intent("tambahin to-do list beli telur dan susu", "tambahin to-do list beli telur dan susu"),
            Some("Beli telur dan susu".to_string())
        );

        assert_eq!(
            extract_todo_intent("tambah tugas belajar rust", "tambah tugas belajar rust"),
            Some("Belajar rust".to_string())
        );

        assert_eq!(
            extract_todo_intent("tolong ingatkan saya besok pagi ada meeting ya", "tolong ingatkan saya besok pagi ada meeting ya"),
            Some("Besok pagi ada meeting".to_string())
        );

        assert_eq!(
            extract_todo_intent("catat tugas belanja bulanan dong", "catat tugas belanja bulanan dong"),
            Some("Belanja bulanan".to_string())
        );

        assert_eq!(
            extract_todo_intent("todo Beli kopi", "todo Beli kopi"),
            Some("Beli kopi".to_string())
        );

        // General chat should not be matched
        assert_eq!(
            extract_todo_intent("siapa presiden pertama indonesia", "siapa presiden pertama indonesia"),
            None
        );
    }

    #[test]
    fn test_natural_language_timer_extraction() {
        assert_eq!(extract_timer_intent("timer 25"), Some(25));
        assert_eq!(extract_timer_intent("pasang timer 15 menit"), Some(15));
        assert_eq!(extract_timer_intent("tolong setel timer 20 menit dong"), Some(20));
        assert_eq!(extract_timer_intent("bikin timer 5 menit"), Some(5));
        assert_eq!(extract_timer_intent("mulai pomodoro 25 menit"), Some(25));
        assert_eq!(extract_timer_intent("berapa lama membuat kopi?"), None);
    }
}
