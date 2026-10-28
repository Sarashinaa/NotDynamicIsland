//! EcoQoS and Working Set Memory Trimming
//! Forces execution on Efficiency Cores and reclaims page cache on idle.

use windows::Win32::System::Threading::{
    GetCurrentProcess, SetProcessInformation, ProcessPowerThrottling,
    PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION, PROCESS_POWER_THROTTLING_STATE,
};

/// Configures EcoQoS: Schedules threads on Efficiency Cores & allows deep sleep C-states
pub fn enable_eco_qos() {
    unsafe {
        let mut state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED
                | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
            StateMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED
                | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        };

        let res = SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            &mut state as *mut _ as *const _,
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        );

        if res.is_ok() {
            println!("[EcoQoS] Efficiency-core power throttling active.");
        }
    }
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
