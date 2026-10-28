//! Project Nova: High-Performance Dynamic Island for Windows 11
//! Architecture: 100% Native Rust, mimalloc, CPU Software Renderer, Event-Driven, Zero Polling.
//! Follows design specification in follow_this_design (Resting, Closed, Music, Lyrics, Shelf, To-do, Notifications)

#![windows_subsystem = "windows"]

use std::path::PathBuf;
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
use render::{PixelBuffer, TextRenderer};
use system::{
    enable_eco_qos, trim_working_set, BrightnessListener, KeyboardHook, MediaController,
    NetworkListener, NotificationListener, PowerListener, PrivacyListener, ToastNotificationPayload, VolumeListener,
    WM_APP_BRIGHTNESS_CHANGED, WM_APP_CAPSLOCK_CHANGED, WM_APP_MEDIA_CHANGED,
    WM_APP_NETWORK_CHANGED, WM_APP_NOTIFICATION_ARRIVED, WM_APP_POWER_CHANGED, WM_APP_PRIVACY_CHANGED,
    WM_APP_TODO_UPDATED, WM_APP_VOLUME_CHANGED,
};
use ui::{ClickAction, ExpandedPage, IslandMode, IslandState, LayoutManager, Rect};
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
    AppendMenuW, CreatePopupMenu, DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW,
    FindWindowW, GetCursorPos, GetMessageW, KillTimer, PostQuitMessage, SetForegroundWindow,
    SetTimer, TrackPopupMenu, TranslateMessage, HTCLIENT, HTTRANSPARENT, MF_SEPARATOR, MF_STRING,
    MSG, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_CHAR, WM_DESTROY, WM_DROPFILES,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCHITTEST, WM_POWERBROADCAST, WM_RBUTTONUP,
    WM_TIMER,
};

const WM_MOUSEWHEEL: u32 = 0x020A;
const WM_MOUSEHWHEEL: u32 = 0x020E;
const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_CLIPBOARDUPDATE: u32 = 0x031D;

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
    media_ctrl: Option<MediaController>,
    _volume_listener: Option<VolumeListener>,
    _power_listener: Option<PowerListener>,
    _brightness_listener: Option<BrightnessListener>,
    _notification_listener: Option<NotificationListener>,
    _keyboard_hook: Option<KeyboardHook>,
    _privacy_listener: Option<system::privacy::PrivacyListener>,
    _network_listener: Option<system::network::NetworkListener>,
    is_animating: bool,
    last_tick: Instant,
    settled_since: Option<Instant>,
    last_trim: Instant,
}

unsafe impl Send for App {}

static APP: Mutex<Option<App>> = Mutex::new(None);

fn wake_animation(hwnd: HWND, app: &mut App) {
    if !app.is_animating {
        app.is_animating = true;
        app.settled_since = None;
        app.last_tick = Instant::now();
        unsafe {
            let _ = SetTimer(hwnd, TIMER_ANIMATION_ID, 16, None);
        }
    }
}

pub unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCHITTEST => {
            // Hit-testing passthrough: return HTTRANSPARENT when outside the top notch bounds
            let screen_x = (lparam.0 & 0xFFFF) as i16 as i32;
            let screen_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;

            let mut pt = POINT {
                x: screen_x,
                y: screen_y,
            };
            unsafe {
                let _ = ScreenToClient(hwnd, &mut pt);
            }

            if let Ok(lock) = APP.lock() {
                if let Some(app) = lock.as_ref() {
                    // While dragging slider, capture all hit testing
                    if app.island_state.is_dragging_slider {
                        return LRESULT(HTCLIENT as isize);
                    }

                    let cur_x = app.springs.x.current;
                    let cur_w = app.springs.width.current;
                    let cur_h = app.springs.height.current;
                    let cur_r = app.springs.corner_radius.current;
                    let r_top = cur_r.min(cur_w * 0.5).min(cur_h * 0.5);

                    let hit_margin = 6.0;
                    if pt.x as f32 >= cur_x - r_top - hit_margin
                        && pt.x as f32 <= cur_x + cur_w + r_top + hit_margin
                        && pt.y >= 0
                        && (pt.y as f32) <= cur_h + hit_margin
                    {
                        return LRESULT(HTCLIENT as isize);
                    }
                }
            }
            // Pass clicks through to whatever application is behind the transparent canvas!
            LRESULT(HTTRANSPARENT as isize)
        }

        WM_LBUTTONDOWN => {
            let cx = (lparam.0 & 0xFFFF) as i16 as f32;
            let cy = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;

            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    let cur_x = app.springs.x.current;
                    let cur_w = app.springs.width.current;

                    // 1. Check if user clicked/dragged on the media seek bar
                    match app.island_state.mode {
                        IslandMode::Expanded(ExpandedPage::Music) => {
                            let progress_x = cur_x + 56.0;
                            let progress_w = cur_w - 112.0;
                            let bar_y = 90.0;
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
                            let bar_y = 78.0;
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
                            let card_rect = app.layout.get_shelf_card_rect(i, item_count, cur_x, cur_w);
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
                        let left_x = cur_x + 44.0;
                        let input_rect = Rect { x: left_x, y: 92.0, w: 140.0, h: 26.0 };
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

            // Check if pending shelf drag exceeded drag threshold (4px)
            // MUST release APP.lock() BEFORE start_drag_drop to prevent re-entrant modal deadlocks during DoDragDrop!
            let mut shelf_drag_item: Option<system::shelf::ShelfItem> = None;
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
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
                }
            }

            if let Some(item) = shelf_drag_item {
                system::shelf::start_drag_drop(&item);
                if let Ok(mut lock) = APP.lock() {
                    if let Some(app) = lock.as_mut() {
                        wake_animation(hwnd, app);
                    }
                }
                return LRESULT(0);
            }

            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
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
                    }
                }
            }
            LRESULT(0)
        }

        WM_MOUSELEAVE => {
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    if !app.island_state.is_dragging_slider && !app.island_state.todo.is_input_active {
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

            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    // If user was pending shelf drag, but released without dragging -> copy to clipboard!
                    if let Some((idx, _, _)) = app.island_state.pending_shelf_drag.take() {
                        unsafe {
                            let _ = ReleaseCapture();
                        }
                        if let Some(item) = app.island_state.shelf.items.get(idx).cloned() {
                            match &item.kind {
                                system::shelf::ShelfItemKind::File(path) => {
                                    system::shelf::copy_file_to_clipboard_hdrop(path);
                                    app.island_state.show_banner(
                                        "Shelf".to_string(),
                                        "File Disalin ke Clipboard".to_string(),
                                        item.name.clone(),
                                    );
                                }
                                system::shelf::ShelfItemKind::Text(text) => {
                                    system::shelf::copy_text_to_clipboard(text);
                                    app.island_state.show_banner(
                                        "Shelf".to_string(),
                                        "Teks Disalin ke Clipboard".to_string(),
                                        item.name.clone(),
                                    );
                                }
                            }
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

                    match action {
                        Some(ClickAction::PrevPage) => {
                            app.island_state.cycle_page(false);
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::NextPage) => {
                            app.island_state.cycle_page(true);
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::SelectPage(page)) => {
                            app.island_state.select_page(page);
                            wake_animation(hwnd, app);
                        }
                        Some(ClickAction::ToggleLyrics) => {
                            app.island_state.toggle_lyrics();
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
                                match &item.kind {
                                    system::shelf::ShelfItemKind::File(path) => {
                                        system::shelf::copy_file_to_clipboard_hdrop(path);
                                        app.island_state.show_banner(
                                            "Shelf".to_string(),
                                            "File Disalin ke Clipboard".to_string(),
                                            item.name.clone(),
                                        );
                                    }
                                    system::shelf::ShelfItemKind::Text(text) => {
                                        system::shelf::copy_text_to_clipboard(text);
                                        app.island_state.show_banner(
                                            "Shelf".to_string(),
                                            "Teks Disalin ke Clipboard".to_string(),
                                            item.name.clone(),
                                        );
                                    }
                                }
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

            // Right-click context menu: quick switch to any page, test notifications, and exit
            let mut pt = POINT::default();
            unsafe {
                if GetCursorPos(&mut pt).is_ok() {
                    if let Ok(hmenu) = CreatePopupMenu() {
                        let _ = AppendMenuW(hmenu, MF_STRING, 1001, windows::core::w!("🎵  Music Controller"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1002, windows::core::w!("📁  File Shelf (Up to 5 files)"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1003, windows::core::w!("✅  TodoList"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1015, windows::core::w!("⏱️  Focus Timer & Stopwatch"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1022, windows::core::w!("🛠️  Tools & Control Center"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1004, windows::core::w!("💬  Toggle Synced Lyrics"));
                        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
                        let _ = AppendMenuW(hmenu, MF_STRING, 1016, windows::core::w!("🎯  Pick Screen Color (Eyedropper)"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1017, windows::core::w!("✂️  Screen Snip (Snipping Tool)"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1018, windows::core::w!("🎙️  Toggle Microphone Mute"));
                        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
                        let _ = AppendMenuW(hmenu, MF_STRING, 1019, windows::core::w!("🟢  Test Camera In-Use Privacy Alert"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1020, windows::core::w!("🟠  Test Microphone In-Use Privacy Alert"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1023, windows::core::w!("📞  Test Active Voice Call (WhatsApp - 03:25)"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1024, windows::core::w!("📹  Test Active Video Call (Discord - 01:45)"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1025, windows::core::w!("⏹️  End Call HUD Simulation"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1021, windows::core::w!("🔔  Test Focus Session Completed"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1005, windows::core::w!("⚡  Test Charging Notification"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1009, windows::core::w!("🔌  Test Charger Disconnected"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1010, windows::core::w!("☀️  Test Brightness Notification"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1011, windows::core::w!("🔔  Test WhatsApp Banner"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1012, windows::core::w!("🔋  Toggle Energy Saver (#EAA300)"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1006, windows::core::w!("🎧  Test Bluetooth Headphones Connected (AirBuds 85%)"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1013, windows::core::w!("🎧  Test Wired Headset Connected (3.5mm Jack)"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1014, windows::core::w!("🔌  Test Headphones Disconnected"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1007, windows::core::w!("🔊  Test Volume Notification"));
                        let _ = AppendMenuW(hmenu, MF_STRING, 1008, windows::core::w!("🔠  Test Caps Lock Notification"));
                        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, None);
                        let _ = AppendMenuW(hmenu, MF_STRING, 1099, windows::core::w!("❌  Exit NotDynamicIsland"));

                        let _ = SetForegroundWindow(hwnd);
                        let cmd = TrackPopupMenu(
                            hmenu,
                            TPM_RETURNCMD | TPM_RIGHTBUTTON,
                            pt.x,
                            pt.y,
                            0,
                            hwnd,
                            None,
                        );
                        let _ = DestroyMenu(hmenu);

                        if cmd.0 == 1099 {
                            let _ = DestroyWindow(hwnd);
                        } else if cmd.0 != 0 {
                            if let Ok(mut lock) = APP.lock() {
                                if let Some(app) = lock.as_mut() {
                                    match cmd.0 {
                                        1001 => app.island_state.select_page(ExpandedPage::Music),
                                        1002 => app.island_state.select_page(ExpandedPage::Shelf),
                                        1003 => app.island_state.select_page(ExpandedPage::ToDo),
                                        1004 => app.island_state.toggle_lyrics(),
                                        1005 => app.island_state.set_charging(true, 70),
                                        1006 => app.island_state.set_headphones("AirBuds Pro".to_string(), 85, true),
                                        1013 => app.island_state.set_headphones("Wired Headphones".to_string(), 0, true),
                                        1014 => app.island_state.set_headphones("AirBuds Pro".to_string(), 0, false),
                                        1007 => app.island_state.set_volume(75, false),
                                        1008 => app.island_state.set_caps_lock(true),
                                        1009 => app.island_state.set_charging(false, 68),
                                        1010 => app.island_state.set_brightness(75),
                                        1011 => app.island_state.show_banner(
                                            "WhatsApp".to_string(),
                                            "Budi Santoso".to_string(),
                                            "Halo! File dokumen kemarin sudah saya kirim ya".to_string(),
                                        ),
                                        1012 => {
                                            let current = app.island_state.is_energy_saver;
                                            app.island_state.is_energy_saver = !current;
                                        }
                                        1015 => app.island_state.select_page(ExpandedPage::Timer),
                                        1022 => app.island_state.select_page(ExpandedPage::Tools),
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
                                        1019 => {
                                            app.island_state.show_privacy_alert(true, "Camera".to_string());
                                        }
                                        1020 => {
                                            app.island_state.show_privacy_alert(false, "Microphone".to_string());
                                        }
                                        1021 => {
                                            app.island_state.show_timer_completed("Focus session completed! Great job taking 25 minutes of deep focus.".to_string());
                                        }
                                        1023 => {
                                            app.island_state.set_call_active(true, false, "WhatsApp".to_string());
                                            app.island_state.active_call.duration_secs = 205;
                                        }
                                        1024 => {
                                            app.island_state.set_call_active(true, true, "Discord".to_string());
                                            app.island_state.active_call.duration_secs = 105;
                                        }
                                        1025 => {
                                            app.island_state.set_call_active(false, false, String::new());
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
                            // Enter: commit typed task
                            app.island_state.todo.commit_input();
                            wake_animation(hwnd, app);
                        } else if code == 27 {
                            // Esc: cancel input
                            app.island_state.todo.cancel_input();
                            wake_animation(hwnd, app);
                        } else if code == 8 {
                            // Backspace
                            app.island_state.todo.input_backspace();
                            wake_animation(hwnd, app);
                        } else if let Some(ch) = char::from_u32(code as u32) {
                            if !ch.is_control() {
                                app.island_state.todo.input_char(ch);
                                wake_animation(hwnd, app);
                            }
                        }
                    }
                }
            }
            LRESULT(0)
        }

        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
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

                    if let IslandMode::Expanded(ExpandedPage::ToDo) = app.island_state.mode {
                        // In ToDo page: strictly scroll the task list and NEVER glitch-switch pages!
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
                    } else if let IslandMode::Lyrics = app.island_state.mode {
                        // In Lyrics view: if cursor is on the right side (lyrics pane), scroll lyrics!
                        let div_x = cur_x + 230.0;
                        if (pt.x as f32) >= div_x {
                            app.island_state.scroll_lyrics(delta as f32);
                            wake_animation(hwnd, app);
                        } else {
                            // On the left side: switch pages
                            app.island_state.vwheel_accum += delta as i32;
                            if app.island_state.vwheel_accum.abs() >= 120 {
                                let forward = app.island_state.vwheel_accum < 0;
                                app.island_state.vwheel_accum = 0;
                                app.island_state.navigate_page_clamped(forward);
                                wake_animation(hwnd, app);
                            }
                        }
                    } else {
                        // On other pages: accumulate until full mouse wheel click (|accum| >= 120) before switching
                        app.island_state.vwheel_accum += delta as i32;
                        if app.island_state.vwheel_accum.abs() >= 120 {
                            let forward = app.island_state.vwheel_accum < 0;
                            app.island_state.vwheel_accum = 0;
                            if app.island_state.mode == IslandMode::Resting
                                || app.island_state.mode == IslandMode::Closed
                            {
                                app.island_state.on_mouse_enter();
                            }
                            // Clamped navigation: stops at ends ("mentok")
                            app.island_state.navigate_page_clamped(forward);
                            wake_animation(hwnd, app);
                        }
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
                        wake_animation(hwnd, app);
                    }
                }
            }
            LRESULT(0)
        }

        0x0008 | 0x0006 => {
            // WM_KILLFOCUS | WM_ACTIVATE: dismiss input mode & auto-minimize if cursor is away
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    if app.island_state.todo.is_input_active {
                        app.island_state.todo.commit_input();
                        app.island_state.todo.is_input_active = false;
                    }
                    let mut pt = POINT::default();
                    if unsafe { GetCursorPos(&mut pt) }.is_ok() {
                        unsafe {
                            let _ = ScreenToClient(hwnd, &mut pt);
                        }
                        let cur_x = app.springs.x.current;
                        let cur_w = app.springs.width.current;
                        let cur_h = app.springs.height.current;
                        if (pt.x as f32) < cur_x || (pt.x as f32) > (cur_x + cur_w) || pt.y < 0 || (pt.y as f32) > cur_h {
                            app.island_state.is_hovered = false;
                            app.island_state.unhover_deadline = Some(Instant::now() + std::time::Duration::from_millis(500));
                        }
                    }
                    wake_animation(hwnd, app);
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

            let mut new_files = Vec::new();
            let mut new_text: Option<String> = None;

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
                            let ptr = windows::Win32::System::Memory::GlobalLock(
                                windows::Win32::Foundation::HGLOBAL(hdata.0),
                            ) as *const u16;
                            if !ptr.is_null() {
                                let mut len = 0;
                                while *ptr.add(len) != 0 {
                                    len += 1;
                                }
                                let slice = std::slice::from_raw_parts(ptr, len);
                                let text = String::from_utf16_lossy(slice).trim().to_string();
                                let _ = windows::Win32::System::Memory::GlobalUnlock(
                                    windows::Win32::Foundation::HGLOBAL(hdata.0),
                                );
                                if !text.is_empty() {
                                    new_text = Some(text);
                                }
                            }
                        }
                        let _ = CloseClipboard();
                    }
                }
            }

            if !new_files.is_empty() {
                if let Ok(mut lock) = APP.lock() {
                    if let Some(app) = lock.as_mut() {
                        let mut added = false;
                        for path in new_files {
                            if let Some(file) = system::shelf::ShelfItem::from_path(path) {
                                app.island_state.shelf.add(file);
                                added = true;
                            }
                        }
                        if added {
                            wake_animation(hwnd, app);
                        }
                    }
                }
            } else if let Some(text) = new_text {
                if let Ok(mut lock) = APP.lock() {
                    if let Some(app) = lock.as_mut() {
                        let item = system::shelf::ShelfItem::from_text(text);
                        app.island_state.shelf.add(item);
                        wake_animation(hwnd, app);
                    }
                }
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
                {
                    let status = PowerListener::query_power_status();
                    if let Ok(mut lock) = APP.lock() {
                        if let Some(app) = lock.as_mut() {
                            app.island_state.set_power_status(
                                status.is_charging,
                                status.is_energy_saver,
                                status.battery_percent,
                            );
                            wake_animation(hwnd, app);
                        }
                    }
                }
            } else {
                let status = PowerListener::query_power_status();
                if let Ok(mut lock) = APP.lock() {
                    if let Some(app) = lock.as_mut() {
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

        WM_APP_POWER_CHANGED => {
            let status = PowerListener::query_power_status();
            if let Ok(mut lock) = APP.lock() {
                if let Some(app) = lock.as_mut() {
                    app.island_state.set_power_status(
                        status.is_charging,
                        status.is_energy_saver,
                        status.battery_percent,
                    );
                    wake_animation(hwnd, app);
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

                    // Automatic detection of active voice / video call in communication apps
                    if status.mic_active && system::privacy::is_call_app(&status.mic_app) {
                        let is_video = status.camera_active;
                        app.island_state.set_call_active(true, is_video, status.mic_app.clone());
                    } else if !status.mic_active && app.island_state.active_call.is_active {
                        app.island_state.set_call_active(false, false, String::new());
                    }

                    if status.camera_active && !prev_cam {
                        app.island_state.show_notification(
                            ui::NotificationKind::PrivacyAlert {
                                is_camera: true,
                                app_name: status.camera_app.clone(),
                            },
                            3500,
                        );
                    } else if status.mic_active && !prev_mic {
                        app.island_state.show_notification(
                            ui::NotificationKind::PrivacyAlert {
                                is_camera: false,
                                app_name: status.mic_app.clone(),
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
                }
            }
            LRESULT(0)
        }

        WM_TIMER => {
            if wparam.0 == TIMER_ANIMATION_ID {
                let mut should_kill_timer = false;

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
                        let lyric_layouts = LayoutManager::compute_lyric_layouts(&app.text_renderer, &lyrics, 304.0);
                        let active_y = LayoutManager::get_lyric_target_y(&lyric_layouts, active_idx);

                        // Strict boundary clamping: cannot scroll above line 0 or below the last line!
                        let max_scroll = lyric_layouts.last().map(|l| l.y_offset).unwrap_or(0.0);
                        let bounded_target = (active_y + app.island_state.lyrics_manual_scroll).clamp(0.0, max_scroll);
                        app.island_state.lyrics_manual_scroll = bounded_target - active_y;

                        let target_lyric_y = bounded_target;
                        let lyric_dist = (target_lyric_y - app.island_state.lyrics_scroll_y).abs();
                        app.island_state.lyrics_scroll_y += (target_lyric_y - app.island_state.lyrics_scroll_y) * (dt * 12.0).min(1.0);

                        // Render frame into pixel buffer
                        app.pixel_buffer.clear();
                        app.layout.render(
                            &mut app.pixel_buffer,
                            &app.text_renderer,
                            &app.springs,
                            &app.island_state,
                            &lyrics,
                            cursor_pos,
                        );

                        // Blit to layered window DWM surface
                        app.window.update_layered_surface(&app.pixel_buffer.data);

                        // Is animation actively needed?
                        let is_busy = is_spring_moving
                            || app.island_state.is_hovered
                            || app.island_state.is_dragging_slider
                            || app.island_state.pending_shelf_drag.is_some()
                            || app.island_state.todo.is_input_active
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
                                if now - settled_time > std::time::Duration::from_millis(500) {
                                    should_kill_timer = true;
                                    app.is_animating = false;
                                }
                            }
                        } else {
                            app.settled_since = None;
                        }
                    }
                }

                if should_kill_timer {
                    unsafe {
                        let _ = KillTimer(hwnd, TIMER_ANIMATION_ID);
                    }
                    // Idle working set trimming flushes memory down to ~1-2 MB!
                    trim_working_set();
                }
            }
            LRESULT(0)
        }

        WM_DESTROY => {
            unsafe {
                let _ = RemoveClipboardFormatListener(hwnd);
                PostQuitMessage(0);
            }
            LRESULT(0)
        }

        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn main() {
    // Install emergency panic handler to capture any panics into crash.log
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("[PANIC] {}\n", info);
        eprintln!("{}", msg);
        let _ = std::fs::write("crash.log", msg);
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

    // 3. Create Win32 TopMost Layered ToolWindow anchored to the top bezel
    let window = match Win32LayeredWindow::create("Project Nova - Dynamic Island", window_proc) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[Fatal] Failed to create window: {:?}", e);
            return;
        }
    };

    let hwnd = window.hwnd;

    // Register clipboard listener to automatically capture copied files into File Shelf
    unsafe {
        let _ = AddClipboardFormatListener(hwnd);
    }

    // 4. Initialize spring geometry at top center (Resting mode dimensions: 236x32, r:16)
    let initial_w = 236.0;
    let initial_h = 32.0;
    let initial_r = 16.0;
    let initial_x = (CANVAS_W as f32 - initial_w) * 0.5;
    let initial_y = 0.0;
    let springs = SpringRect::new(initial_x, initial_y, initial_w, initial_h, initial_r);

    // 5. Initialize renderers
    let text_renderer = TextRenderer::new();
    let pixel_buffer = PixelBuffer::new(CANVAS_W, CANVAS_H);
    let layout = LayoutManager::new();
    let mut island_state = IslandState::new();

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
    power_listener.register(hwnd);

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

    // 7. Store app state in global container
    {
        let mut lock = APP.lock().unwrap();
        *lock = Some(App {
            window,
            springs,
            island_state,
            layout,
            text_renderer,
            pixel_buffer,
            media_ctrl: Some(media_ctrl),
            _volume_listener: Some(volume_listener),
            _power_listener: Some(power_listener),
            _brightness_listener: Some(brightness_listener),
            _notification_listener: Some(notification_listener),
            _keyboard_hook: Some(keyboard_hook),
            _privacy_listener: Some(privacy_listener),
            _network_listener: Some(network_listener),
            is_animating: true,
            last_tick: Instant::now(),
            settled_since: None,
            last_trim: Instant::now(),
        });
    }

    // 8. Start initial animation tick to render the first frame
    unsafe {
        let _ = SetTimer(hwnd, TIMER_ANIMATION_ID, 16, None);
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
}
