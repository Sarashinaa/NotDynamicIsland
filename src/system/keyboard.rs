//! Event-driven Low-Level Keyboard Hook for Caps Lock and Media Key Notifications
//! Zero polling: Hook proc awakened by Windows input subsystem only when keys are pressed.

use std::sync::atomic::{AtomicIsize, Ordering};
use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CAPITAL};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, PostMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK,
    KBDLLHOOKSTRUCT, WH_KEYBOARD_LL, WM_KEYUP,
};

pub const WM_APP_CAPSLOCK_CHANGED: u32 = 0x8004;

static TARGET_HWND: AtomicIsize = AtomicIsize::new(0);
static HOOK_HANDLE: AtomicIsize = AtomicIsize::new(0);

pub struct KeyboardHook {
    active: bool,
}

impl KeyboardHook {
    pub fn new() -> Self {
        Self { active: false }
    }

    pub fn start(&mut self, hwnd: HWND) {
        TARGET_HWND.store(hwnd.0 as isize, Ordering::SeqCst);

        unsafe {
            let hook = SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(keyboard_hook_proc),
                HMODULE::default(),
                0,
            );

            if let Ok(h) = hook {
                HOOK_HANDLE.store(h.0 as isize, Ordering::SeqCst);
                self.active = true;
                println!("[Keyboard] Installed low-level hook for Caps Lock events.");
            }
        }
    }
}

impl Drop for KeyboardHook {
    fn drop(&mut self) {
        if self.active {
            let h = HOOK_HANDLE.swap(0, Ordering::SeqCst);
            if h != 0 {
                unsafe {
                    let _ = UnhookWindowsHookEx(HHOOK(h as *mut _));
                }
            }
        }
    }
}

unsafe extern "system" fn keyboard_hook_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code >= 0 && wparam.0 == WM_KEYUP as usize {
        let kbd = unsafe { *(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if kbd.vkCode == VK_CAPITAL.0 as u32 {
            // Check Caps Lock toggled status
            let state = unsafe { GetKeyState(VK_CAPITAL.0 as i32) };
            let is_on = (state & 0x0001) != 0;

            let target = TARGET_HWND.load(Ordering::SeqCst);
            if target != 0 {
                unsafe {
                    let _ = PostMessageW(
                        HWND(target as *mut _),
                        WM_APP_CAPSLOCK_CHANGED,
                        WPARAM(if is_on { 1 } else { 0 }),
                        LPARAM(0),
                    );
                }
            }
        }
    }

    unsafe { CallNextHookEx(HHOOK::default(), code, wparam, lparam) }
}
