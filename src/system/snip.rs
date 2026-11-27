//! Quick Screen Snip (Snipping Tool Trigger & Markup Activation)
//! Invokes Windows Screen Clipping protocol `ms-screenclip:` asynchronously and detached,
//! and opens captured screenshot `.png` files directly inside Snipping Tool's Markup & Share editor.
//! Fully prevents any UI thread blocking, COM deadlocks, or window hangs.

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows::core::{w, IUnknown, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IBindCtx, CLSCTX_LOCAL_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{
    ApplicationActivationManager, IApplicationActivationManager, IShellItem, IShellItemArray,
    SHCreateItemFromParsingName, SHCreateShellItemArrayFromShellItem, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const SNIPPING_TOOL_AUMID: PCWSTR = w!("Microsoft.ScreenSketch_8wekyb3d8bbwe!App");

static LAST_TRIGGER_TIME: AtomicU64 = AtomicU64::new(0);

struct ComInitGuard(bool);

impl Drop for ComInitGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

pub fn trigger_screen_snip() {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let last = LAST_TRIGGER_TIME.load(Ordering::Relaxed);
    if now_ms.saturating_sub(last) < 600 {
        // Debounce rapid repeated clicks within 600ms
        return;
    }
    LAST_TRIGGER_TIME.store(now_ms, Ordering::Relaxed);

    std::thread::spawn(move || {
        let sys_root = std::env::var("SystemRoot")
            .or_else(|_| std::env::var("windir"))
            .unwrap_or_else(|_| r"C:\Windows".to_string());
        let explorer_exe = std::path::PathBuf::from(sys_root).join("explorer.exe");
        // Launch detached via %SystemRoot%\explorer.exe to avoid any COM/DDE synchronization or CWD binary planting
        let _ = std::process::Command::new(explorer_exe)
            .arg("ms-screenclip:")
            .spawn();
    });
}

fn find_newest_image_in_dir(dir: &Path) -> Option<(PathBuf, SystemTime)> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(PathBuf, SystemTime)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if !crate::system::shelf::is_image_file(&path) {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let mtime = meta.modified().unwrap_or(UNIX_EPOCH);
        let is_newer = match &best {
            Some((_, best_time)) => mtime > *best_time,
            None => true,
        };
        if is_newer {
            best = Some((path, mtime));
        }
    }
    best
}

fn resolve_latest_screenshot_path_with_folder(
    shelf_hint: Option<PathBuf>,
    screenshots_folder: Option<PathBuf>,
) -> Option<PathBuf> {
    let folder_latest = screenshots_folder.and_then(|dir| find_newest_image_in_dir(&dir));

    if let Some(hint) = shelf_hint {
        let is_valid_hint = hint.is_file() && crate::system::shelf::is_image_file(&hint);
        if is_valid_hint {
            let hint_mtime = std::fs::metadata(&hint)
                .and_then(|m| m.modified())
                .ok();
            let is_temp = crate::system::shelf::is_temp_shelf_capture(&hint);

            if let Some((folder_path, folder_mtime)) = folder_latest {
                if is_temp {
                    // Prefer the real Pictures\Screenshots PNG when it matches or supersedes the temporary Shelf capture
                    let within_temp_window = match hint_mtime {
                        Some(ht) => {
                            folder_mtime >= ht
                                || ht.duration_since(folder_mtime).unwrap_or(Duration::MAX)
                                    <= Duration::from_secs(5)
                        }
                        None => true,
                    };
                    if within_temp_window {
                        return Some(folder_path);
                    }
                    return Some(hint);
                }

                if let Some(ht) = hint_mtime {
                    if folder_mtime > ht {
                        return Some(folder_path);
                    }
                }
            }
            return Some(hint);
        }
    }

    folder_latest.map(|(p, _)| p)
}

/// Resolves the newest screenshot file using the in-memory shelf hint and the system Screenshots folder.
/// Performs filesystem I/O, so invoke only on a background thread outside any UI locks.
pub fn resolve_latest_screenshot_path(shelf_hint: Option<PathBuf>) -> Option<PathBuf> {
    resolve_latest_screenshot_path_with_folder(
        shelf_hint,
        crate::system::shelf::resolve_screenshots_folder(),
    )
}

fn strip_extended_prefix(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

/// Opens an existing image file directly inside Windows Snipping Tool's Markup & Share editor.
/// Must run on a background thread outside any UI lock.
pub fn open_image_in_snipping_tool_markup(path: &Path) -> bool {
    if !path.is_file() || !crate::system::shelf::is_image_file(path) {
        return false;
    }

    let clean_path = strip_extended_prefix(path);
    let wide_path: Vec<u16> = clean_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let _com_guard = ComInitGuard(hr.is_ok());

        if let Ok(shell_item) = SHCreateItemFromParsingName::<PCWSTR, Option<&IBindCtx>, IShellItem>(
            PCWSTR(wide_path.as_ptr()),
            None,
        ) {
            if let Ok(item_array) =
                SHCreateShellItemArrayFromShellItem::<&IShellItem, IShellItemArray>(&shell_item)
            {
                if let Ok(mgr) =
                    CoCreateInstance::<Option<&IUnknown>, IApplicationActivationManager>(
                        &ApplicationActivationManager,
                        None,
                        CLSCTX_LOCAL_SERVER,
                    )
                {
                    if mgr
                        .ActivateForFile(SNIPPING_TOOL_AUMID, &item_array, w!("open"))
                        .is_ok()
                    {
                        return true;
                    }
                }
            }
        }

        // Fallback to default image handler if Store Snipping Tool AUMID is unavailable
        let res = ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide_path.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
        (res.0 as usize) > 32
    }
}

/// Asynchronously resolves the latest screenshot and opens it in Snipping Tool's Markup & Share editor.
pub fn open_latest_screenshot_markup_async(shelf_hint: Option<PathBuf>) {
    std::thread::Builder::new()
        .name("SnipMarkupWorker".to_string())
        .spawn(move || {
            if let Some(path) = resolve_latest_screenshot_path(shelf_hint) {
                if !open_image_in_snipping_tool_markup(&path) {
                    trigger_screen_snip();
                }
            } else {
                trigger_screen_snip();
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_latest_screenshot_path_with_hint() {
        let temp_root = std::env::temp_dir().join(format!(
            "ndi_snip_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_root);
        let _ = std::fs::create_dir_all(&temp_root);

        let old_png = temp_root.join("Screenshot_2026_01.png");
        let new_png = temp_root.join("Screenshot_2026_02.png");
        let non_image = temp_root.join("Notes.txt");

        std::fs::write(&old_png, b"old").unwrap();
        std::thread::sleep(Duration::from_millis(25));
        std::fs::write(&new_png, b"new").unwrap();
        std::thread::sleep(Duration::from_millis(25));
        std::fs::write(&non_image, b"ignore me").unwrap();

        // 1. Directory scanner picks newest image and ignores .txt
        let (found, _) = find_newest_image_in_dir(&temp_root).expect("should find newest image");
        assert_eq!(found, new_png);

        // 2. Newer folder image wins over older shelf hint
        let resolved = resolve_latest_screenshot_path_with_folder(
            Some(old_png.clone()),
            Some(temp_root.clone()),
        );
        assert_eq!(resolved, Some(new_png.clone()));

        // 3. Valid shelf hint works when screenshots folder is unavailable
        let resolved_hint_only = resolve_latest_screenshot_path_with_folder(
            Some(old_png.clone()),
            None,
        );
        assert_eq!(resolved_hint_only, Some(old_png));

        // 4. Missing shelf hint falls back to screenshots folder
        let missing_hint = temp_root.join("missing.png");
        let resolved_missing = resolve_latest_screenshot_path_with_folder(
            Some(missing_hint),
            Some(temp_root.clone()),
        );
        assert_eq!(resolved_missing, Some(new_png));

        let _ = std::fs::remove_dir_all(&temp_root);
    }
}
