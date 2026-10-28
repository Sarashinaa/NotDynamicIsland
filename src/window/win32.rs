//! Win32 Layered ToolWindow with Hit-Testing Passthrough & CPU Double Buffer

use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::ptr::null_mut;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HDC, HBITMAP,
};
use windows::Win32::System::Ole::{RegisterDragDrop, RevokeDragDrop, IDropTarget};
use windows::Win32::UI::Shell::DragAcceptFiles;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetSystemMetrics, RegisterClassExW,
    SetWindowPos, ShowWindow, CS_HREDRAW, CS_VREDRAW, HWND_TOPMOST, SM_CXSCREEN,
    SWP_NOACTIVATE, SW_SHOW, ULW_ALPHA, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

pub const CANVAS_W: usize = 820;
pub const CANVAS_H: usize = 340;
pub const TIMER_ANIMATION_ID: usize = 1;
pub const AC_SRC_OVER: u32 = 0x00;
pub const AC_SRC_ALPHA: u32 = 0x01;

pub struct Win32LayeredWindow {
    pub hwnd: HWND,
    pub hdc_mem: HDC,
    pub hbitmap: HBITMAP,
    pub dib_pixels: *mut u32,
    pub win_x: i32,
    pub win_y: i32,
}

unsafe impl Send for Win32LayeredWindow {}

pub type WndProcFn = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

impl Win32LayeredWindow {
    pub fn create(title: &str, wnd_proc: WndProcFn) -> Result<Self, windows::core::Error> {
        let title_w: Vec<u16> = std::ffi::OsStr::new(title)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let class_name = w!("NotDynamicIsland_Class");

        unsafe {
            let hinstance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?;

            let wnd_class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wnd_proc),
                hInstance: hinstance.into(),
                lpszClassName: class_name,
                ..Default::default()
            };

            RegisterClassExW(&wnd_class);

            let screen_w = GetSystemMetrics(SM_CXSCREEN);
            let win_x = (screen_w - CANVAS_W as i32) / 2;
            let win_y = 0; // Anchored flush to the top of the screen!

            let ex_style = WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW;
            let style = WS_POPUP;

            let hwnd = CreateWindowExW(
                ex_style,
                class_name,
                PCWSTR(title_w.as_ptr()),
                style,
                win_x,
                win_y,
                CANVAS_W as i32,
                CANVAS_H as i32,
                HWND::default(),
                None,
                hinstance,
                None,
            )?;

            // Allow Drag & Drop of files and text directly to the island
            DragAcceptFiles(hwnd, true);
            let drop_target: IDropTarget = crate::system::shelf::IslandDropTarget::new(hwnd).into();
            let _ = RegisterDragDrop(hwnd, &drop_target);

            // Create 32-bit BGRA DIBSection
            let hdc_screen = HDC::default();
            let hdc_mem = CreateCompatibleDC(hdc_screen);

            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: CANVAS_W as i32,
                    biHeight: -(CANVAS_H as i32), // top-down DIB
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };

            let mut dib_ptr: *mut std::ffi::c_void = null_mut();
            let hbitmap = CreateDIBSection(
                hdc_mem,
                &bmi,
                DIB_RGB_COLORS,
                &mut dib_ptr,
                windows::Win32::Foundation::HANDLE::default(),
                0,
            )?;

            SelectObject(hdc_mem, hbitmap);

            // Position and show window
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                win_x,
                win_y,
                CANVAS_W as i32,
                CANVAS_H as i32,
                SWP_NOACTIVATE,
            );
            let _ = ShowWindow(hwnd, SW_SHOW);

            Ok(Self {
                hwnd,
                hdc_mem,
                hbitmap,
                dib_pixels: dib_ptr as *mut u32,
                win_x,
                win_y,
            })
        }
    }

    /// Blits the PixelBuffer to DIBSection and pushes to Desktop Window Manager (DWM)
    pub fn update_layered_surface(&self, buffer: &[u32]) {
        if self.dib_pixels.is_null() || buffer.len() != CANVAS_W * CANVAS_H {
            return;
        }

        unsafe {
            // Copy pixels directly to DIB memory
            std::ptr::copy_nonoverlapping(
                buffer.as_ptr(),
                self.dib_pixels,
                CANVAS_W * CANVAS_H,
            );

            let pt_dst = POINT {
                x: self.win_x,
                y: self.win_y,
            };
            let size = SIZE {
                cx: CANVAS_W as i32,
                cy: CANVAS_H as i32,
            };
            let pt_src = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };

            let _ = windows::Win32::UI::WindowsAndMessaging::UpdateLayeredWindow(
                self.hwnd,
                HDC::default(),
                Some(&pt_dst),
                Some(&size),
                self.hdc_mem,
                Some(&pt_src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }
}

impl Drop for Win32LayeredWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = RevokeDragDrop(self.hwnd);
            let _ = DeleteObject(self.hbitmap);
            let _ = DeleteDC(self.hdc_mem);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}


