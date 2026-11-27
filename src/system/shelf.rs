//! Zero-Copy Shelf (Files & Text Snippets)
//! Supports files via shell icons & CF_HDROP, plus text snippets via custom 32x32 icon & CF_UNICODETEXT.
//! Supports real OLE Drag-and-Drop and instant copy to clipboard into WhatsApp, Telegram, Discord, Browser, or Explorer!

use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows::core::{implement, w, HRESULT, PCWSTR, Result};
use windows::Win32::Foundation::{
    CloseHandle, GlobalFree, BOOL, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS,
    DV_E_FORMATETC, E_INVALIDARG, E_NOTIMPL, HANDLE, HGLOBAL, HWND, LPARAM, POINTL, S_FALSE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, SelectObject, BITMAP,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HDC,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat24bppBGR,
    GUID_WICPixelFormat32bppBGRA, IWICBitmapFrameEncode, IWICBitmapScaler, IWICFormatConverter,
    IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapEncoderNoCache,
    WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadDirectoryChangesW, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY,
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_INFORMATION,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, DVASPECT_CONTENT,
    IAdviseSink, IDataObject, IEnumFORMATETC, IEnumSTATDATA, FORMATETC, STGMEDIUM, STGMEDIUM_0,
    TYMED_HGLOBAL,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GHND};
use windows::Win32::System::Ole::{
    DoDragDrop, IDropSource, IDropTarget, IDropTarget_Impl, ReleaseStgMedium, DROPEFFECT,
    DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE,
};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Shell::{
    FOLDERID_Screenshots, SHGetFileInfoW, SHGetKnownFolderPath, KNOWN_FOLDER_FLAG, SHFILEINFOW,
    SHGFI_ICON, SHGFI_LARGEICON,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, PostMessageW};

pub const WM_APP_SHELF_ITEM_READY: u32 = 0x8017;
const DATA_S_SAMEFORMATETC: HRESULT = HRESULT(0x00040130);

static CF_TEXT_PLAIN: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
static CF_TEXT_PLAIN_UTF8: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
static CF_PNG: std::sync::OnceLock<u32> = std::sync::OnceLock::new();

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

pub fn get_cf_png() -> u32 {
    *CF_PNG.get_or_init(|| unsafe { RegisterClipboardFormatW(w!("PNG")) })
}

fn temp_snippet_file_path() -> PathBuf {
    shelf_captures_dir().join("shelf_text_snippet.txt")
}

fn get_or_create_temp_text_file(text: &str) -> PathBuf {
    let file_path = temp_snippet_file_path();
    let _ = fs::write(&file_path, text);
    file_path
}

pub fn cleanup_temp_shelf_snippet() {
    let _ = fs::remove_file(temp_snippet_file_path());
}

unsafe fn read_ole_utf16_bounded(hglobal: windows::Win32::Foundation::HGLOBAL) -> Option<String> {
    let byte_len = unsafe { windows::Win32::System::Memory::GlobalSize(hglobal) };
    if byte_len < 2 {
        return None;
    }
    let max_units = (byte_len / 2).min(65536);
    let ptr = unsafe { windows::Win32::System::Memory::GlobalLock(hglobal) } as *const u16;
    if ptr.is_null() {
        return None;
    }
    let mut len = 0usize;
    while len < max_units && unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ptr, len) })
        .trim()
        .to_string();
    let _ = unsafe { windows::Win32::System::Memory::GlobalUnlock(hglobal) };
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

unsafe fn read_ole_bytes_bounded(hglobal: windows::Win32::Foundation::HGLOBAL) -> Option<String> {
    let byte_len = unsafe { windows::Win32::System::Memory::GlobalSize(hglobal) };
    if byte_len == 0 {
        return None;
    }
    let max_bytes = byte_len.min(65536);
    let ptr = unsafe { windows::Win32::System::Memory::GlobalLock(hglobal) } as *const u8;
    if ptr.is_null() {
        return None;
    }
    let mut len = 0usize;
    while len < max_bytes && unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    let text = String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ptr, len) })
        .trim()
        .to_string();
    let _ = unsafe { windows::Win32::System::Memory::GlobalUnlock(hglobal) };
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
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
    // If it's a text or file item, also copy to clipboard so destination apps have it available immediately!
    if let ShelfItemKind::Text(ref text) = item.kind {
        copy_text_to_clipboard(text);
    } else if let ShelfItemKind::File(ref path) = item.kind {
        if is_image_file(path) {
            copy_image_file_to_clipboard(path);
        } else {
            copy_file_to_clipboard_hdrop(path);
        }
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
                    let text_opt = if stg.tymed == TYMED_HGLOBAL.0 as u32 && unsafe { !stg.u.hGlobal.is_invalid() } {
                        unsafe { read_ole_utf16_bounded(stg.u.hGlobal) }
                    } else {
                        None
                    };
                    unsafe { ReleaseStgMedium(&stg as *const _ as *mut _) };

                    if let Some(text) = text_opt {
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
                        let text_opt = if stg.tymed == TYMED_HGLOBAL.0 as u32 && unsafe { !stg.u.hGlobal.is_invalid() } {
                            unsafe { read_ole_bytes_bounded(stg.u.hGlobal) }
                        } else {
                            None
                        };
                        unsafe { ReleaseStgMedium(&stg as *const _ as *mut _) };

                        if let Some(text) = text_opt {
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
                    let text_opt = if stg.tymed == TYMED_HGLOBAL.0 as u32 && unsafe { !stg.u.hGlobal.is_invalid() } {
                        unsafe { read_ole_bytes_bounded(stg.u.hGlobal) }
                    } else {
                        None
                    };
                    unsafe { ReleaseStgMedium(&stg as *const _ as *mut _) };

                    if let Some(text) = text_opt {
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

        // Use crisp rounded thumbnail preview for images; fall back to shell icon for other files
        let (icon_pixels, icon_width, icon_height) = if is_image_file(&path) {
            create_image_thumbnail_from_path(&path, 48, 32)
                .map(|(px, w, h, _, _)| (px, w, h))
                .unwrap_or_else(|| extract_icon_and_destroy(&path))
        } else {
            extract_icon_and_destroy(&path)
        };

        Some(Self {
            kind: ShelfItemKind::File(path),
            name,
            size_formatted,
            icon_pixels,
            icon_width,
            icon_height,
        })
    }

    pub fn from_image_with_thumb(
        path: PathBuf,
        thumb: (Vec<u32>, usize, usize),
        size_bytes: u64,
    ) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Screenshot.png".to_string());

        Self {
            kind: ShelfItemKind::File(path),
            name,
            size_formatted: format_file_size(size_bytes),
            icon_pixels: thumb.0,
            icon_width: thumb.1,
            icon_height: thumb.2,
        }
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

    /// Replaces the most recent temporary clipboard screenshot with its finalized Pictures\Screenshots path.
    /// Returns the superseded temp path so the caller can remove it outside any UI lock.
    pub fn upgrade_recent_temp_capture(&mut self, real_item: ShelfItem) -> Option<PathBuf> {
        if self.items.iter().any(|i| i.kind == real_item.kind) {
            return None;
        }
        for slot in self.items.iter_mut().rev() {
            let old_temp = match &slot.kind {
                ShelfItemKind::File(p) if is_temp_shelf_capture(p) => Some(p.clone()),
                _ => None,
            };
            if let Some(temp_path) = old_temp {
                *slot = real_item;
                return Some(temp_path);
            }
        }
        None
    }

    pub fn remove_at(&mut self, index: usize) {
        if index < self.items.len() {
            self.items.remove(index);
        }
    }

    /// Returns the most recently added image file path currently on the shelf (pure in-memory).
    pub fn latest_image_path(&self) -> Option<PathBuf> {
        self.items.iter().rev().find_map(|item| match &item.kind {
            ShelfItemKind::File(path) if is_image_file(path) => Some(path.clone()),
            _ => None,
        })
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.items.clear();
        cleanup_temp_shelf_snippet();
    }
}

/// Allocates a movable `GHND` global memory block and copies `bytes` into it.
unsafe fn alloc_hglobal_bytes(bytes: &[u8]) -> Option<HGLOBAL> {
    if bytes.is_empty() {
        return None;
    }
    let h_global = unsafe { GlobalAlloc(GHND, bytes.len()).ok()? };
    let ptr = unsafe { GlobalLock(h_global) as *mut u8 };
    if ptr.is_null() {
        let _ = unsafe { GlobalFree(h_global) };
        return None;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        let _ = GlobalUnlock(h_global);
    }
    Some(h_global)
}

/// Allocates a `CF_HDROP` `HGLOBAL` pointing to `path`.
unsafe fn build_hdrop_hglobal(path: &Path) -> Option<HGLOBAL> {
    let wide_path: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .chain(std::iter::once(0))
        .collect();

    let dropfiles_size = std::mem::size_of::<windows::Win32::UI::Shell::DROPFILES>();
    let total_size = dropfiles_size + wide_path.len() * 2;
    let h_global = unsafe { GlobalAlloc(GHND, total_size).ok()? };
    let ptr = unsafe { GlobalLock(h_global) as *mut u8 };
    if ptr.is_null() {
        let _ = unsafe { GlobalFree(h_global) };
        return None;
    }

    unsafe {
        let df = ptr as *mut windows::Win32::UI::Shell::DROPFILES;
        (*df).pFiles = dropfiles_size as u32;
        (*df).fWide = BOOL(1);

        let files_ptr = ptr.add(dropfiles_size) as *mut u16;
        std::ptr::copy_nonoverlapping(wide_path.as_ptr(), files_ptr, wide_path.len());
        let _ = GlobalUnlock(h_global);
    }
    Some(h_global)
}

/// Copies a file to the Windows Clipboard in CF_HDROP format
/// Allows instant paste (Ctrl+V) into WhatsApp, Discord, Browser, or Explorer!
pub fn copy_file_to_clipboard_hdrop(path: &Path) -> bool {
    unsafe {
        let Some(h_global) = build_hdrop_hglobal(path) else {
            return false;
        };

        IS_INTERNAL_CLIPBOARD_COPY.store(true, Ordering::SeqCst);
        if OpenClipboard(HWND::default()).is_ok() {
            let _ = EmptyClipboard();
            let res = SetClipboardData(15, HANDLE(h_global.0));
            if res.is_err() {
                let _ = GlobalFree(h_global);
            }
            let _ = CloseClipboard();
            return res.is_ok();
        }
        IS_INTERNAL_CLIPBOARD_COPY.store(false, Ordering::SeqCst);
        let _ = GlobalFree(h_global);
    }
    false
}

/// Decodes an image file via WIC into a bottom-up 24bpp `CF_DIB` payload for legacy desktop apps.
fn build_dib_bytes_from_image_bytes(img_bytes: &[u8]) -> Option<Vec<u8>> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let factory: IWICImagingFactory =
            windows::Win32::System::Com::CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .ok()?;
        let stream = factory.CreateStream().ok()?;
        stream.InitializeFromMemory(img_bytes).ok()?;
        let decoder = factory
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;

        let (mut w, mut h) = (0u32, 0u32);
        frame.GetSize(&mut w, &mut h).ok()?;
        let pixel_count = (w as u64).saturating_mul(h as u64);
        if w == 0 || h == 0 || pixel_count > 8_300_000 {
            return None;
        }

        let converter: IWICFormatConverter = factory.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &frame,
                &GUID_WICPixelFormat24bppBGR,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;

        let stride = ((w as usize) * 3 + 3) & !3;
        let pixel_bytes = stride * (h as usize);
        let mut top_down = vec![0u8; pixel_bytes];
        converter
            .CopyPixels(std::ptr::null(), stride as u32, &mut top_down)
            .ok()?;

        let header_size = std::mem::size_of::<BITMAPINFOHEADER>();
        let mut dib = Vec::with_capacity(header_size + pixel_bytes);
        let bih = BITMAPINFOHEADER {
            biSize: header_size as u32,
            biWidth: w as i32,
            biHeight: h as i32, // Positive height = standard bottom-up DIB
            biPlanes: 1,
            biBitCount: 24,
            biCompression: BI_RGB.0,
            biSizeImage: pixel_bytes as u32,
            ..Default::default()
        };
        let bih_bytes = std::slice::from_raw_parts(&bih as *const _ as *const u8, header_size);
        dib.extend_from_slice(bih_bytes);

        // Flip top-down WIC rows to bottom-up DIB order
        for row in (0..h as usize).rev() {
            let start = row * stride;
            dib.extend_from_slice(&top_down[start..start + stride]);
        }
        Some(dib)
    }
}

/// Copies an image file to the clipboard in multiple formats (`CF_HDROP`, `PNG`, and `CF_DIB`)
/// so both file-aware apps (Explorer/WhatsApp) and image-aware apps (Chrome/Discord/Paint/Word) paste seamlessly.
pub fn copy_image_file_to_clipboard(path: &Path) -> bool {
    unsafe {
        let h_hdrop = build_hdrop_hglobal(path);

        let img_bytes = fs::metadata(path)
            .ok()
            .filter(|m| m.len() > 0 && m.len() <= 15 * 1024 * 1024)
            .and_then(|_| fs::read(path).ok());

        let is_png = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("png"))
            .unwrap_or(false);

        let h_png = if is_png {
            img_bytes.as_deref().and_then(|b| alloc_hglobal_bytes(b))
        } else {
            None
        };

        let h_dib = img_bytes
            .as_deref()
            .and_then(build_dib_bytes_from_image_bytes)
            .and_then(|dib| alloc_hglobal_bytes(&dib));

        if h_hdrop.is_none() && h_png.is_none() && h_dib.is_none() {
            return false;
        }

        IS_INTERNAL_CLIPBOARD_COPY.store(true, Ordering::SeqCst);
        if OpenClipboard(HWND::default()).is_ok() {
            let _ = EmptyClipboard();
            let mut any_ok = false;

            if let Some(h) = h_hdrop {
                if SetClipboardData(15, HANDLE(h.0)).is_ok() {
                    any_ok = true;
                } else {
                    let _ = GlobalFree(h);
                }
            }
            if let Some(h) = h_png {
                let cf_png = get_cf_png();
                if cf_png != 0 && SetClipboardData(cf_png, HANDLE(h.0)).is_ok() {
                    any_ok = true;
                } else {
                    let _ = GlobalFree(h);
                }
            }
            if let Some(h) = h_dib {
                if SetClipboardData(8, HANDLE(h.0)).is_ok() {
                    any_ok = true;
                } else {
                    let _ = GlobalFree(h);
                }
            }

            let _ = CloseClipboard();
            return any_ok;
        }

        IS_INTERNAL_CLIPBOARD_COPY.store(false, Ordering::SeqCst);
        if let Some(h) = h_hdrop {
            let _ = GlobalFree(h);
        }
        if let Some(h) = h_png {
            let _ = GlobalFree(h);
        }
        if let Some(h) = h_dib {
            let _ = GlobalFree(h);
        }
    }
    false
}

/// Returns true if the path has a supported raster image extension.
pub fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "bmp" | "webp"
            )
        })
        .unwrap_or(false)
}

/// Prepends a 14-byte BITMAPFILEHEADER onto raw `CF_DIB` bytes so WIC can decode it cleanly.
pub fn dib_to_bmp_bytes(dib: &[u8]) -> Option<Vec<u8>> {
    if dib.len() < 40 {
        return None;
    }
    let bi_size = u32::from_le_bytes([dib[0], dib[1], dib[2], dib[3]]);
    if bi_size < 40 || (bi_size as usize) > dib.len() {
        return None;
    }
    let bi_bit_count = u16::from_le_bytes([dib[14], dib[15]]);
    let bi_compression = u32::from_le_bytes([dib[16], dib[17], dib[18], dib[19]]);
    let bi_clr_used = u32::from_le_bytes([dib[32], dib[33], dib[34], dib[35]]);

    let color_table_bytes = if bi_size == 40 && bi_compression == 3 {
        // BI_BITFIELDS has 3 DWORD color masks after BITMAPINFOHEADER
        12u32
    } else if bi_bit_count <= 8 {
        let entries = if bi_clr_used > 0 {
            bi_clr_used
        } else {
            1u32 << bi_bit_count
        };
        entries.saturating_mul(4)
    } else {
        bi_clr_used.saturating_mul(4)
    };

    let bf_off_bits = 14u32.saturating_add(bi_size).saturating_add(color_table_bytes);
    let file_size = 14usize.saturating_add(dib.len()).min(u32::MAX as usize) as u32;

    let mut bmp = Vec::with_capacity(14 + dib.len());
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&file_size.to_le_bytes());
    bmp.extend_from_slice(&0u16.to_le_bytes());
    bmp.extend_from_slice(&0u16.to_le_bytes());
    bmp.extend_from_slice(&bf_off_bits.to_le_bytes());
    bmp.extend_from_slice(dib);
    Some(bmp)
}

/// Encodes a BMP byte stream into a PNG file on disk using WIC (converting to 24bppBGR to avoid zero-alpha GDI artifacts).
pub fn encode_bmp_bytes_to_png_file_wic(bmp_bytes: &[u8], out_path: &Path) -> Option<(u32, u32)> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let factory: IWICImagingFactory =
            windows::Win32::System::Com::CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .ok()?;

        let in_stream = factory.CreateStream().ok()?;
        in_stream.InitializeFromMemory(bmp_bytes).ok()?;
        let decoder = factory
            .CreateDecoderFromStream(&in_stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;

        let (mut w, mut h) = (0u32, 0u32);
        frame.GetSize(&mut w, &mut h).ok()?;
        if w < 8 || h < 8 {
            return None;
        }

        let converter: IWICFormatConverter = factory.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &frame,
                &GUID_WICPixelFormat24bppBGR,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;

        let wide_out: Vec<u16> = out_path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let out_stream = factory.CreateStream().ok()?;
        out_stream
            .InitializeFromFilename(PCWSTR(wide_out.as_ptr()), 0x40000000)
            .ok()?;

        let encoder = factory
            .CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())
            .ok()?;
        encoder
            .Initialize(&out_stream, WICBitmapEncoderNoCache)
            .ok()?;

        let mut frame_encode: Option<IWICBitmapFrameEncode> = None;
        encoder
            .CreateNewFrame(&mut frame_encode, std::ptr::null_mut())
            .ok()?;
        let frame_encode = frame_encode?;
        frame_encode.Initialize(None).ok()?;
        frame_encode.SetSize(w, h).ok()?;
        let mut fmt = GUID_WICPixelFormat24bppBGR;
        frame_encode.SetPixelFormat(&mut fmt).ok()?;
        frame_encode.WriteSource(&converter, std::ptr::null()).ok()?;
        frame_encode.Commit().ok()?;
        encoder.Commit().ok()?;

        Some((w, h))
    }
}

/// Decodes raw image bytes via WIC into a crisp rounded-corner BGRA thumbnail plus original image dimensions.
pub fn create_image_thumbnail_from_bytes(
    img_bytes: &[u8],
    max_w: usize,
    max_h: usize,
) -> Option<(Vec<u32>, usize, usize, u32, u32)> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let factory: IWICImagingFactory =
            windows::Win32::System::Com::CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .ok()?;

        let stream = factory.CreateStream().ok()?;
        stream.InitializeFromMemory(img_bytes).ok()?;
        let decoder = factory
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;

        let (mut orig_w, mut orig_h) = (0u32, 0u32);
        frame.GetSize(&mut orig_w, &mut orig_h).ok()?;
        if orig_w < 8 || orig_h < 8 {
            return None;
        }

        let scale = (max_w as f32 / orig_w as f32).min(max_h as f32 / orig_h as f32);
        let tw = ((orig_w as f32 * scale).round() as usize).clamp(16, max_w.max(16));
        let th = ((orig_h as f32 * scale).round() as usize).clamp(16, max_h.max(16));

        let scaler: IWICBitmapScaler = factory.CreateBitmapScaler().ok()?;
        scaler
            .Initialize(&frame, tw as u32, th as u32, WICBitmapInterpolationModeFant)
            .ok()?;

        let converter: IWICFormatConverter = factory.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &scaler,
                &GUID_WICPixelFormat32bppBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;

        let mut pixels = vec![0u32; tw * th];
        let stride = (tw * 4) as u32;
        converter
            .CopyPixels(
                std::ptr::null(),
                stride,
                std::slice::from_raw_parts_mut(pixels.as_mut_ptr() as *mut u8, pixels.len() * 4),
            )
            .ok()?;

        if pixels.iter().all(|px| (*px >> 24) == 0) {
            for px in pixels.iter_mut() {
                *px |= 0xFF000000;
            }
        }

        // Apply crisp 4px SDF rounded corners and subtle 1px inner border (straight non-premultiplied ARGB for blend_icon)
        for py in 0..th {
            for px in 0..tw {
                let dist = crate::render::primitives::rounded_rect_sdf(
                    px as f32 + 0.5,
                    py as f32 + 0.5,
                    0.0,
                    0.0,
                    tw as f32,
                    th as f32,
                    4.0,
                );
                let cov = (0.5 - dist).clamp(0.0, 1.0);
                let idx = py * tw + px;
                if cov <= 0.001 {
                    pixels[idx] = 0;
                    continue;
                }

                let p = pixels[idx];
                let orig_a = ((p >> 24) & 0xFF) as f32;
                let mut r = ((p >> 16) & 0xFF) as f32;
                let mut g = ((p >> 8) & 0xFF) as f32;
                let mut b = (p & 0xFF) as f32;

                if dist > -1.0 && dist <= 0.0 && orig_a > 128.0 {
                    // Subtle glass rim highlight around thumbnail edge
                    let border_mix = 0.28;
                    r = r * (1.0 - border_mix) + 255.0 * border_mix;
                    g = g * (1.0 - border_mix) + 255.0 * border_mix;
                    b = b * (1.0 - border_mix) + 255.0 * border_mix;
                }

                let out_a = (orig_a * cov).round().clamp(0.0, 255.0) as u32;
                let out_r = r.round().clamp(0.0, 255.0) as u32;
                let out_g = g.round().clamp(0.0, 255.0) as u32;
                let out_b = b.round().clamp(0.0, 255.0) as u32;
                pixels[idx] = (out_a << 24) | (out_r << 16) | (out_g << 8) | out_b;
            }
        }

        Some((pixels, tw, th, orig_w, orig_h))
    }
}

/// Loads an image file safely (<= 20 MB) and generates its rounded Shelf thumbnail.
pub fn create_image_thumbnail_from_path(
    path: &Path,
    max_w: usize,
    max_h: usize,
) -> Option<(Vec<u32>, usize, usize, u32, u32)> {
    let len = fs::metadata(path).ok()?.len();
    if len == 0 || len > 20 * 1024 * 1024 {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    create_image_thumbnail_from_bytes(&bytes, max_w, max_h)
}

/// Directory storing temporary PNG captures promoted from clipboard screenshots.
pub fn shelf_captures_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("NotDynamicIsland").join("ShelfCaptures");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn is_temp_shelf_capture(path: &Path) -> bool {
    path.parent() == Some(shelf_captures_dir().as_path())
}

fn generate_screenshot_path(dir: &Path) -> PathBuf {
    let st = unsafe { GetLocalTime() };
    let base_name = format!("Screenshot_{:02}{:02}{:02}.png", st.wHour, st.wMinute, st.wSecond);
    let candidate = dir.join(&base_name);
    if !candidate.exists() {
        return candidate;
    }
    for n in 0..100u32 {
        let suffix = if n == 0 {
            format!("Screenshot_{:02}{:02}{:02}_{:03}.png", st.wHour, st.wMinute, st.wSecond, st.wMilliseconds)
        } else {
            format!("Screenshot_{:02}{:02}{:02}_{:03}_{}.png", st.wHour, st.wMinute, st.wSecond, st.wMilliseconds, n)
        };
        let p = dir.join(suffix);
        if !p.exists() {
            return p;
        }
    }
    candidate
}

fn prune_old_shelf_captures(dir: &Path, keep_max: usize) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let p = entry.path();
            if !p.is_file() {
                return None;
            }
            let modified = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(UNIX_EPOCH);
            Some((modified, p))
        })
        .collect();

    if files.len() <= keep_max {
        return;
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, old_path) in files.into_iter().skip(keep_max) {
        let _ = fs::remove_file(old_path);
    }
}

pub struct PendingShelfItem {
    pub item: ShelfItem,
    pub upgrade_recent_temp: bool,
    pub dimensions: Option<(u32, u32)>,
}

static PENDING_SHELF_ITEMS: Mutex<Vec<PendingShelfItem>> = Mutex::new(Vec::new());
static LAST_CLIPBOARD_SCREENSHOT_MS: AtomicU64 = AtomicU64::new(0);
static LAST_CLIPBOARD_IMAGE_HASH: AtomicU64 = AtomicU64::new(0);

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn fnv1a_64(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

pub fn take_pending_shelf_items() -> Vec<PendingShelfItem> {
    if let Ok(mut queue) = PENDING_SHELF_ITEMS.lock() {
        std::mem::take(&mut *queue)
    } else {
        Vec::new()
    }
}

fn push_pending_and_notify(hwnd: HWND, pending: PendingShelfItem) {
    if let Ok(mut queue) = PENDING_SHELF_ITEMS.lock() {
        queue.push(pending);
    }
    unsafe {
        let _ = PostMessageW(hwnd, WM_APP_SHELF_ITEM_READY, WPARAM(0), LPARAM(0));
    }
}

/// Processes a newly copied clipboard screenshot/image asynchronously off the UI thread.
pub fn handle_clipboard_image_async(hwnd: HWND, raw_bytes: Vec<u8>, is_png: bool) {
    if raw_bytes.len() < 32 {
        return;
    }

    let now_ms = epoch_ms();
    let hash = fnv1a_64(&raw_bytes);
    let prev_hash = LAST_CLIPBOARD_IMAGE_HASH.load(Ordering::Relaxed);
    let prev_ms = LAST_CLIPBOARD_SCREENSHOT_MS.load(Ordering::Relaxed);
    if hash == prev_hash && now_ms.saturating_sub(prev_ms) < 3000 {
        return;
    }
    LAST_CLIPBOARD_IMAGE_HASH.store(hash, Ordering::Relaxed);
    LAST_CLIPBOARD_SCREENSHOT_MS.store(now_ms, Ordering::Relaxed);

    let hwnd_val = hwnd.0 as usize;
    std::thread::Builder::new()
        .name("ShelfImageWorker".to_string())
        .spawn(move || {
            let dir = shelf_captures_dir();
            let out_path = generate_screenshot_path(&dir);

            let (thumb_pixels, tw, th, orig_w, orig_h) = if is_png {
                if raw_bytes.len() < 8 || &raw_bytes[..8] != b"\x89PNG\r\n\x1a\n" {
                    return;
                }
                let Some(thumb) = create_image_thumbnail_from_bytes(&raw_bytes, 48, 32) else {
                    return;
                };
                if fs::write(&out_path, &raw_bytes).is_err() {
                    return;
                }
                thumb
            } else {
                let Some(bmp_bytes) = dib_to_bmp_bytes(&raw_bytes) else {
                    return;
                };
                if encode_bmp_bytes_to_png_file_wic(&bmp_bytes, &out_path).is_none() {
                    return;
                }
                let Some(thumb) = create_image_thumbnail_from_path(&out_path, 48, 32) else {
                    let _ = fs::remove_file(&out_path);
                    return;
                };
                thumb
            };

            prune_old_shelf_captures(&dir, 10);
            let file_len = fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
            let item = ShelfItem::from_image_with_thumb(out_path, (thumb_pixels, tw, th), file_len);
            LAST_CLIPBOARD_SCREENSHOT_MS.store(epoch_ms(), Ordering::Relaxed);
            push_pending_and_notify(
                HWND(hwnd_val as *mut _),
                PendingShelfItem {
                    item,
                    upgrade_recent_temp: false,
                    dimensions: Some((orig_w, orig_h)),
                },
            );
            crate::system::trim_working_set();
        })
        .ok();
}

/// Decodes a screenshot image file on a background thread and sends it to the Shelf.
pub fn enqueue_shelf_image_file(hwnd: HWND, path: PathBuf, upgrade_recent_temp: bool) {
    let hwnd_val = hwnd.0 as usize;
    std::thread::Builder::new()
        .name("ShelfFileThumb".to_string())
        .spawn(move || {
            let Some((px, w, h, orig_w, orig_h)) = create_image_thumbnail_from_path(&path, 48, 32) else {
                return;
            };
            let file_len = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let item = ShelfItem::from_image_with_thumb(path, (px, w, h), file_len);
            push_pending_and_notify(
                HWND(hwnd_val as *mut _),
                PendingShelfItem {
                    item,
                    upgrade_recent_temp,
                    dimensions: Some((orig_w, orig_h)),
                },
            );
            crate::system::trim_working_set();
        })
        .ok();
}

/// Resolves the user's Windows Screenshots folder dynamically without hardcoded paths.
pub fn resolve_screenshots_folder() -> Option<PathBuf> {
    unsafe {
        if let Ok(pwstr) = SHGetKnownFolderPath(&FOLDERID_Screenshots, KNOWN_FOLDER_FLAG(0), HANDLE::default()) {
            let mut len = 0usize;
            while *pwstr.0.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(pwstr.0, len);
            let path = PathBuf::from(String::from_utf16_lossy(slice));
            CoTaskMemFree(Some(pwstr.0 as *const _));
            if path.exists() {
                return Some(path);
            }
        }
    }

    let mut candidates = Vec::new();
    if let Some(onedrive) = std::env::var_os("OneDrive") {
        let od = PathBuf::from(onedrive);
        candidates.push(od.join("Pictures").join("Screenshots"));
        candidates.push(od.join("Gambar").join("Screenshots"));
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        let base = PathBuf::from(profile);
        candidates.push(base.join("Pictures").join("Screenshots"));
        candidates.push(base.join("OneDrive").join("Pictures").join("Screenshots"));
        candidates.push(base.join("OneDrive").join("Gambar").join("Screenshots"));
    }
    candidates.into_iter().find(|p| p.exists())
}

/// Event-driven 0.0% idle CPU watcher for the Windows Screenshots folder (captures `Win+PrtSc` and Snipping Tool auto-saves).
pub struct ScreenshotWatcher {
    running: Arc<AtomicBool>,
}

impl ScreenshotWatcher {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start(&mut self, hwnd: HWND) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let running = Arc::clone(&self.running);
        let hwnd_val = hwnd.0 as usize;

        std::thread::Builder::new()
            .name("ScreenshotWatcher".to_string())
            .spawn(move || {
                let Some(folder) = resolve_screenshots_folder() else {
                    return;
                };
                let wide_dir: Vec<u16> = folder
                    .as_os_str()
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();

                let dir_handle = unsafe {
                    CreateFileW(
                        PCWSTR(wide_dir.as_ptr()),
                        FILE_LIST_DIRECTORY.0,
                        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                        None,
                        OPEN_EXISTING,
                        FILE_FLAG_BACKUP_SEMANTICS,
                        HANDLE::default(),
                    )
                };
                let Ok(dir_handle) = dir_handle else {
                    return;
                };

                let mut buf = vec![0u32; 1024];
                let mut last_seen: Option<(PathBuf, u64)> = None;

                while running.load(Ordering::SeqCst) {
                    let mut bytes_returned = 0u32;
                    let ok = unsafe {
                        ReadDirectoryChangesW(
                            dir_handle,
                            buf.as_mut_ptr() as *mut _,
                            (buf.len() * 4) as u32,
                            false,
                            FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE,
                            Some(&mut bytes_returned),
                            None,
                            None,
                        )
                    };
                    if ok.is_err() || !running.load(Ordering::SeqCst) {
                        break;
                    }
                    if bytes_returned == 0 {
                        continue;
                    }

                    let byte_limit = (bytes_returned as usize).min(buf.len() * 4);
                    let raw_ptr = buf.as_ptr() as *const u8;
                    let mut offset = 0usize;

                    while offset + std::mem::size_of::<FILE_NOTIFY_INFORMATION>() <= byte_limit {
                        let info = unsafe { &*(raw_ptr.add(offset) as *const FILE_NOTIFY_INFORMATION) };
                        let name_bytes = info.FileNameLength as usize;
                        let name_offset = offset + 12;
                        if name_bytes > 0 && name_offset + name_bytes <= byte_limit {
                            let utf16_slice = unsafe {
                                std::slice::from_raw_parts(
                                    raw_ptr.add(name_offset) as *const u16,
                                    name_bytes / 2,
                                )
                            };
                            let file_name = String::from_utf16_lossy(utf16_slice);
                            let action = info.Action.0;

                            if (action == 1 || action == 3 || action == 5)
                                && !file_name.starts_with('.')
                                && !file_name.ends_with(".tmp")
                            {
                                let full_path = folder.join(&file_name);
                                if is_image_file(&full_path) {
                                    let now_ms = epoch_ms();
                                    let is_dup = last_seen
                                        .as_ref()
                                        .map(|(p, t)| p == &full_path && now_ms.saturating_sub(*t) < 2500)
                                        .unwrap_or(false);

                                    if !is_dup {
                                        last_seen = Some((full_path.clone(), now_ms));

                                        // Wait briefly on watcher thread until Windows finishes flushing the file
                                        let mut ready = false;
                                        for _ in 0..6 {
                                            if fs::metadata(&full_path).map(|m| m.len() > 0).unwrap_or(false)
                                                && fs::File::open(&full_path).is_ok()
                                            {
                                                ready = true;
                                                break;
                                            }
                                            std::thread::sleep(Duration::from_millis(60));
                                        }

                                        if ready {
                                            let last_clip_ms = LAST_CLIPBOARD_SCREENSHOT_MS.load(Ordering::Relaxed);
                                            let upgrade_recent_temp = epoch_ms().saturating_sub(last_clip_ms) <= 4500;
                                            enqueue_shelf_image_file(
                                                HWND(hwnd_val as *mut _),
                                                full_path,
                                                upgrade_recent_temp,
                                            );
                                        }
                                    }
                                }
                            }
                        }

                        if info.NextEntryOffset == 0 {
                            break;
                        }
                        offset += info.NextEntryOffset as usize;
                    }
                }

                unsafe {
                    let _ = CloseHandle(dir_handle);
                }
            })
            .ok();
    }
}

impl Drop for ScreenshotWatcher {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
    }
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
                    let ok = windows::Win32::System::DataExchange::SetClipboardData(
                        13,
                        windows::Win32::Foundation::HANDLE(h_global.0),
                    )
                    .is_ok();
                    let _ = windows::Win32::System::DataExchange::CloseClipboard();
                    if ok {
                        return true;
                    }
                    IS_INTERNAL_CLIPBOARD_COPY.store(false, Ordering::SeqCst);
                } else {
                    IS_INTERNAL_CLIPBOARD_COPY.store(false, Ordering::SeqCst);
                }
            }
            let _ = windows::Win32::Foundation::GlobalFree(h_global);
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

    #[test]
    fn test_dib_to_bmp_bytes_and_wic_png_roundtrip() {
        // Construct a synthetic 32x24 32-bit CF_DIB buffer with alpha == 0 (simulating GDI PrintScreen)
        let w = 32u32;
        let h = 24u32;
        let bih = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: h as i32,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: w * h * 4,
            ..Default::default()
        };
        let mut dib = Vec::with_capacity(40 + (w * h * 4) as usize);
        let bih_bytes = unsafe {
            std::slice::from_raw_parts(&bih as *const _ as *const u8, std::mem::size_of::<BITMAPINFOHEADER>())
        };
        dib.extend_from_slice(bih_bytes);
        for _ in 0..(w * h) {
            // BGR0 with zero alpha byte (as GDI BitBlt produces)
            dib.extend_from_slice(&[0xD0, 0x70, 0x20, 0x00]);
        }

        let bmp = dib_to_bmp_bytes(&dib).expect("dib_to_bmp_bytes must succeed");
        assert_eq!(&bmp[0..2], b"BM");

        let out_path = std::env::temp_dir().join("ndi_test_dib_roundtrip.png");
        let _ = fs::remove_file(&out_path);
        let dims = encode_bmp_bytes_to_png_file_wic(&bmp, &out_path);
        assert_eq!(dims, Some((w, h)));
        assert!(out_path.exists());

        let thumb = create_image_thumbnail_from_path(&out_path, 48, 32)
            .expect("create_image_thumbnail_from_path must succeed");
        let (pixels, tw, th, orig_w, orig_h) = thumb;
        assert_eq!((orig_w, orig_h), (w, h));
        assert!(tw <= 48 && th <= 32 && tw >= 16 && th >= 16);
        assert_eq!(pixels.len(), tw * th);

        // Center pixel must be opaque (not zero-alpha transparent)
        let center_px = pixels[(th / 2) * tw + (tw / 2)];
        assert_eq!((center_px >> 24) & 0xFF, 255);
        // Outer corner pixel (0, 0) must be trimmed/transparent due to 4px rounded corners
        assert!(((pixels[0] >> 24) & 0xFF) < 255);

        let _ = fs::remove_file(&out_path);
    }

    #[test]
    fn test_shelf_upgrade_recent_temp_capture() {
        let mut shelf = FileShelf::new();
        let temp_path = shelf_captures_dir().join("Screenshot_TestTemp_120000.png");
        let temp_item = ShelfItem::from_image_with_thumb(temp_path.clone(), (vec![0xFFFFFFFF; 16 * 16], 16, 16), 1024);
        shelf.add(temp_item);
        assert_eq!(shelf.items.len(), 1);

        let final_path = std::env::temp_dir().join("Screenshot_TestFinal.png");
        let final_item = ShelfItem::from_image_with_thumb(final_path.clone(), (vec![0xFF00FF00; 16 * 16], 16, 16), 2048);
        let replaced = shelf.upgrade_recent_temp_capture(final_item);

        assert_eq!(replaced, Some(temp_path));
        assert_eq!(shelf.items.len(), 1);
        assert_eq!(shelf.items[0].path(), Some(final_path.as_path()));
    }
}
