//! Quick Screen Color Picker (Eyedropper Tool)
//! Captures the pixel color under the screen cursor and copies the hex code directly to clipboard.

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{GetDC, GetPixel, ReleaseDC};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GHND};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl ScreenColor {
    pub fn hex(&self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn rgb_string(&self) -> String {
        format!("rgb({}, {}, {})", self.r, self.g, self.b)
    }

    /// Queries the pixel color under the current desktop cursor.
    pub fn pick_at_cursor() -> Option<Self> {
        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_err() {
                return None;
            }

            let hdc = GetDC(HWND(std::ptr::null_mut()));
            if hdc.0.is_null() {
                return None;
            }

            let color_ref = GetPixel(hdc, pt.x, pt.y);
            let _ = ReleaseDC(HWND(std::ptr::null_mut()), hdc);

            // COLORREF is 0x00BBGGRR
            let r = (color_ref.0 & 0xFF) as u8;
            let g = ((color_ref.0 >> 8) & 0xFF) as u8;
            let b = ((color_ref.0 >> 16) & 0xFF) as u8;

            Some(Self { r, g, b })
        }
    }

    /// Copies this color's hex code to the Windows clipboard.
    pub fn copy_to_clipboard(&self) -> bool {
        let hex_str = self.hex();
        copy_string_to_clipboard(&hex_str)
    }
}

pub fn copy_string_to_clipboard(text: &str) -> bool {
    unsafe {
        if OpenClipboard(HWND(std::ptr::null_mut())).is_err() {
            return false;
        }

        let _ = EmptyClipboard();

        let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes_needed = utf16.len() * std::mem::size_of::<u16>();

        if let Ok(handle) = GlobalAlloc(GHND, bytes_needed) {
            let ptr = GlobalLock(handle);
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(utf16.as_ptr() as *const u8, ptr as *mut u8, bytes_needed);
                let _ = GlobalUnlock(handle);
                // CF_UNICODETEXT = 13
                let _ = SetClipboardData(13, windows::Win32::Foundation::HANDLE(handle.0));
            }
        }

        let _ = CloseClipboard();
        true
    }
}
