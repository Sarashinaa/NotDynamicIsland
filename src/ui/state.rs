//! UI State Management for the Dynamic Island
//! Implements exact designs from follow_this_design (Resting, Closed, Music, Lyrics, Shelf, To-do, Notifications)

use std::time::Instant;
use crate::system::calendar::CalendarManager;
use crate::system::media::MediaTrackInfo;
use crate::system::network::NetworkSpeed;
use crate::system::privacy::PrivacyStatus;
use crate::system::shelf::FileShelf;
use crate::system::timer::FocusTimer;
use crate::system::todo::TodoList;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExpandedPage {
    Music = 0,
    Shelf = 1,
    ToDo = 2,
    Calendar = 3,
    Timer = 4,
    Tools = 5,
}

impl ExpandedPage {
    #[allow(dead_code)]
    pub fn next(&self) -> Self {
        match self {
            Self::Music => Self::Shelf,
            Self::Shelf => Self::ToDo,
            Self::ToDo => Self::Calendar,
            Self::Calendar => Self::Timer,
            Self::Timer => Self::Tools,
            Self::Tools => Self::Music,
        }
    }

    #[allow(dead_code)]
    pub fn prev(&self) -> Self {
        match self {
            Self::Music => Self::Tools,
            Self::Shelf => Self::Music,
            Self::ToDo => Self::Shelf,
            Self::Calendar => Self::ToDo,
            Self::Timer => Self::Calendar,
            Self::Tools => Self::Timer,
        }
    }

    #[allow(dead_code)]
    pub fn index(&self) -> usize {
        *self as usize
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum NotificationKind {
    Charging { percent: u32 },
    ChargerDisconnected { percent: u32 },
    Brightness { level: u32 },
    Headphones { name: String, battery: u32, connected: bool },
    Volume { level: u32, muted: bool },
    CapsLock { enabled: bool },
    Banner { app_name: String, title: String, message: String },
    ColorPicked { hex: String, rgb: String },
    PrivacyAlert { is_camera: bool, app_name: String },
    TimerCompleted { message: String },
}

#[derive(Clone, Debug, PartialEq)]
pub enum IslandMode {
    Resting,                              // resting.png: robot eyes
    Closed,                               // closed.png: battery + time + sound wave
    Expanded(ExpandedPage),               // music.png / shelf.png / to-do.png
    Lyrics,                               // lyrics.png: side-by-side player & synced lyrics
    Notification(NotificationKind),       // charging.png / headphones.png / volume / capslock / banner
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActiveCallInfo {
    pub is_active: bool,
    pub is_video: bool,
    pub app_name: String,
    pub duration_secs: u32,
    pub duration_accum: f32,
}

impl ActiveCallInfo {
    pub fn format_duration(&self) -> String {
        let mins = self.duration_secs / 60;
        let secs = self.duration_secs % 60;
        format!("{:02}:{:02}", mins, secs)
    }
}

pub struct IslandState {
    pub mode: IslandMode,
    pub previous_mode: IslandMode,
    pub timeout_at: Option<Instant>,
    pub unhover_deadline: Option<Instant>,
    pub active_page: ExpandedPage,
    pub media_info: MediaTrackInfo,
    pub shelf: FileShelf,
    pub todo: TodoList,
    pub active_call: ActiveCallInfo,
    pub is_hovered: bool,
    pub battery_percent: u32,
    pub is_charging: bool,
    pub is_energy_saver: bool,
    pub brightness_level: u32,
    pub has_initialized_brightness: bool,
    pub volume_level: f32,
    pub is_muted: bool,
    pub caps_lock: bool,
    pub anim_time: f32,
    pub media_progress_accum: f32,
    pub is_dragging_slider: bool,
    pub drag_seek_secs: Option<u32>,
    pub wheel_accum: i32,
    pub vwheel_accum: i32,
    pub lyrics_scroll_y: f32,
    pub lyrics_manual_scroll: f32,
    pub lyrics_manual_until: Option<Instant>,
    pub pending_shelf_drag: Option<(usize, f32, f32)>,
    pub privacy: PrivacyStatus,
    pub focus_timer: FocusTimer,
    pub calendar: CalendarManager,
    pub network_speed: NetworkSpeed,
    pub is_color_picker_active: bool,
    #[allow(dead_code)]
    pub recording_start_time: Option<Instant>,
}

impl IslandState {
    pub fn new() -> Self {
        let (bat, chg) = query_battery_status();
        Self {
            mode: IslandMode::Resting,
            previous_mode: IslandMode::Resting,
            timeout_at: None,
            unhover_deadline: None,
            active_page: ExpandedPage::Music,
            media_info: MediaTrackInfo::default(),
            shelf: FileShelf::new(),
            todo: TodoList::new(),
            is_hovered: false,
            battery_percent: bat,
            is_charging: chg,
            is_energy_saver: false,
            brightness_level: 50,
            has_initialized_brightness: false,
            volume_level: 50.0,
            is_muted: false,
            caps_lock: false,
            anim_time: 0.0,
            media_progress_accum: 0.0,
            is_dragging_slider: false,
            drag_seek_secs: None,
            wheel_accum: 0,
            vwheel_accum: 0,
            lyrics_scroll_y: 0.0,
            lyrics_manual_scroll: 0.0,
            lyrics_manual_until: None,
            pending_shelf_drag: None,
            privacy: PrivacyStatus::default(),
            focus_timer: FocusTimer::new(),
            calendar: CalendarManager::new(),
            network_speed: NetworkSpeed::default(),
            is_color_picker_active: false,
            recording_start_time: None,
            active_call: ActiveCallInfo::default(),
        }
    }

    pub fn set_call_active(&mut self, active: bool, is_video: bool, app_name: String) {
        if active {
            if !self.active_call.is_active || self.active_call.app_name != app_name {
                self.active_call.duration_secs = 0;
                self.active_call.duration_accum = 0.0;
            }
            self.active_call.is_active = true;
            self.active_call.is_video = is_video;
            self.active_call.app_name = if app_name.is_empty() { "Call".to_string() } else { app_name };
            if self.mode == IslandMode::Resting || self.mode == IslandMode::Closed {
                self.mode = IslandMode::Resting;
            }
        } else {
            self.active_call.is_active = false;
            self.active_call.duration_secs = 0;
            self.active_call.duration_accum = 0.0;
        }
    }

    pub fn get_current_playback_time(&self) -> f32 {
        (self.media_info.position_secs as f32) + self.media_progress_accum
    }

    pub fn scroll_lyrics(&mut self, delta: f32) {
        // Delta > 0: scroll up/backward in lyrics; Delta < 0: scroll down/forward
        self.lyrics_manual_scroll -= delta * 0.22;
        self.lyrics_manual_until = Some(Instant::now() + std::time::Duration::from_millis(3500));
    }

    pub fn reset_lyrics_manual_scroll(&mut self) {
        self.lyrics_manual_scroll = 0.0;
        self.lyrics_manual_until = None;
    }

    /// Mouse entered island bounds: automatically expands to current active page
    pub fn on_mouse_enter(&mut self) {
        self.is_hovered = true;
        self.unhover_deadline = None;

        match self.mode {
            IslandMode::Resting | IslandMode::Closed => {
                // Keep active_page in 100% sync with displayed page
                self.mode = IslandMode::Expanded(self.active_page);
            }
            _ => {}
        }
    }

    /// Mouse left island bounds: set grace period before collapsing to Resting or Closed
    pub fn on_mouse_leave(&mut self) {
        self.is_hovered = false;
        // Grace period of 1.0 second before collapsing
        self.unhover_deadline = Some(Instant::now() + std::time::Duration::from_millis(1000));
    }

    pub fn cycle_page(&mut self, forward: bool) {
        self.navigate_page_clamped(forward);
    }

    /// User requirement: Clamped navigation so gesture/scrollwheel stops at the ends ("mentok")
    pub fn navigate_page_clamped(&mut self, forward: bool) {
        self.todo.cancel_input();
        self.calendar.cancel_input();
        let target = match self.active_page {
            ExpandedPage::Music => {
                if forward {
                    ExpandedPage::Shelf
                } else {
                    ExpandedPage::Music
                }
            }
            ExpandedPage::Shelf => {
                if forward {
                    ExpandedPage::ToDo
                } else {
                    ExpandedPage::Music
                }
            }
            ExpandedPage::ToDo => {
                if forward {
                    ExpandedPage::Calendar
                } else {
                    ExpandedPage::Shelf
                }
            }
            ExpandedPage::Calendar => {
                if forward {
                    ExpandedPage::Timer
                } else {
                    ExpandedPage::ToDo
                }
            }
            ExpandedPage::Timer => {
                if forward {
                    ExpandedPage::Tools
                } else {
                    ExpandedPage::Calendar
                }
            }
            ExpandedPage::Tools => {
                if forward {
                    ExpandedPage::Tools
                } else {
                    ExpandedPage::Timer
                }
            }
        };
        if target != self.active_page {
            self.active_page = target;
            self.mode = IslandMode::Expanded(target);
        }
    }

    pub fn select_page(&mut self, page: ExpandedPage) {
        self.todo.cancel_input();
        self.calendar.cancel_input();
        self.active_page = page;
        self.mode = IslandMode::Expanded(page);
    }

    pub fn toggle_lyrics(&mut self) {
        match self.mode {
            IslandMode::Lyrics => {
                self.mode = IslandMode::Expanded(ExpandedPage::Music);
            }
            _ => {
                self.active_page = ExpandedPage::Music;
                self.mode = IslandMode::Lyrics;
            }
        }
    }

    pub fn set_power_status(&mut self, is_charging: bool, is_energy_saver: bool, percent: u32) {
        let prev_charging = self.is_charging;
        self.is_charging = is_charging;
        self.is_energy_saver = is_energy_saver;
        self.battery_percent = percent;

        if is_charging && !prev_charging {
            self.show_notification(NotificationKind::Charging { percent }, 3000);
        } else if !is_charging && prev_charging {
            self.show_notification(NotificationKind::ChargerDisconnected { percent }, 3000);
        }
    }

    pub fn set_charging(&mut self, is_charging: bool, percent: u32) {
        self.set_power_status(is_charging, self.is_energy_saver, percent);
    }

    pub fn set_brightness(&mut self, level: u32) {
        self.brightness_level = level;
        self.show_notification(NotificationKind::Brightness { level }, 2400);
    }

    pub fn show_banner(&mut self, app_name: String, title: String, message: String) {
        self.show_notification(NotificationKind::Banner { app_name, title, message }, 4000);
    }

    pub fn set_headphones(&mut self, name: String, battery: u32, connected: bool) {
        self.show_notification(NotificationKind::Headphones { name, battery, connected }, 3400);
    }

    pub fn set_volume(&mut self, level: u32, muted: bool) {
        self.volume_level = level as f32;
        self.is_muted = muted;
        self.show_notification(NotificationKind::Volume { level, muted }, 2400);
    }

    pub fn set_caps_lock(&mut self, enabled: bool) {
        self.caps_lock = enabled;
        self.show_notification(NotificationKind::CapsLock { enabled }, 2000);
    }

    pub fn show_privacy_alert(&mut self, is_camera: bool, app_name: String) {
        self.show_notification(NotificationKind::PrivacyAlert { is_camera, app_name }, 3200);
    }

    pub fn show_timer_completed(&mut self, message: String) {
        self.show_notification(NotificationKind::TimerCompleted { message }, 5000);
    }

    #[allow(dead_code)]
    pub fn show_color_picked(&mut self, hex: String, rgb: String) {
        self.show_notification(NotificationKind::ColorPicked { hex, rgb }, 3500);
    }

    pub fn show_notification(&mut self, notif: NotificationKind, duration_ms: u64) {
        if !matches!(self.mode, IslandMode::Notification(_)) {
            self.previous_mode = self.mode.clone();
        }
        self.mode = IslandMode::Notification(notif);
        self.timeout_at = Some(Instant::now() + std::time::Duration::from_millis(duration_ms));
    }

    pub fn update_media(&mut self, info: MediaTrackInfo) {
        let was_playing = self.media_info.is_playing;
        let is_now_playing = info.is_playing;
        let had_media = self.media_info.has_media;
        let has_media_now = info.has_media;
        self.media_info = info;

        // If media just started playing and island is in Resting, switch to Closed (or brief notification)
        if (!was_playing || !had_media) && is_now_playing {
            if self.mode == IslandMode::Resting {
                self.mode = IslandMode::Closed;
            }
        } else if !has_media_now && self.mode == IslandMode::Closed {
            self.mode = IslandMode::Resting;
        }
    }

    pub fn check_timeouts(&mut self) -> bool {
        // While user is actively typing or dragging slider, pause unhover collapse
        if self.todo.is_input_active || self.is_dragging_slider {
            return false;
        }

        let now = Instant::now();

        // 1. Check transient notification timeout
        if let Some(deadline) = self.timeout_at {
            if now >= deadline {
                self.timeout_at = None;
                // Return to appropriate background state
                if self.is_hovered {
                    self.mode = IslandMode::Expanded(self.active_page);
                } else if self.media_info.is_playing {
                    self.mode = IslandMode::Closed;
                } else {
                    self.mode = IslandMode::Resting;
                }
                return true;
            }
        }

        // 2. Check unhover collapse deadline (never auto-collapse in Video mode so user can watch without interruption!)
        if let Some(deadline) = self.unhover_deadline {
            if !self.is_hovered && now >= deadline {
                self.unhover_deadline = None;
                if matches!(self.mode, IslandMode::Expanded(_) | IslandMode::Lyrics) {
                    if self.media_info.is_playing {
                        self.mode = IslandMode::Closed;
                    } else {
                        self.mode = IslandMode::Resting;
                    }
                    return true;
                }
            }
        }

        false
    }
}

fn query_battery_status() -> (u32, bool) {
    unsafe {
        let mut sps = windows::Win32::System::Power::SYSTEM_POWER_STATUS::default();
        if windows::Win32::System::Power::GetSystemPowerStatus(&mut sps).is_ok() {
            let percent = if sps.BatteryLifePercent != 255 {
                sps.BatteryLifePercent as u32
            } else {
                100
            };
            let is_charging = sps.ACLineStatus == 1;
            (percent, is_charging)
        } else {
            (100, false)
        }
    }
}
