//! Multi-Monitor Management with dynamic enumeration and persistent targeting
//! Supports arbitrary number of displays (1, 2, 3, 4+) and Follow Cursor mode

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, Ordering};
use windows::Win32::Foundation::{BOOL, HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW, MonitorFromPoint, DEVMODEW,
    ENUM_CURRENT_SETTINGS, HDC, HMONITOR, MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, PostMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
    HHOOK, MSLLHOOKSTRUCT, WH_MOUSE_LL, WM_MOUSEMOVE,
};

pub const WM_APP_DISPLAY_CURSOR_MOVED: u32 = 0x8016;

static TARGET_HWND: AtomicIsize = AtomicIsize::new(0);
static MOUSE_HOOK_HANDLE: AtomicIsize = AtomicIsize::new(0);
static CURSOR_FOLLOW_ENABLED: AtomicBool = AtomicBool::new(false);
static TRANSITION_PENDING: AtomicBool = AtomicBool::new(false);
static CURRENT_MON_LEFT: AtomicI32 = AtomicI32::new(i32::MIN);
static CURRENT_MON_TOP: AtomicI32 = AtomicI32::new(i32::MIN);
static CURRENT_MON_RIGHT: AtomicI32 = AtomicI32::new(i32::MAX);
static CURRENT_MON_BOTTOM: AtomicI32 = AtomicI32::new(i32::MAX);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayTarget {
    FollowCursor,
    Specific(usize), // 1-based monitor index: 1, 2, 3...
}

#[derive(Clone, Debug, PartialEq)]
pub struct MonitorInfo {
    pub hmonitor: isize,
    pub index: usize,
    pub is_primary: bool,
    pub rect: RECT,
    pub name: String,
}

/// Queries the primary display refresh rate in Hz (e.g. 60, 120, 144, 180, 240). Defaults to 60 if unavailable.
pub fn get_primary_refresh_rate() -> u32 {
    unsafe {
        let mut dm = DEVMODEW::default();
        dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        if EnumDisplaySettingsW(
            None,
            ENUM_CURRENT_SETTINGS,
            &mut dm,
        ).as_bool() && dm.dmDisplayFrequency >= 30 {
            return dm.dmDisplayFrequency;
        }
        60
    }
}

/// Computes optimal animation timer tick interval in milliseconds for the current display.
/// For 180Hz displays: 5ms (~200Hz tick rate).
/// For 144Hz displays: 6ms (~166Hz tick rate).
/// For 120Hz displays: 8ms (125Hz tick rate).
/// For 60Hz displays: 16ms (60Hz tick rate).
pub fn get_optimal_animation_interval_ms() -> u32 {
    let hz = get_primary_refresh_rate();
    (1000 / hz).clamp(1, 16)
}

pub struct DisplayManager {
    pub monitors: Vec<MonitorInfo>,
    pub target: DisplayTarget,
    pub active_index: usize,
    file_path: PathBuf,
}

impl DisplayManager {
    pub fn new() -> Self {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        let exe_file = exe_dir.join("display.json");

        let file_path = if exe_file.exists() {
            exe_file
        } else {
            let app_data = std::env::var("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|_| exe_dir.clone());
            let island_dir = app_data.join("NotDynamicIsland");
            let _ = fs::create_dir_all(&island_dir);
            island_dir.join("display.json")
        };

        let mut dm = Self {
            monitors: Vec::new(),
            target: DisplayTarget::FollowCursor,
            active_index: 1,
            file_path,
        };
        dm.refresh();
        dm.load();
        dm
    }

    pub fn refresh(&mut self) {
        self.monitors = enumerate_monitors();
        if self.monitors.is_empty() {
            // Absolute fallback if no monitors returned
            self.monitors.push(MonitorInfo {
                hmonitor: 0,
                index: 1,
                is_primary: true,
                rect: RECT { left: 0, top: 0, right: 1920, bottom: 1080 },
                name: "Display 1 (Primary)".to_string(),
            });
        }
    }

    pub fn set_target(&mut self, target: DisplayTarget) {
        self.target = target;
        self.save();
    }

    pub fn get_target_rect(&mut self, cursor_pt: Option<POINT>) -> Option<RECT> {
        if self.monitors.is_empty() {
            self.refresh();
        }

        let chosen = match self.target {
            DisplayTarget::Specific(target_idx) => {
                if let Some(mon) = self.monitors.iter().find(|m| m.index == target_idx) {
                    mon
                } else if let Some(primary) = self.monitors.iter().find(|m| m.is_primary) {
                    primary
                } else {
                    &self.monitors[0]
                }
            }
            DisplayTarget::FollowCursor => {
                let mut matched_idx: Option<usize> = None;
                if let Some(pt) = cursor_pt {
                    unsafe {
                        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
                        matched_idx = self.monitors.iter().position(|m| {
                            m.hmonitor == hmon.0 as isize
                                || (pt.x >= m.rect.left
                                    && pt.x < m.rect.right
                                    && pt.y >= m.rect.top
                                    && pt.y < m.rect.bottom)
                        });
                    }
                }
                if matched_idx.is_none() && cursor_pt.is_some() {
                    self.refresh();
                    if let Some(pt) = cursor_pt {
                        unsafe {
                            let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
                            matched_idx = self.monitors.iter().position(|m| {
                                m.hmonitor == hmon.0 as isize
                                    || (pt.x >= m.rect.left
                                        && pt.x < m.rect.right
                                        && pt.y >= m.rect.top
                                        && pt.y < m.rect.bottom)
                            });
                        }
                    }
                }
                if let Some(idx) = matched_idx {
                    &self.monitors[idx]
                } else if let Some(primary) = self.monitors.iter().find(|m| m.is_primary) {
                    primary
                } else {
                    &self.monitors[0]
                }
            }
        };

        self.active_index = chosen.index;
        let r = chosen.rect;
        CURRENT_MON_LEFT.store(r.left, Ordering::SeqCst);
        CURRENT_MON_TOP.store(r.top, Ordering::SeqCst);
        CURRENT_MON_RIGHT.store(r.right, Ordering::SeqCst);
        CURRENT_MON_BOTTOM.store(r.bottom, Ordering::SeqCst);

        Some(r)
    }

    pub fn load(&mut self) {
        if self.file_path.exists() {
            if let Ok(content) = fs::read_to_string(&self.file_path) {
                if let Some(val) = extract_json_target(&content) {
                    self.target = if val == 0 {
                        DisplayTarget::FollowCursor
                    } else {
                        DisplayTarget::Specific(val)
                    };
                }
            }
        }
    }

    pub fn save(&self) {
        if let Some(parent) = self.file_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let code = match self.target {
            DisplayTarget::FollowCursor => 0,
            DisplayTarget::Specific(idx) => idx,
        };
        let out = format!("{{\"target\":{}}}\n", code);
        let _ = fs::write(&self.file_path, out);
    }
}

pub fn clear_transition_pending() {
    TRANSITION_PENDING.store(false, Ordering::SeqCst);
}

pub fn update_display_hook(hwnd: HWND, enable_follow: bool) {
    CURSOR_FOLLOW_ENABLED.store(enable_follow, Ordering::SeqCst);
    TARGET_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
    TRANSITION_PENDING.store(false, Ordering::SeqCst);

    if enable_follow {
        if MOUSE_HOOK_HANDLE.load(Ordering::SeqCst) == 0 {
            unsafe {
                if let Ok(hook) = SetWindowsHookExW(
                    WH_MOUSE_LL,
                    Some(display_mouse_hook_proc),
                    HMODULE::default(),
                    0,
                ) {
                    MOUSE_HOOK_HANDLE.store(hook.0 as isize, Ordering::SeqCst);
                }
            }
        }
    } else {
        let h = MOUSE_HOOK_HANDLE.swap(0, Ordering::SeqCst);
        if h != 0 {
            unsafe {
                let _ = UnhookWindowsHookEx(HHOOK(h as *mut _));
            }
        }
    }
}

unsafe extern "system" fn display_mouse_hook_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code >= 0 && CURSOR_FOLLOW_ENABLED.load(Ordering::Relaxed) {
        if wparam.0 as u32 == WM_MOUSEMOVE {
            let ms = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
            let px = ms.pt.x;
            let py = ms.pt.y;

            let left = CURRENT_MON_LEFT.load(Ordering::Relaxed);
            let top = CURRENT_MON_TOP.load(Ordering::Relaxed);
            let right = CURRENT_MON_RIGHT.load(Ordering::Relaxed);
            let bottom = CURRENT_MON_BOTTOM.load(Ordering::Relaxed);

            // Fast register comparison: only notify when crossing the active monitor rectangle
            if px < left || px >= right || py < top || py >= bottom {
                // Atomic guard: ensure only ONE transition message is in flight until processed
                if !TRANSITION_PENDING.swap(true, Ordering::SeqCst) {
                    let target = TARGET_HWND.load(Ordering::Relaxed);
                    if target != 0 {
                        unsafe {
                            let _ = PostMessageW(
                                HWND(target as *mut _),
                                WM_APP_DISPLAY_CURSOR_MOVED,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                    }
                }
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn enumerate_monitors() -> Vec<MonitorInfo> {
    let mut monitors: Vec<MonitorInfo> = Vec::new();

    unsafe extern "system" fn enum_proc(
        hmonitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        let list = unsafe { &mut *(lparam.0 as *mut Vec<MonitorInfo>) };
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;

        if unsafe { GetMonitorInfoW(hmonitor, &mut mi.monitorInfo as *mut _ as *mut _).as_bool() } {
            let is_primary = (mi.monitorInfo.dwFlags & 1) != 0;
            let r = mi.monitorInfo.rcMonitor;
            let w = r.right - r.left;
            let h = r.bottom - r.top;
            let idx = list.len() + 1;

            let name = if is_primary {
                format!("Display {} (Primary - {}×{})", idx, w, h)
            } else {
                format!("Display {} ({}×{})", idx, w, h)
            };

            list.push(MonitorInfo {
                hmonitor: hmonitor.0 as isize,
                index: idx,
                is_primary,
                rect: r,
                name,
            });
        }
        BOOL(1)
    }

    unsafe {
        let _ = EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(enum_proc),
            LPARAM(&mut monitors as *mut _ as isize),
        );
    }

    monitors
}

fn extract_json_target(content: &str) -> Option<usize> {
    let key = "\"target\":";
    let start = content.find(key)? + key.len();
    let sub = &content[start..];
    let end = sub.find([',', '}', '\n', '\r'])?;
    sub[..end].trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_manager_enumeration_and_naming() {
        let dm = DisplayManager::new();
        assert!(!dm.monitors.is_empty(), "Must detect at least one display");
        for (i, m) in dm.monitors.iter().enumerate() {
            assert_eq!(m.index, i + 1);
            assert!(m.name.starts_with("Display "));
            assert!(m.rect.right > m.rect.left);
            assert!(m.rect.bottom > m.rect.top);
        }
    }

    #[test]
    fn test_display_target_save_and_load_roundtrip() {
        let test_path = std::env::temp_dir().join("test_notdynamicisland_display.json");
        let _ = fs::remove_file(&test_path);

        let dm = DisplayManager {
            monitors: vec![
                MonitorInfo {
                    hmonitor: 100,
                    index: 1,
                    is_primary: true,
                    rect: RECT { left: 0, top: 0, right: 1920, bottom: 1080 },
                    name: "Display 1 (Primary - 1920×1080)".to_string(),
                },
                MonitorInfo {
                    hmonitor: 200,
                    index: 2,
                    is_primary: false,
                    rect: RECT { left: 1920, top: 0, right: 3840, bottom: 1080 },
                    name: "Display 2 (1920×1080)".to_string(),
                },
            ],
            target: DisplayTarget::Specific(2),
            active_index: 2,
            file_path: test_path.clone(),
        };

        dm.save();

        let mut loaded_dm = DisplayManager {
            monitors: dm.monitors.clone(),
            target: DisplayTarget::FollowCursor,
            active_index: 1,
            file_path: test_path.clone(),
        };
        loaded_dm.load();
        assert_eq!(loaded_dm.target, DisplayTarget::Specific(2));

        let _ = fs::remove_file(&test_path);
    }

    #[test]
    fn test_transition_pending_flag() {
        clear_transition_pending();
        assert!(!TRANSITION_PENDING.load(Ordering::SeqCst));
        assert!(!TRANSITION_PENDING.swap(true, Ordering::SeqCst));
        assert!(TRANSITION_PENDING.load(Ordering::SeqCst));
        // Next swap must return true (meaning already in flight)
        assert!(TRANSITION_PENDING.swap(true, Ordering::SeqCst));
        clear_transition_pending();
        assert!(!TRANSITION_PENDING.load(Ordering::SeqCst));
    }

    #[test]
    fn test_follow_cursor_target_rect_matching() {
        let mut dm = DisplayManager {
            monitors: vec![
                MonitorInfo {
                    hmonitor: 100,
                    index: 1,
                    is_primary: true,
                    rect: RECT { left: 0, top: 0, right: 1536, bottom: 960 },
                    name: "Display 1 (Primary - 1536×960)".to_string(),
                },
                MonitorInfo {
                    hmonitor: 200,
                    index: 2,
                    is_primary: false,
                    rect: RECT { left: -1920, top: 113, right: 0, bottom: 1193 },
                    name: "Display 2 (1920×1080)".to_string(),
                },
            ],
            target: DisplayTarget::FollowCursor,
            active_index: 1,
            file_path: PathBuf::from("dummy.json"),
        };

        // Point inside monitor 2
        let pt2 = POINT { x: -500, y: 500 };
        let r2 = dm.get_target_rect(Some(pt2)).unwrap();
        assert_eq!(r2.left, -1920);
        assert_eq!(dm.active_index, 2);

        // Point inside monitor 1
        let pt1 = POINT { x: 500, y: 500 };
        let r1 = dm.get_target_rect(Some(pt1)).unwrap();
        assert_eq!(r1.left, 0);
        assert_eq!(dm.active_index, 1);
    }
}
