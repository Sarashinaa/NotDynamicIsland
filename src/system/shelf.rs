//! Zero-Copy Shelf (Files & Text Snippets)
//! Supports files via shell icons & CF_HDROP, plus text snippets via custom 32x32 icon & CF_UNICODETEXT.
//! Supports real OLE Drag-and-Drop and instant copy to clipboard into WhatsApp, Telegram, Discord, Browser, or Explorer!

use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use windows::core::{implement, HRESULT, PCWSTR, Result};
use windows::Win32::Foundation::{
    BOOL, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, DV_E_FORMATETC,
    E_INVALIDARG, E_NOTIMPL, HWND, POINTL, S_FALSE,
};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, SelectObject, BITMAP,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HDC,
};
use windows::Win32::System::Com::{
    DVASPECT_CONTENT, IAdviseSink, IDataObject, IEnumFORMATETC, IEnumSTATDATA, FORMATETC, STGMEDIUM,
    STGMEDIUM_0, TYMED_HGLOBAL,
};
use windows::Win32::System::Ole::{
    DoDragDrop, IDropSource, IDropTarget, IDropTarget_Impl, ReleaseStgMedium, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE,
};

const DATA_S_SAMEFORMATETC: HRESULT = HRESULT(0x00040130);
use windows::core::w;
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::UI::Shell::{
    SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo};

static CF_TEXT_PLAIN: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
static CF_TEXT_PLAIN_UTF8: std::sync::OnceLock<u16> = std::sync::OnceLock::new();

pub fn get_cf_text_plain() -> u16 {
    *CF_TEXT_PLAIN.get_or_init(|| unsafe {
        RegisterClipboardFormatW(w!("text/plain")) as u16
    })
}

pub fn get_cf_text_plain_utf8() -> u16 {
    *CF_TEXT_PLAIN_UTF8.get_or_init(|| unsafe {
        RegisterClipboardFormatW(w!("text/plain;charset=utf-8")) as u16
    })
}

fn get_or_create_temp_text_file(text: &str) -> PathBuf {
    let temp_dir = std::env::temp_dir();
    let file_path = temp_dir.join("shelf_text_snippet.txt");
    let _ = fs::write(&file_path, text);
    file_path
}

/// Flag to suppress clipboard loops when NotDynamicIsland copies text/file itself
pub static IS_INTERNAL_CLIPBOARD_COPY: AtomicBool = AtomicBool::new(false);

#[implement(IDropSource)]
struct ShelfDropSource;

impl windows::Win32::System::Ole::IDropSource_Impl for ShelfDropSource_Impl {
    fn QueryContinueDrag(
        &self,
        fescapepressed: BOOL,
        grfkeystate: windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS,
    ) -> HRESULT {
        // Cancel if Escape pressed or Right mouse button clicked (MK_RBUTTON = 2)
        if fescapepressed.as_bool() || (grfkeystate.0 & 0x0002) != 0 {
            return DRAGDROP_S_CANCEL;
        }
        // Left mouse button released (MK_LBUTTON = 1) -> complete the drop!
        if (grfkeystate.0 & 0x0001) == 0 {
            return DRAGDROP_S_DROP;
        }
        HRESULT(0) // S_OK
    }

    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

#[implement(IDataObject)]
struct ShelfDataObject {
    kind: ShelfItemKind,
}

impl windows::Win32::System::Com::IDataObject_Impl for ShelfDataObject_Impl {
    fn GetData(&self, pformatetc_in: *const FORMATETC) -> Result<STGMEDIUM> {
        let fmt = unsafe {
            if pformatetc_in.is_null() {
                match &self.kind {
                    ShelfItemKind::File(_) => 15, // CF_HDROP
                    ShelfItemKind::Text(_) => 13, // CF_UNICODETEXT
                }
            } else {
                (*pformatetc_in).cfFormat
            }
        };

        match &self.kind {
            ShelfItemKind::File(path) => {
                if fmt != 15 {
                    return Err(windows::core::Error::from_hresult(DV_E_FORMATETC));
                }

                let wide_path: Vec<u16> = path
                    .as_os_str()
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .chain(std::iter::once(0))
                    .collect();

                let dropfiles_size = std::mem::size_of::<windows::Win32::UI::Shell::DROPFILES>();
                let total_size = dropfiles_size + wide_path.len() * 2;

                unsafe {
                    let h_global = windows::Win32::System::Memory::GlobalAlloc(
                        windows::Win32::System::Memory::GHND,
                        total_size,
                    )?;
                    let ptr = windows::Win32::System::Memory::GlobalLock(h_global) as *mut u8;
                    if ptr.is_null() {
                        return Err(windows::core::Error::from_hresult(E_NOTIMPL));
                    }

                    let df = ptr as *mut windows::Win32::UI::Shell::DROPFILES;
                    (*df).pFiles = dropfiles_size as u32;
                    (*df).fWide = BOOL(1);

                    let files_ptr = ptr.add(dropfiles_size) as *mut u16;
                    std::ptr::copy_nonoverlapping(wide_path.as_ptr(), files_ptr, wide_path.len());

                    let _ = windows::Win32::System::Memory::GlobalUnlock(h_global);

                    Ok(STGMEDIUM {
                        tymed: TYMED_HGLOBAL.0 as u32,
                        u: STGMEDIUM_0 {
                            hGlobal: windows::Win32::Foundation::HGLOBAL(h_global.0),
                        },
                        pUnkForRelease: std::mem::ManuallyDrop::new(None),
                    })
                }
            }
            ShelfItemKind::Text(text) => {
                // fmt 13 is CF_UNICODETEXT
                if fmt == 13 {
                    let wide_text: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                    let total_size = wide_text.len() * 2;
                    unsafe {
                        let h_global = windows::Win32::System::Memory::GlobalAlloc(
                            windows::Win32::System::Memory::GHND,
                            total_size,
                        )?;
                        let ptr = windows::Win32::System::Memory::GlobalLock(h_global) as *mut u16;
                        if ptr.is_null() {
                            return Err(windows::core::Error::from_hresult(E_NOTIMPL));
                        }
                        std::ptr::copy_nonoverlapping(wide_text.as_ptr(), ptr, wide_text.len());
                        let _ = windows::Win32::System::Memory::GlobalUnlock(h_global);

                        Ok(STGMEDIUM {
                            tymed: TYMED_HGLOBAL.0 as u32,
                            u: STGMEDIUM_0 {
                                hGlobal: windows::Win32::Foundation::HGLOBAL(h_global.0),
                            },
                            pUnkForRelease: std::mem::ManuallyDrop::new(None),
                        })
                    }
                } else if fmt == 1 || fmt == 7 {
                    // fmt 1 is CF_TEXT, fmt 7 is CF_OEMTEXT
                    let mut ansi_bytes: Vec<u8> = text.as_bytes().to_vec();
                    ansi_bytes.push(0);
                    unsafe {
                        let h_global = windows::Win32::System::Memory::GlobalAlloc(
                            windows::Win32::System::Memory::GHND,
                            ansi_bytes.len(),
                        )?;
                        let ptr = windows::Win32::System::Memory::GlobalLock(h_global) as *mut u8;
                        if ptr.is_null() {
                            return Err(windows::core::Error::from_hresult(E_NOTIMPL));
                        }
                        std::ptr::copy_nonoverlapping(ansi_bytes.as_ptr(), ptr, ansi_bytes.len());
                        let _ = windows::Win32::System::Memory::GlobalUnlock(h_global);

                        Ok(STGMEDIUM {
                            tymed: TYMED_HGLOBAL.0 as u32,
                            u: STGMEDIUM_0 {
                                hGlobal: windows::Win32::Foundation::HGLOBAL(h_global.0),
                            },
                            pUnkForRelease: std::mem::ManuallyDrop::new(None),
                        })
                    }
                } else if fmt == 16 {
                    // fmt 16 is CF_LOCALE
                    unsafe {
                        let h_global = windows::Win32::System::Memory::GlobalAlloc(
                            windows::Win32::System::Memory::GHND,
                            4,
                        )?;
                        let ptr = windows::Win32::System::Memory::GlobalLock(h_global) as *mut u32;
                        if !ptr.is_null() {
                            *ptr = 0x0400; // LOCALE_USER_DEFAULT
                            let _ = windows::Win32::System::Memory::GlobalUnlock(h_global);
                            Ok(STGMEDIUM {
                                tymed: TYMED_HGLOBAL.0 as u32,
                                u: STGMEDIUM_0 {
                                    hGlobal: windows::Win32::Foundation::HGLOBAL(h_global.0),
                                },
                                pUnkForRelease: std::mem::ManuallyDrop::new(None),
                            })
                        } else {
                            Err(windows::core::Error::from_hresult(E_NOTIMPL))
                        }
                    }
                } else if fmt == 15 {
                    // CF_HDROP: Allows dropping text card into File Explorer, Desktop, or file drop zones!
                    let temp_path = get_or_create_temp_text_file(text);
                    let wide_path: Vec<u16> = temp_path
                        .as_os_str()
                        .encode_wide()
                        .chain(std::iter::once(0))
                        .chain(std::iter::once(0))
                        .collect();

                    let dropfiles_size = std::mem::size_of::<windows::Win32::UI::Shell::DROPFILES>();
                    let total_size = dropfiles_size + wide_path.len() * 2;

                    unsafe {
                        let h_global = windows::Win32::System::Memory::GlobalAlloc(
                            windows::Win32::System::Memory::GHND,
                            total_size,
                        )?;
                        let ptr = windows::Win32::System::Memory::GlobalLock(h_global) as *mut u8;
                        if ptr.is_null() {
                            return Err(windows::core::Error::from_hresult(E_NOTIMPL));
                        }

                        let df = ptr as *mut windows::Win32::UI::Shell::DROPFILES;
                        (*df).pFiles = dropfiles_size as u32;
                        (*df).fWide = BOOL(1);

                        let files_ptr = ptr.add(dropfiles_size) as *mut u16;
                        std::ptr::copy_nonoverlapping(wide_path.as_ptr(), files_ptr, wide_path.len());

                        let _ = windows::Win32::System::Memory::GlobalUnlock(h_global);

                        Ok(STGMEDIUM {
                            tymed: TYMED_HGLOBAL.0 as u32,
                            u: STGMEDIUM_0 {
                                hGlobal: windows::Win32::Foundation::HGLOBAL(h_global.0),
                            },
                            pUnkForRelease: std::mem::ManuallyDrop::new(None),
                        })
                    }
                } else if fmt == get_cf_text_plain() || fmt == get_cf_text_plain_utf8() {
                    let mut utf8_bytes = text.as_bytes().to_vec();
                    utf8_bytes.push(0);
                    unsafe {
                        let h_global = windows::Win32::System::Memory::GlobalAlloc(
                            windows::Win32::System::Memory::GHND,
                            utf8_bytes.len(),
                        )?;
                        let ptr = windows::Win32::System::Memory::GlobalLock(h_global) as *mut u8;
                        if ptr.is_null() {
                            return Err(windows::core::Error::from_hresult(E_NOTIMPL));
                        }
                        std::ptr::copy_nonoverlapping(utf8_bytes.as_ptr(), ptr, utf8_bytes.len());
                        let _ = windows::Win32::System::Memory::GlobalUnlock(h_global);

                        Ok(STGMEDIUM {
                            tymed: TYMED_HGLOBAL.0 as u32,
                            u: STGMEDIUM_0 {
                                hGlobal: windows::Win32::Foundation::HGLOBAL(h_global.0),
                            },
                            pUnkForRelease: std::mem::ManuallyDrop::new(None),
                        })
                    }
                } else {
                    Err(windows::core::Error::from_hresult(DV_E_FORMATETC))
                }
            }
        }
    }

    fn GetDataHere(&self, _pformatetc: *const FORMATETC, _pmedium: *mut STGMEDIUM) -> Result<()> {
        Err(windows::core::Error::from_hresult(E_NOTIMPL))
    }

    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        if pformatetc.is_null() {
            return DV_E_FORMATETC;
        }
        let (fmt, tymed) = unsafe { ((*pformatetc).cfFormat, (*pformatetc).tymed) };
        if (tymed & TYMED_HGLOBAL.0 as u32) == 0 && tymed != 0 {
            return windows::Win32::Foundation::DV_E_TYMED;
        }
        match &self.kind {
            ShelfItemKind::File(_) => {
                if fmt == 15 {
                    HRESULT(0) // S_OK
                } else {
                    DV_E_FORMATETC
                }
            }
            ShelfItemKind::Text(_) => {
                if fmt == 13 || fmt == 1 || fmt == 7 || fmt == 16 || fmt == 15 || fmt == get_cf_text_plain() || fmt == get_cf_text_plain_utf8() {
                    HRESULT(0) // S_OK
                } else {
                    DV_E_FORMATETC
                }
            }
        }
    }

    fn GetCanonicalFormatEtc(
        &self,
        pformatetc_in: *const FORMATETC,
        pformatetc_out: *mut FORMATETC,
    ) -> HRESULT {
        if pformatetc_in.is_null() || pformatetc_out.is_null() {
            return E_INVALIDARG;
        }
        unsafe {
            *pformatetc_out = *pformatetc_in;
            (*pformatetc_out).ptd = std::ptr::null_mut();
        }
        DATA_S_SAMEFORMATETC
    }

    fn SetData(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *const STGMEDIUM,
        _frelease: BOOL,
    ) -> Result<()> {
        Err(windows::core::Error::from_hresult(E_NOTIMPL))
    }

    fn EnumFormatEtc(&self, dwdirection: u32) -> Result<IEnumFORMATETC> {
        // DATADIR_GET = 1
        if dwdirection == 1 {
            let formats = match &self.kind {
                ShelfItemKind::File(_) => vec![15u16],
                ShelfItemKind::Text(_) => vec![13u16, 1u16, get_cf_text_plain(), get_cf_text_plain_utf8(), 15u16, 16u16, 7u16],
            };
            Ok(ShelfEnumFormatEtc {
                formats,
                index: AtomicUsize::new(0),
            }
            .into())
        } else {
            Err(windows::core::Error::from_hresult(E_NOTIMPL))
        }
    }

    fn DAdvise(
        &self,
        _pformatetc: *const FORMATETC,
        _advf: u32,
        _padvsink: Option<&IAdviseSink>,
    ) -> Result<u32> {
        Err(windows::core::Error::from_hresult(E_NOTIMPL))
    }

    fn DUnadvise(&self, _dwconnection: u32) -> Result<()> {
        Err(windows::core::Error::from_hresult(E_NOTIMPL))
    }

    fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
        Err(windows::core::Error::from_hresult(E_NOTIMPL))
    }
}

#[implement(IEnumFORMATETC)]
struct ShelfEnumFormatEtc {
    formats: Vec<u16>,
    index: AtomicUsize,
}

impl windows::Win32::System::Com::IEnumFORMATETC_Impl for ShelfEnumFormatEtc_Impl {
    fn Next(&self, celt: u32, rgelt: *mut FORMATETC, pceltfetched: *mut u32) -> HRESULT {
        if rgelt.is_null() {
            return E_INVALIDARG;
        }
        let mut fetched = 0;
        let mut idx = self.index.load(Ordering::SeqCst);
        while fetched < celt && idx < self.formats.len() {
            unsafe {
                *rgelt.add(fetched as usize) = FORMATETC {
                    cfFormat: self.formats[idx],
                    ptd: std::ptr::null_mut(),
                    dwAspect: DVASPECT_CONTENT.0 as u32,
                    lindex: -1,
                    tymed: TYMED_HGLOBAL.0 as u32,
                };
            }
            fetched += 1;
            idx += 1;
        }
        self.index.store(idx, Ordering::SeqCst);
        if !pceltfetched.is_null() {
            unsafe {
                *pceltfetched = fetched;
            }
        }
        if fetched == celt {
            HRESULT(0)
        } else {
            S_FALSE
        }
    }

    fn Skip(&self, celt: u32) -> Result<()> {
        let cur = self.index.load(Ordering::SeqCst);
        let next = (cur + celt as usize).min(self.formats.len());
        self.index.store(next, Ordering::SeqCst);
        if next == cur + celt as usize {
            Ok(())
        } else {
            Err(windows::core::Error::from_hresult(S_FALSE))
        }
    }

    fn Reset(&self) -> Result<()> {
        self.index.store(0, Ordering::SeqCst);
        Ok(())
    }

    fn Clone(&self) -> Result<IEnumFORMATETC> {
        Ok(ShelfEnumFormatEtc {
            formats: self.formats.clone(),
            index: AtomicUsize::new(self.index.load(Ordering::SeqCst)),
        }
        .into())
    }
}

/// Initiates real OLE Drag-and-Drop for a shelf file or text item.
/// Directly drag from shelf into WhatsApp, Telegram, Discord, Browser, or File Explorer!
pub fn start_drag_drop(item: &ShelfItem) {
    // If it's a text item, also copy to clipboard so destination apps have it available immediately!
    if let ShelfItemKind::Text(ref text) = item.kind {
        copy_text_to_clipboard(text);
    } else if let ShelfItemKind::File(ref path) = item.kind {
        copy_file_to_clipboard_hdrop(path);
    }

    let data_obj: IDataObject = ShelfDataObject {
        kind: item.kind.clone(),
    }
    .into();
    let drop_src: IDropSource = ShelfDropSource.into();
    let mut effect = DROPEFFECT(0);
    unsafe {
        let _ = DoDragDrop(
            &data_obj,
            &drop_src,
            DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK,
            &mut effect,
        );
    }
}

#[implement(IDropTarget)]
pub struct IslandDropTarget {
    hwnd: HWND,
}

impl IslandDropTarget {
    pub fn new(hwnd: HWND) -> Self {
        Self { hwnd }
    }
}

impl IDropTarget_Impl for IslandDropTarget_Impl {
    fn DragEnter(
        &self,
        pdataobj: Option<&IDataObject>,
        _grfkeystate: windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> Result<()> {
        if pdweffect.is_null() {
            return Err(windows::core::Error::from_hresult(E_INVALIDARG));
        }
        unsafe {
            *pdweffect = if pdataobj.is_some() {
                DROPEFFECT_COPY
            } else {
                windows::Win32::System::Ole::DROPEFFECT_NONE
            };
        }
        Ok(())
    }

    fn DragOver(
        &self,
        _grfkeystate: windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> Result<()> {
        if !pdweffect.is_null() {
            unsafe {
                *pdweffect = DROPEFFECT_COPY;
            }
        }
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        Ok(())
    }

    fn Drop(
        &self,
        pdataobj: Option<&IDataObject>,
        _grfkeystate: windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> Result<()> {
        if pdweffect.is_null() {
            return Err(windows::core::Error::from_hresult(E_INVALIDARG));
        }

        let mut dropped = false;

        if let Some(data) = pdataobj {
            // 1. Try CF_HDROP (Files)
            let fmt_hdrop = FORMATETC {
                cfFormat: 15,
                ptd: std::ptr::null_mut(),
                dwAspect: DVASPECT_CONTENT.0 as u32,
                lindex: -1,
                tymed: TYMED_HGLOBAL.0 as u32,
            };
            if let Ok(stg) = unsafe { data.GetData(&fmt_hdrop) } {
                let hglobal = unsafe { stg.u.hGlobal };
                if stg.tymed == TYMED_HGLOBAL.0 as u32 && !hglobal.is_invalid() {
                    let hdrop = windows::Win32::UI::Shell::HDROP(hglobal.0);
                    let count = unsafe { windows::Win32::UI::Shell::DragQueryFileW(hdrop, 0xFFFFFFFF, None) };
                    let mut files = Vec::new();
                    for i in 0..count {
                        let mut buf = vec![0u16; 1024];
                        let len = unsafe { windows::Win32::UI::Shell::DragQueryFileW(hdrop, i, Some(&mut buf)) };
                        if len > 0 {
                            let path_str = String::from_utf16_lossy(&buf[..len as usize]);
                            files.push(PathBuf::from(path_str));
                        }
                    }
                    if !files.is_empty() {
                        unsafe {
                            ReleaseStgMedium(&stg as *const _ as *mut _);
                        }
                        if let Ok(mut lock) = crate::APP.lock() {
                            if let Some(app) = lock.as_mut() {
                                for path in files {
                                    if let Some(item) = ShelfItem::from_path(path) {
                                        app.island_state.shelf.add(item);
                                    }
                                }
                                app.island_state.select_page(crate::ui::state::ExpandedPage::Shelf);
                                crate::wake_animation(self.hwnd, app);
                                dropped = true;
                            }
                        }
                    }
                }
            }

            // 2. If not files, try CF_UNICODETEXT (Unicode Text)
            if !dropped {
                let fmt_text = FORMATETC {
                    cfFormat: 13,
                    ptd: std::ptr::null_mut(),
                    dwAspect: DVASPECT_CONTENT.0 as u32,
                    lindex: -1,
                    tymed: TYMED_HGLOBAL.0 as u32,
                };
                if let Ok(stg) = unsafe { data.GetData(&fmt_text) } {
                    let hglobal = unsafe { stg.u.hGlobal };
                    if stg.tymed == TYMED_HGLOBAL.0 as u32 && !hglobal.is_invalid() {
                        unsafe {
                            let ptr = windows::Win32::System::Memory::GlobalLock(hglobal) as *const u16;
                            if !ptr.is_null() {
                                let mut len = 0;
                                while *ptr.add(len) != 0 {
                                    len += 1;
                                }
                                let slice = std::slice::from_raw_parts(ptr, len);
                                let text = String::from_utf16_lossy(slice).trim().to_string();
                                let _ = windows::Win32::System::Memory::GlobalUnlock(hglobal);
                                ReleaseStgMedium(&stg as *const _ as *mut _);

                                if !text.is_empty() {
                                    if let Ok(mut lock) = crate::APP.lock() {
                                        if let Some(app) = lock.as_mut() {
                                            let item = ShelfItem::from_text(text);
                                            let item_name = item.name.clone();
                                            app.island_state.shelf.add(item);
                                            app.island_state.select_page(crate::ui::state::ExpandedPage::Shelf);
                                            app.island_state.show_banner(
                                                "Shelf".to_string(),
                                                "Teks Ditambahkan".to_string(),
                                                item_name,
                                            );
                                            crate::wake_animation(self.hwnd, app);
                                            dropped = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 3. Try registered text/plain formats (Chromium / Firefox / Electron apps)
            if !dropped {
                for plain_fmt in [get_cf_text_plain(), get_cf_text_plain_utf8()] {
                    let fmt_plain = FORMATETC {
                        cfFormat: plain_fmt,
                        ptd: std::ptr::null_mut(),
                        dwAspect: DVASPECT_CONTENT.0 as u32,
                        lindex: -1,
                        tymed: TYMED_HGLOBAL.0 as u32,
                    };
                    if let Ok(stg) = unsafe { data.GetData(&fmt_plain) } {
                        let hglobal = unsafe { stg.u.hGlobal };
                        if stg.tymed == TYMED_HGLOBAL.0 as u32 && !hglobal.is_invalid() {
                            unsafe {
                                let ptr = windows::Win32::System::Memory::GlobalLock(hglobal) as *mut u8;
                                if !ptr.is_null() {
                                    let mut len = 0;
                                    while *ptr.add(len) != 0 {
                                        len += 1;
                                    }
                                    let slice = std::slice::from_raw_parts(ptr, len);
                                    let text = String::from_utf8_lossy(slice).trim().to_string();
                                    let _ = windows::Win32::System::Memory::GlobalUnlock(stg.u.hGlobal);
                                    ReleaseStgMedium(&stg as *const _ as *mut _);

                                    if !text.is_empty() {
                                        if let Ok(mut lock) = crate::APP.lock() {
                                            if let Some(app) = lock.as_mut() {
                                                let item = ShelfItem::from_text(text);
                                                let item_name = item.name.clone();
                                                app.island_state.shelf.add(item);
                                                app.island_state.select_page(crate::ui::state::ExpandedPage::Shelf);
                                                app.island_state.show_banner(
                                                    "Shelf".to_string(),
                                                    "Teks Ditambahkan".to_string(),
                                                    item_name,
                                                );
                                                crate::wake_animation(self.hwnd, app);
                                                dropped = true;
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 4. Fallback to CF_TEXT (ANSI Text)
            if !dropped {
                let fmt_ansi = FORMATETC {
                    cfFormat: 1,
                    ptd: std::ptr::null_mut(),
                    dwAspect: DVASPECT_CONTENT.0 as u32,
                    lindex: -1,
                    tymed: TYMED_HGLOBAL.0 as u32,
                };
                if let Ok(stg) = unsafe { data.GetData(&fmt_ansi) } {
                    let hglobal = unsafe { stg.u.hGlobal };
                    if stg.tymed == TYMED_HGLOBAL.0 as u32 && !hglobal.is_invalid() {
                        unsafe {
                            let ptr = windows::Win32::System::Memory::GlobalLock(hglobal) as *mut u8;
                            if !ptr.is_null() {
                                let mut len = 0;
                                while *ptr.add(len) != 0 {
                                    len += 1;
                                }
                                let slice = std::slice::from_raw_parts(ptr, len);
                                let text = String::from_utf8_lossy(slice).trim().to_string();
                                let _ = windows::Win32::System::Memory::GlobalUnlock(stg.u.hGlobal);
                                ReleaseStgMedium(&stg as *const _ as *mut _);

                                if !text.is_empty() {
                                    if let Ok(mut lock) = crate::APP.lock() {
                                        if let Some(app) = lock.as_mut() {
                                            let item = ShelfItem::from_text(text);
                                            let item_name = item.name.clone();
                                            app.island_state.shelf.add(item);
                                            app.island_state.select_page(crate::ui::state::ExpandedPage::Shelf);
                                            app.island_state.show_banner(
                                                "Shelf".to_string(),
                                                "Teks Ditambahkan".to_string(),
                                                item_name,
                                            );
                                            crate::wake_animation(self.hwnd, app);
                                            dropped = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        unsafe {
            *pdweffect = if dropped {
                DROPEFFECT_COPY
            } else {
                windows::Win32::System::Ole::DROPEFFECT_NONE
            };
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ShelfItemKind {
    File(PathBuf),
    Text(String),
}

#[derive(Clone, Debug)]
pub struct ShelfItem {
    pub kind: ShelfItemKind,
    pub name: String,
    pub size_formatted: String,
    pub icon_pixels: Vec<u32>, // 32x32 BGRA
    pub icon_width: usize,
    pub icon_height: usize,
}

// Backwards compatibility alias
pub type ShelfFile = ShelfItem;

impl ShelfItem {
    pub fn from_path(path: PathBuf) -> Option<Self> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        let metadata = fs::metadata(&path).ok();
        let size_bytes = metadata.map(|m| m.len()).unwrap_or(0);
        let size_formatted = format_file_size(size_bytes);

        // Extract shell icon via SHGetFileInfoW
        let (icon_pixels, icon_width, icon_height) = extract_icon_and_destroy(&path);

        Some(Self {
            kind: ShelfItemKind::File(path),
            name,
            size_formatted,
            icon_pixels,
            icon_width,
            icon_height,
        })
    }

    pub fn from_text(text: String) -> Self {
        let trimmed = text.trim();
        let first_line = trimmed.lines().next().unwrap_or("Text");
        let name = if first_line.chars().count() > 22 {
            let truncated: String = first_line.chars().take(20).collect();
            format!("{truncated}...")
        } else if first_line.is_empty() {
            "Text snippet".to_string()
        } else {
            first_line.to_string()
        };

        let char_count = trimmed.chars().count();
        let size_formatted = if char_count == 1 {
            "1 char".to_string()
        } else {
            format!("{} chars", char_count)
        };

        let (icon_pixels, icon_width, icon_height) = create_text_icon();

        Self {
            kind: ShelfItemKind::Text(trimmed.to_string()),
            name,
            size_formatted,
            icon_pixels,
            icon_width,
            icon_height,
        }
    }

    #[allow(dead_code)]
    pub fn path(&self) -> Option<&Path> {
        match &self.kind {
            ShelfItemKind::File(p) => Some(p.as_path()),
            ShelfItemKind::Text(_) => None,
        }
    }

    #[allow(dead_code)]
    pub fn text(&self) -> Option<&str> {
        match &self.kind {
            ShelfItemKind::Text(t) => Some(t.as_str()),
            ShelfItemKind::File(_) => None,
        }
    }
}

/// Generates a crisp, high-DPI 32x32 document text icon
fn create_text_icon() -> (Vec<u32>, usize, usize) {
    let w = 32;
    let h = 32;
    let mut pixels = vec![0u32; w * h];

    // Colors: BGRA format
    let doc_bg = 0xFF2A364F;       // Dark slate-indigo paper
    let doc_border = 0xFF60A5FA;   // Soft sky blue border
    let fold_color = 0xFF38BDF8;   // Top-right folded corner accent
    let line_white = 0xFFE2E8F0;   // Bright header line
    let line_dim = 0xFF94A3B8;     // Secondary text line
    let line_sky = 0xFF38BDF8;     // Sky blue third text line

    // Document boundaries: x: 6..26, y: 3..29
    for y in 3..29 {
        for x in 6..26 {
            // Folded top-right triangle cutoff: x >= 21 and y <= 8
            if x >= 21 && y <= (3 + (x - 21)) {
                continue;
            }

            let is_edge = x == 6 || x == 25 || y == 3 || y == 28;
            let col = if is_edge { doc_border } else { doc_bg };
            pixels[y * w + x] = col;
        }
    }

    // Folded corner polygon
    for y in 3..8 {
        for x in 21..26 {
            if (x - 21) >= (y - 3) {
                pixels[y * w + x] = fold_color;
            }
        }
    }

    // Line 1: Header (x: 10..22, y: 11..12)
    for y in 11..=12 {
        for x in 10..22 {
            pixels[y * w + x] = line_white;
        }
    }

    // Line 2: Body 1 (x: 10..22, y: 15..=16)
    for y in 15..=16 {
        for x in 10..22 {
            pixels[y * w + x] = line_dim;
        }
    }

    // Line 3: Body 2 (x: 10..18, y: 19..=20)
    for y in 19..=20 {
        for x in 10..18 {
            pixels[y * w + x] = line_sky;
        }
    }

    // Line 4: Body 3 (x: 10..15, y: 23..=24)
    for y in 23..=24 {
        for x in 10..15 {
            pixels[y * w + x] = line_dim;
        }
    }

    (pixels, w, h)
}

fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn extract_icon_and_destroy(path: &Path) -> (Vec<u32>, usize, usize) {
    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();

    unsafe {
        let mut sfi = SHFILEINFOW::default();
        let res = SHGetFileInfoW(
            PCWSTR(wide_path.as_ptr()),
            windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut sfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );

        if res == 0 || sfi.hIcon.is_invalid() {
            return (Vec::new(), 0, 0);
        }

        let mut icon_info = windows::Win32::UI::WindowsAndMessaging::ICONINFO::default();
        if GetIconInfo(sfi.hIcon, &mut icon_info).is_err() {
            let _ = DestroyIcon(sfi.hIcon);
            return (Vec::new(), 0, 0);
        }

        let hdc = CreateCompatibleDC(HDC::default());
        let mut bm = BITMAP::default();
        GetObjectW(
            icon_info.hbmColor,
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut _),
        );

        let w = bm.bmWidth as usize;
        let h = bm.bmHeight as usize;
        let mut pixels = vec![0u32; w * h];

        if w > 0 && h > 0 {
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: bm.bmWidth,
                    biHeight: -bm.bmHeight, // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };

            let old_bmp = SelectObject(hdc, icon_info.hbmColor);
            GetDIBits(
                hdc,
                icon_info.hbmColor,
                0,
                bm.bmHeight as u32,
                Some(pixels.as_mut_ptr() as *mut _),
                &mut bmi,
                DIB_RGB_COLORS,
            );
            SelectObject(hdc, old_bmp);

            // Fix up alpha channel if shell icon did not provide pre-multiplied 32-bit alpha
            for px in pixels.iter_mut() {
                let a = (*px >> 24) & 0xFF;
                if a == 0 {
                    // Check if color is non-zero
                    if (*px & 0x00FFFFFF) != 0 {
                        *px |= 0xFF000000;
                    }
                }
            }
        }

        // Strictly clean up GDI and HICON handles!
        let _ = DeleteDC(hdc);
        let _ = DeleteObject(icon_info.hbmColor);
        let _ = DeleteObject(icon_info.hbmMask);
        let _ = DestroyIcon(sfi.hIcon);

        (pixels, w, h)
    }
}

#[derive(Clone, Debug, Default)]
pub struct FileShelf {
    pub items: Vec<ShelfItem>,
}

impl FileShelf {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn add(&mut self, item: ShelfItem) {
        // Remove duplicate if same file path or same text
        self.items.retain(|i| match (&i.kind, &item.kind) {
            (ShelfItemKind::File(p1), ShelfItemKind::File(p2)) => p1 != p2,
            (ShelfItemKind::Text(t1), ShelfItemKind::Text(t2)) => t1 != t2,
            _ => true,
        });

        if self.items.len() >= 5 {
            self.items.remove(0); // Up to 5 items on shelf
        }
        self.items.push(item);
    }

    pub fn remove_at(&mut self, index: usize) {
        if index < self.items.len() {
            self.items.remove(index);
        }
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

/// Copies a file to the Windows Clipboard in CF_HDROP format
/// Allows instant paste (Ctrl+V) into WhatsApp, Discord, Browser, or Explorer!
pub fn copy_file_to_clipboard_hdrop(path: &Path) -> bool {
    let wide_path: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .chain(std::iter::once(0))
        .collect();

    let dropfiles_size = std::mem::size_of::<windows::Win32::UI::Shell::DROPFILES>();
    let total_size = dropfiles_size + wide_path.len() * 2;

    unsafe {
        let h_global = windows::Win32::System::Memory::GlobalAlloc(
            windows::Win32::System::Memory::GHND,
            total_size,
        );
        if let Ok(h_global) = h_global {
            let ptr = windows::Win32::System::Memory::GlobalLock(h_global) as *mut u8;
            if !ptr.is_null() {
                let df = ptr as *mut windows::Win32::UI::Shell::DROPFILES;
                (*df).pFiles = dropfiles_size as u32;
                (*df).fWide = windows::Win32::Foundation::BOOL(1);

                let files_ptr = ptr.add(dropfiles_size) as *mut u16;
                std::ptr::copy_nonoverlapping(wide_path.as_ptr(), files_ptr, wide_path.len());

                let _ = windows::Win32::System::Memory::GlobalUnlock(h_global);

                IS_INTERNAL_CLIPBOARD_COPY.store(true, Ordering::SeqCst);
                if windows::Win32::System::DataExchange::OpenClipboard(
                    windows::Win32::Foundation::HWND::default(),
                )
                .is_ok()
                {
                    let _ = windows::Win32::System::DataExchange::EmptyClipboard();
                    // CF_HDROP format is 15
                    let _ = windows::Win32::System::DataExchange::SetClipboardData(
                        15,
                        windows::Win32::Foundation::HANDLE(h_global.0),
                    );
                    let _ = windows::Win32::System::DataExchange::CloseClipboard();
                    return true;
                }
            }
        }
    }
    false
}

/// Copies text to the Windows Clipboard in CF_UNICODETEXT format
/// Allows instant paste (Ctrl+V) into WhatsApp, Discord, Browser, or any input box!
pub fn copy_text_to_clipboard(text: &str) -> bool {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let total_size = wide.len() * 2;

    unsafe {
        let h_global = windows::Win32::System::Memory::GlobalAlloc(
            windows::Win32::System::Memory::GHND,
            total_size,
        );
        if let Ok(h_global) = h_global {
            let ptr = windows::Win32::System::Memory::GlobalLock(h_global) as *mut u16;
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                let _ = windows::Win32::System::Memory::GlobalUnlock(h_global);

                IS_INTERNAL_CLIPBOARD_COPY.store(true, Ordering::SeqCst);
                if windows::Win32::System::DataExchange::OpenClipboard(
                    windows::Win32::Foundation::HWND::default(),
                )
                .is_ok()
                {
                    let _ = windows::Win32::System::DataExchange::EmptyClipboard();
                    // CF_UNICODETEXT format is 13
                    let _ = windows::Win32::System::DataExchange::SetClipboardData(
                        13,
                        windows::Win32::Foundation::HANDLE(h_global.0),
                    );
                    let _ = windows::Win32::System::DataExchange::CloseClipboard();
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shelf_text_item_creation() {
        let text = "Halo ini testing drag and drop teks di Shelf";
        let item = ShelfItem::from_text(text.to_string());
        assert_eq!(item.kind, ShelfItemKind::Text(text.to_string()));
        assert!(item.name.contains("Halo ini testing"));
        assert_eq!(item.size_formatted, format!("{} chars", text.chars().count()));
        assert_eq!(item.icon_width, 32);
        assert_eq!(item.icon_height, 32);
        assert_eq!(item.icon_pixels.len(), 32 * 32);
    }

    #[test]
    fn test_shelf_add_and_dedup() {
        let mut shelf = FileShelf::new();
        shelf.add(ShelfItem::from_text("Teks A".to_string()));
        shelf.add(ShelfItem::from_text("Teks B".to_string()));
        shelf.add(ShelfItem::from_text("Teks A".to_string())); // Duplicate
        assert_eq!(shelf.items.len(), 2);
        assert_eq!(shelf.items[1].name, "Teks A");
    }

    #[test]
    fn test_shelf_capacity_limit() {
        let mut shelf = FileShelf::new();
        for i in 1..=7 {
            shelf.add(ShelfItem::from_text(format!("Item {}", i)));
        }
        assert_eq!(shelf.items.len(), 5);
        assert_eq!(shelf.items[0].name, "Item 3");
        assert_eq!(shelf.items[4].name, "Item 7");
    }

    #[test]
    fn test_shelf_data_object_get_data() {
        let text = "Drag text payload test";
        let item = ShelfItem::from_text(text.to_string());
        let data_obj: IDataObject = ShelfDataObject { kind: item.kind.clone() }.into();
        let fmt = FORMATETC {
            cfFormat: 13,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0 as u32,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        unsafe {
            let res = data_obj.GetData(&fmt);
            assert!(res.is_ok());
            let stg = res.unwrap();
            assert_eq!(stg.tymed, TYMED_HGLOBAL.0 as u32);
            let ptr = windows::Win32::System::Memory::GlobalLock(stg.u.hGlobal) as *const u16;
            assert!(!ptr.is_null());
            let mut len = 0;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(ptr, len);
            let extracted = String::from_utf16_lossy(slice);
            assert_eq!(extracted, text);
            let _ = windows::Win32::System::Memory::GlobalUnlock(stg.u.hGlobal);
        }

        // Test text/plain format
        let fmt_plain = FORMATETC {
            cfFormat: get_cf_text_plain(),
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0 as u32,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        unsafe {
            let res = data_obj.GetData(&fmt_plain);
            assert!(res.is_ok());
            let stg = res.unwrap();
            assert_eq!(stg.tymed, TYMED_HGLOBAL.0 as u32);
            let ptr = windows::Win32::System::Memory::GlobalLock(stg.u.hGlobal) as *const u8;
            assert!(!ptr.is_null());
            let mut len = 0;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(ptr, len);
            let extracted = String::from_utf8_lossy(slice);
            assert_eq!(extracted, text);
            let _ = windows::Win32::System::Memory::GlobalUnlock(stg.u.hGlobal);
        }

        // Test CF_HDROP format on text card (allows dropping to Desktop/Explorer as a .txt file)
        let fmt_hdrop = FORMATETC {
            cfFormat: 15,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0 as u32,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        unsafe {
            let res = data_obj.GetData(&fmt_hdrop);
            assert!(res.is_ok());
            let stg = res.unwrap();
            assert_eq!(stg.tymed, TYMED_HGLOBAL.0 as u32);
            let hdrop = windows::Win32::UI::Shell::HDROP(stg.u.hGlobal.0);
            let count = windows::Win32::UI::Shell::DragQueryFileW(hdrop, 0xFFFFFFFF, None);
            assert_eq!(count, 1);
            let mut buf = vec![0u16; 512];
            let len = windows::Win32::UI::Shell::DragQueryFileW(hdrop, 0, Some(&mut buf));
            assert!(len > 0);
            let path_str = String::from_utf16_lossy(&buf[..len as usize]);
            assert!(path_str.ends_with("shelf_text_snippet.txt"));
        }
    }
}
