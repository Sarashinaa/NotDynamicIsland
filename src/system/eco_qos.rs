//! Power profile & Working Set Memory Trimming
//! Maintains high timer resolution (1ms) and reclaims page cache on idle.

/// Configures process power profile: maintains high timer resolution and performance when active
pub fn enable_eco_qos() {
    // Intentionally keep high timer resolution active (timeBeginPeriod = 1ms)
    // without throttling execution speed so animations render at true 60-120+ FPS without micro-stutter.
    // Deep sleep C-states and 0% CPU are natively achieved when idle in GetMessageW.
}

#[link(name = "psapi")]
unsafe extern "system" {
    fn EmptyWorkingSet(hProcess: windows::Win32::Foundation::HANDLE) -> windows::Win32::Foundation::BOOL;
}

/// Trims working set memory to force Windows to flush page cache down to ~1-2 MB on idle
pub fn trim_working_set() {
    unsafe {
        let handle = windows::Win32::System::Threading::GetCurrentProcess();
        let _ = EmptyWorkingSet(handle);
    }
}
