//! Quick Screen Snip (Snipping Tool Trigger)
//! Invokes Windows Screen Clipping protocol `ms-screenclip:` asynchronously and detached.
//! Fully prevents any UI thread blocking, COM deadlocks, or window hangs.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static LAST_TRIGGER_TIME: AtomicU64 = AtomicU64::new(0);

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
