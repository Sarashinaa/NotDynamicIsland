//! App launcher and Windows automation engine with typo tolerance, alias resolution,
//! process decoupling, and natural language command parsing.
//! 0.0% idle CPU, zero heavyweight GUI dependencies, purely Win32 API.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
    IsHungAppWindow, IsIconic, IsWindowVisible, SetForegroundWindow, ShowWindow,
    GWL_STYLE, SW_RESTORE, SW_SHOW, SW_SHOWNORMAL, WS_CHILD,
};

#[link(name = "user32")]
unsafe extern "system" {
    fn LockWorkStation() -> BOOL;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AppAlias {
    pub search_keyword: &'static str,
    pub executable: &'static str,
    pub uri_scheme: Option<&'static str>,
    pub friendly_name: &'static str,
}

const APP_ALIASES: &[(&[&str], AppAlias)] = &[
    (
        &["wa", "whatsapp", "whatsap", "watsap", "whatapp", "watsapp"],
        AppAlias {
            search_keyword: "whatsapp",
            executable: "whatsapp.exe",
            uri_scheme: Some("whatsapp:"),
            friendly_name: "WhatsApp",
        },
    ),
    (
        &["dc", "discord", "diskord", "discrod"],
        AppAlias {
            search_keyword: "discord",
            executable: "update.exe",
            uri_scheme: Some("discord:"),
            friendly_name: "Discord",
        },
    ),
    (
        &["tele", "tg", "telegram", "telegaram"],
        AppAlias {
            search_keyword: "telegram",
            executable: "telegram.exe",
            uri_scheme: Some("tg:"),
            friendly_name: "Telegram",
        },
    ),
    (
        &["spoti", "spotify", "spotipi", "spotifi"],
        AppAlias {
            search_keyword: "spotify",
            executable: "spotify.exe",
            uri_scheme: Some("spotify:"),
            friendly_name: "Spotify",
        },
    ),
    (
        &["wrod", "wodr", "word", "words", "ms word", "microsoft word", "winword", "doc", "docs"],
        AppAlias {
            search_keyword: "word",
            executable: "winword.exe",
            uri_scheme: None,
            friendly_name: "Microsoft Word",
        },
    ),
    (
        &["eksel", "exel", "excel", "ms excel", "microsoft excel", "xls", "xlsx", "spreadsheet"],
        AppAlias {
            search_keyword: "excel",
            executable: "excel.exe",
            uri_scheme: None,
            friendly_name: "Microsoft Excel",
        },
    ),
    (
        &["ppt", "pptx", "powerpoint", "power point", "ms powerpoint", "slides", "presentasi"],
        AppAlias {
            search_keyword: "powerpoint",
            executable: "powerpnt.exe",
            uri_scheme: None,
            friendly_name: "Microsoft PowerPoint",
        },
    ),
    (
        &["kalkulator", "kalku", "calc", "calculator"],
        AppAlias {
            search_keyword: "calculator",
            executable: "calc.exe",
            uri_scheme: Some("calculator:"),
            friendly_name: "Kalkulator",
        },
    ),
    (
        &["notepad", "noteped"],
        AppAlias {
            search_keyword: "notepad",
            executable: "notepad.exe",
            uri_scheme: None,
            friendly_name: "Notepad",
        },
    ),
    (
        &["onenote", "one note", "ms onenote", "microsoft onenote"],
        AppAlias {
            search_keyword: "onenote",
            executable: "onenote.exe",
            uri_scheme: Some("onenote:"),
            friendly_name: "Microsoft OneNote",
        },
    ),
    (
        &[
            "stickynotes",
            "sticky_notes",
            "sticky note",
            "sticky notes",
            "stickynote",
            "sticky",
            "stiky note",
            "stiky notes",
        ],
        AppAlias {
            search_keyword: "sticky",
            executable: "stikybl.exe",
            uri_scheme: Some("ms-sticky-notes:"),
            friendly_name: "Sticky Notes",
        },
    ),
    (
        &["brv", "brave", "brave browser"],
        AppAlias {
            search_keyword: "brave",
            executable: "brave.exe",
            uri_scheme: None,
            friendly_name: "Brave Browser",
        },
    ),
    (
        &["krom", "crome", "chrome", "google chrome"],
        AppAlias {
            search_keyword: "chrome",
            executable: "chrome.exe",
            uri_scheme: None,
            friendly_name: "Google Chrome",
        },
    ),
    (
        &["edg", "edge", "msedge", "microsoft edge"],
        AppAlias {
            search_keyword: "edge",
            executable: "msedge.exe",
            uri_scheme: None,
            friendly_name: "Microsoft Edge",
        },
    ),
    (
        &["code", "vscode", "vs code", "visual studio code", "kodingan"],
        AppAlias {
            search_keyword: "visual studio code",
            executable: "code.cmd",
            uri_scheme: Some("vscode:"),
            friendly_name: "Visual Studio Code",
        },
    ),
    (
        &["terminal", "wt", "windows terminal"],
        AppAlias {
            search_keyword: "terminal",
            executable: "wt.exe",
            uri_scheme: None,
            friendly_name: "Windows Terminal",
        },
    ),
    (
        &["cmd", "command prompt"],
        AppAlias {
            search_keyword: "cmd.exe",
            executable: "cmd.exe",
            uri_scheme: None,
            friendly_name: "Command Prompt",
        },
    ),
    (
        &["powershell", "pwsh"],
        AppAlias {
            search_keyword: "powershell",
            executable: "powershell.exe",
            uri_scheme: None,
            friendly_name: "PowerShell",
        },
    ),
    (
        &["explorer", "berkas", "file explorer"],
        AppAlias {
            search_keyword: "explorer",
            executable: "explorer.exe",
            uri_scheme: None,
            friendly_name: "File Explorer",
        },
    ),
    (
        &["setting", "settings", "pengaturan", "setelan"],
        AppAlias {
            search_keyword: "settings",
            executable: "systemsettings.exe",
            uri_scheme: Some("ms-settings:"),
            friendly_name: "Pengaturan Windows",
        },
    ),
    (
        &["paint", "mspaint", "gambar"],
        AppAlias {
            search_keyword: "paint",
            executable: "mspaint.exe",
            uri_scheme: Some("ms-paint:"),
            friendly_name: "Paint",
        },
    ),
    (
        &["snip", "snipping tool", "tangkapan layar"],
        AppAlias {
            search_keyword: "snippingtool",
            executable: "snippingtool.exe",
            uri_scheme: Some("ms-screenclip:"),
            friendly_name: "Snipping Tool",
        },
    ),
];

/// Resolves an app query or typo/slang into its canonical alias definition
pub fn resolve_app_alias(input: &str) -> Option<AppAlias> {
    let lower = input.trim().to_lowercase();
    for (aliases, info) in APP_ALIASES {
        for alias in *aliases {
            if lower == *alias {
                return Some(*info);
            }
        }
    }
    None
}

/// Resolves an input to either known alias or a generic executable/search target
pub fn resolve_or_generic(input: &str) -> (String, String, Option<String>, String) {
    let lower = input.trim().to_lowercase();
    if let Some(res) = resolve_app_alias(&lower) {
        (
            res.search_keyword.to_string(),
            res.executable.to_string(),
            res.uri_scheme.map(|s| s.to_string()),
            res.friendly_name.to_string(),
        )
    } else {
        let clean = input.trim();
        let exe = if clean.to_lowercase().ends_with(".exe") {
            clean.to_lowercase()
        } else {
            format!("{}.exe", clean.to_lowercase())
        };
        let friendly = capitalize_words(clean);
        (clean.to_lowercase(), exe, None, friendly)
    }
}

fn capitalize_words(s: &str) -> String {
    let mut words = Vec::new();
    for word in s.split_whitespace() {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            let cap = first.to_uppercase().chain(chars).collect::<String>();
            words.push(cap);
        }
    }
    if words.is_empty() {
        s.to_string()
    } else {
        words.join(" ")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AppLaunchIntent {
    OpenApp { name: String, friendly_name: String },
    OpenBrowserProfile { browser: String, profile: String },
    OpenExplorer { path: String },
    CreateNewFile { folder: String, filename: String },
    LockWorkstation,
}

/// Identifies generic or ambiguous app requests that should NOT be eagerly launched offline,
/// but instead routed to Groq AI for conversational disambiguation (e.g. asking which note/browser app).
pub fn is_ambiguous_app_target(target: &str) -> bool {
    let lower = target.trim().to_lowercase();
    match lower.as_str() {
        "note" | "notes" | "catatan" | "teks" | "aplikasi catatan" | "app catatan" | "buku catatan" => true,
        "browser" | "peramban" | "web" | "web browser" | "aplikasi browser" | "app browser" | "internet" => true,
        "terminal" | "konsol" | "console" | "cli" | "command line" => true,
        "office" | "ms office" | "microsoft office" => true,
        _ => false,
    }
}

/// Natural language extractor for offline 0ms execution of Windows commands
pub fn extract_launch_intent(prompt: &str) -> Option<AppLaunchIntent> {
    let lower = prompt.trim().to_lowercase();
    if lower.is_empty() {
        return None;
    }

    // 1. Lock Workstation
    if lower == "lock"
        || lower == "lock pc"
        || lower == "lock windows"
        || lower == "kunci pc"
        || lower == "kunci laptop"
        || lower == "kunci komputer"
        || lower == "kunci layar"
    {
        return Some(AppLaunchIntent::LockWorkstation);
    }

    // 2. Create new txt file ("tolong bikin txt baru di folder A", "buat file txt baru", "bikin txt baru di desktop")
    if lower.contains("bikin txt")
        || lower.contains("buat txt")
        || lower.contains("bikin file txt")
        || lower.contains("buat file txt")
        || lower.contains("buat file baru")
        || lower.contains("bikin file baru")
        || lower.contains("buat catatan baru")
        || lower.contains("bikin catatan baru")
    {
        let folder = if let Some(idx) = lower.find(" di folder ") {
            lower[idx + 11..].trim().to_string()
        } else if let Some(idx) = lower.find(" di ") {
            lower[idx + 4..].trim().to_string()
        } else {
            "desktop".to_string()
        };
        // Strip trailing polite words
        let clean_folder = folder
            .trim_end_matches(" ya")
            .trim_end_matches(" dong")
            .trim_end_matches(" please")
            .trim_end_matches(" tolong")
            .trim()
            .to_string();

        let filename = "Catatan_Baru.txt".to_string();
        return Some(AppLaunchIntent::CreateNewFile {
            folder: clean_folder,
            filename,
        });
    }

    // 3. Browser with Profile ("buka brave profil a", "buka chrome dengan profil kerja", "buka brave profile 1")
    let has_browser = lower.contains("brave") || lower.contains("chrome") || lower.contains("edge") || lower.contains("krom");
    if has_browser && (lower.contains("profil") || lower.contains("profile")) {
        let browser = if lower.contains("chrome") || lower.contains("krom") {
            "chrome".to_string()
        } else if lower.contains("edge") {
            "edge".to_string()
        } else {
            "brave".to_string()
        };

        let profile = if let Some(idx) = lower.find("profil ") {
            lower[idx + 7..].trim().to_string()
        } else if let Some(idx) = lower.find("profile ") {
            lower[idx + 8..].trim().to_string()
        } else {
            "Default".to_string()
        };
        let clean_profile = profile
            .trim_end_matches(" ya")
            .trim_end_matches(" dong")
            .trim_end_matches(" please")
            .trim()
            .to_string();

        return Some(AppLaunchIntent::OpenBrowserProfile {
            browser,
            profile: clean_profile,
        });
    }

    // 4. Open specific explorer folder
    // Check for explicit folder commands: "buka folder ...", "open folder ...", "tolong buka folder ..."
    let folder_prefixes = [
        "tolong buka folder ",
        "buka folder ",
        "open folder ",
        "tolong buka direktori ",
        "buka direktori ",
    ];
    for pfx in &folder_prefixes {
        if lower.starts_with(pfx) {
            let mut target = lower[pfx.len()..].trim();
            for suffix in &[" dong", " ya", " please", " tolong", " sekarang"] {
                if target.ends_with(suffix) {
                    target = target[..target.len() - suffix.len()].trim();
                }
            }
            if !target.is_empty() {
                return Some(AppLaunchIntent::OpenExplorer {
                    path: target.to_string(),
                });
            }
        }
    }

    // Common system folders with or without "folder" keyword (e.g. "buka download", "buka downloads", "buka unduhan", "buka dokumen")
    let is_open_cmd = lower.starts_with("buka ") || lower.starts_with("open ") || lower.starts_with("tolong buka ");
    if is_open_cmd {
        if lower.contains("download") || lower.contains("unduhan") {
            return Some(AppLaunchIntent::OpenExplorer {
                path: "downloads".to_string(),
            });
        }
        if lower.contains("document") || lower.contains("dokumen") {
            return Some(AppLaunchIntent::OpenExplorer {
                path: "documents".to_string(),
            });
        }
        if lower.contains("desktop") {
            return Some(AppLaunchIntent::OpenExplorer {
                path: "desktop".to_string(),
            });
        }
        if lower.contains("picture") || lower.contains("gambar") || lower.contains("foto") {
            return Some(AppLaunchIntent::OpenExplorer {
                path: "pictures".to_string(),
            });
        }
        if lower.contains("video") {
            return Some(AppLaunchIntent::OpenExplorer {
                path: "videos".to_string(),
            });
        }
        if lower.contains("music") || lower.contains("musik") || lower.contains("lagu") {
            return Some(AppLaunchIntent::OpenExplorer {
                path: "music".to_string(),
            });
        }
    }

    // Direct folder names without prefix
    if lower == "downloads" || lower == "download" || lower == "unduhan" {
        return Some(AppLaunchIntent::OpenExplorer {
            path: "downloads".to_string(),
        });
    }
    if lower == "documents" || lower == "dokumen" {
        return Some(AppLaunchIntent::OpenExplorer {
            path: "documents".to_string(),
        });
    }

    // 5. Open App with typos / aliases
    // Ambiguous targets (e.g. "note", "catatan", "browser", "terminal", "office")
    // must NOT be eagerly launched. They require Groq AI clarification!
    if is_ambiguous_app_target(&lower) {
        return None;
    }

    // Direct alias match without prefix: "wa", "dc", "spoti", "wrod", "eksel", "tele", "kalkulator"
    if let Some(alias) = resolve_app_alias(&lower) {
        return Some(AppLaunchIntent::OpenApp {
            name: alias.search_keyword.to_string(),
            friendly_name: alias.friendly_name.to_string(),
        });
    }

    // Prefixed match: "tolong buka wa", "buka wrod", "jalankan excel", "open spotify"
    let prefixes = [
        "tolong buka ",
        "buka ",
        "open ",
        "jalankan ",
        "launch ",
        "start ",
        "nyalakan ",
        "bisa buka ",
        "tolong jalankan ",
    ];

    for prefix in &prefixes {
        if lower.starts_with(prefix) {
            let mut target = lower[prefix.len()..].trim();
            // Strip trailing conversational words
            for suffix in &[" dong", " ya", " please", " tolong", " sekarang"] {
                if target.ends_with(suffix) {
                    target = target[..target.len() - suffix.len()].trim();
                }
            }

            if target.is_empty() {
                continue;
            }

            // Ambiguous targets (e.g. "note", "catatan", "browser", "terminal", "office")
            // must NOT be eagerly launched. Let Groq AI ask for clarification!
            if is_ambiguous_app_target(target) {
                return None;
            }

            // If target matches known alias or generic app
            let (_, _, _, friendly) = resolve_or_generic(target);
            return Some(AppLaunchIntent::OpenApp {
                name: target.to_string(),
                friendly_name: friendly,
            });
        }
    }

    None
}

/// Resolves standard Windows folders or absolute/relative paths
pub fn resolve_folder_path(hint: &str) -> PathBuf {
    let lower = hint.trim().to_lowercase();
    let user_profile = std::env::var("USERPROFILE").unwrap_or_default();
    let base = PathBuf::from(&user_profile);

    if lower.contains("download") || lower.contains("unduhan") {
        base.join("Downloads")
    } else if lower.contains("document") || lower.contains("dokumen") {
        base.join("Documents")
    } else if lower.contains("desktop") {
        base.join("Desktop")
    } else if lower.contains("picture") || lower.contains("gambar") || lower.contains("foto") {
        base.join("Pictures")
    } else if lower.contains("video") {
        base.join("Videos")
    } else if lower.contains("music") || lower.contains("musik") || lower.contains("lagu") {
        base.join("Music")
    } else {
        let clean = hint.trim();
        let p = PathBuf::from(clean);
        if p.is_absolute() {
            p
        } else if !user_profile.is_empty() {
            base.join(clean)
        } else {
            p
        }
    }
}

/// Creates a new text file and opens it in Windows Notepad
pub fn create_and_open_new_text_file(
    folder_hint: &str,
    file_name: Option<&str>,
) -> Result<PathBuf, String> {
    let folder = resolve_folder_path(folder_hint);
    if !folder.exists() {
        std::fs::create_dir_all(&folder)
            .map_err(|e| format!("Gagal membuat direktori {:?}: {}", folder, e))?;
    }

    let raw_name = file_name.unwrap_or("Catatan_Baru.txt").trim();
    let clean_name = if !raw_name.ends_with(".txt") {
        format!("{}.txt", raw_name)
    } else {
        raw_name.to_string()
    };

    let mut file_path = folder.join(&clean_name);
    if file_path.exists() {
        let stem = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Catatan");
        for i in 2..100 {
            let candidate = folder.join(format!("{}_{}.txt", stem, i));
            if !candidate.exists() {
                file_path = candidate;
                break;
            }
        }
    }

    if !file_path.exists() {
        std::fs::write(&file_path, "").map_err(|e| format!("Gagal membuat file {:?}: {}", file_path, e))?;
    }

    // Launch Notepad asynchronously
    let wide_path: Vec<u16> = file_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let _ = ShellExecuteW(
            None,
            windows::core::w!("open"),
            windows::core::w!("notepad.exe"),
            PCWSTR(wide_path.as_ptr()),
            None,
            SW_SHOWNORMAL,
        );
    }

    Ok(file_path)
}

/// Opens Windows File Explorer at the target folder or path
pub fn open_explorer_path(folder_or_path: &str) {
    let resolved = resolve_folder_path(folder_or_path);
    let path_str = if resolved.exists() {
        resolved.to_string_lossy().to_string()
    } else {
        folder_or_path.to_string()
    };

    let wide_path: Vec<u16> = OsStr::new(&path_str)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        // Direct shell open for folders
        let res = ShellExecuteW(
            None,
            windows::core::w!("open"),
            PCWSTR(wide_path.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
        if (res.0 as usize) <= 32 {
            // Fallback: invoke explorer.exe with quoted path
            let quoted = format!("\"{}\"", path_str);
            let wide_param: Vec<u16> = quoted.encode_utf16().chain(std::iter::once(0)).collect();
            let _ = ShellExecuteW(
                None,
                windows::core::w!("open"),
                windows::core::w!("explorer.exe"),
                PCWSTR(wide_param.as_ptr()),
                None,
                SW_SHOWNORMAL,
            );
        }
    }
}

/// Launches a Chromium browser (Brave, Chrome, Edge) with a specific user profile
pub fn launch_browser_profile(browser: &str, profile: &str) {
    let exe = match browser.to_lowercase().as_str() {
        "chrome" | "krom" => windows::core::w!("chrome.exe"),
        "edge" | "msedge" | "edg" => windows::core::w!("msedge.exe"),
        _ => windows::core::w!("brave.exe"),
    };

    let param = format!("--profile-directory=\"{}\"", profile);
    let wide_param: Vec<u16> = param.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let _ = ShellExecuteW(
            None,
            windows::core::w!("open"),
            exe,
            PCWSTR(wide_param.as_ptr()),
            None,
            SW_SHOWNORMAL,
        );
    }
}

/// Instantly locks the Windows workstation
pub fn lock_workstation() -> bool {
    unsafe { LockWorkStation().as_bool() }
}

/// Activates an existing application window or launches its process/URI.
/// Fully decoupled from UI thread.
pub fn launch_or_activate_app(app_name: &str) {
    let clean = app_name.trim();
    if clean.is_empty() {
        return;
    }
    let lower = clean.to_lowercase();
    // Exclude internal widgets / system overlays
    if lower == "notification"
        || lower == "system"
        || lower == "shelf"
        || lower == "weather"
        || lower == "eyedropper"
        || lower == "focus timer"
        || lower == "gemini assistant"
        || lower == "todolist"
        || lower == "to-do"
        || lower == "calendar"
        || lower.contains("snip")
        || lower.contains("alat pemotong")
        || lower.contains("screensketch")
        || lower.contains("screenshot")
    {
        return;
    }

    let (search_keyword, exe_name, uri_scheme, _) = resolve_or_generic(clean);

    struct SearchContext {
        keyword: String,
        exe_name: String,
        my_pid: u32,
        found_hwnd: Option<HWND>,
    }

    let my_pid = unsafe { windows::Win32::System::Threading::GetCurrentProcessId() };

    let mut ctx = SearchContext {
        keyword: search_keyword,
        exe_name,
        my_pid,
        found_hwnd: None,
    };

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut SearchContext) };

        if !unsafe { IsWindowVisible(hwnd).as_bool() } {
            return BOOL(1);
        }

        let style = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowLongW(hwnd, GWL_STYLE) as u32 };
        if (style & WS_CHILD.0) != 0 {
            return BOOL(1);
        }

        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid == 0 || pid == ctx.my_pid {
            return BOOL(1);
        }

        if unsafe { IsHungAppWindow(hwnd).as_bool() } {
            return BOOL(1);
        }

        let mut rect = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut rect).is_ok() } {
            if rect.right - rect.left <= 0 || rect.bottom - rect.top <= 0 {
                return BOOL(1);
            }
        }

        // 1. Check window text / title
        let mut text_buf = [0u16; 512];
        let len = unsafe { GetWindowTextW(hwnd, &mut text_buf) };
        if len > 0 {
            let title = String::from_utf16_lossy(&text_buf[..len as usize]).to_lowercase();
            if title.contains(&ctx.keyword) {
                ctx.found_hwnd = Some(hwnd);
                return BOOL(0);
            }
        }

        // 2. Check executable image path
        if let Ok(h_proc) = unsafe {
            windows::Win32::System::Threading::OpenProcess(
                windows::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                pid,
            )
        } {
            let mut img_buf = [0u16; 1024];
            let mut size = img_buf.len() as u32;
            if unsafe {
                windows::Win32::System::Threading::QueryFullProcessImageNameW(
                    h_proc,
                    windows::Win32::System::Threading::PROCESS_NAME_FORMAT(0),
                    PWSTR(img_buf.as_mut_ptr()),
                    &mut size,
                )
            }.is_ok() && size > 0 {
                let path = String::from_utf16_lossy(&img_buf[..size as usize]).to_lowercase();
                if path.contains(&ctx.keyword) || path.ends_with(&ctx.exe_name) {
                    ctx.found_hwnd = Some(hwnd);
                    let _ = unsafe { windows::Win32::Foundation::CloseHandle(h_proc) };
                    return BOOL(0);
                }
            }
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(h_proc) };
        }

        BOOL(1)
    }

    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut ctx as *mut _ as isize));
    }

    if let Some(hwnd) = ctx.found_hwnd {
        unsafe {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            } else {
                let _ = ShowWindow(hwnd, SW_SHOW);
            }
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(hwnd);
        }
    } else {
        // App is not running: launch via URI scheme if available, or invoke executable directly
        let mut launched = false;
        if let Some(uri) = uri_scheme {
            let wide_uri: Vec<u16> = uri.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe {
                let res = ShellExecuteW(
                    None,
                    windows::core::w!("open"),
                    PCWSTR(wide_uri.as_ptr()),
                    None,
                    None,
                    SW_SHOWNORMAL,
                );
                if (res.0 as usize) > 32 {
                    launched = true;
                }
            }
        }

        if !launched {
            let wide_exe: Vec<u16> = ctx.exe_name.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe {
                let _ = ShellExecuteW(
                    None,
                    windows::core::w!("open"),
                    PCWSTR(wide_exe.as_ptr()),
                    None,
                    None,
                    SW_SHOWNORMAL,
                );
            }
        }
    }
}

/// Asynchronous wrapper for `launch_or_activate_app`
pub fn launch_or_activate_app_async(app_name: String) {
    std::thread::Builder::new()
        .name("AppLauncherWorker".to_string())
        .spawn(move || {
            launch_or_activate_app(&app_name);
        })
        .ok();
}

/// Asynchronous wrapper for `launch_browser_profile`
pub fn launch_browser_profile_async(browser: String, profile: String) {
    std::thread::Builder::new()
        .name("BrowserProfileWorker".to_string())
        .spawn(move || {
            launch_browser_profile(&browser, &profile);
        })
        .ok();
}

/// Asynchronous wrapper for `open_explorer_path`
pub fn open_explorer_path_async(path: String) {
    std::thread::Builder::new()
        .name("ExplorerWorker".to_string())
        .spawn(move || {
            open_explorer_path(&path);
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_typos_and_aliases() {
        assert_eq!(
            resolve_app_alias("wa").unwrap().friendly_name,
            "WhatsApp"
        );
        assert_eq!(
            resolve_app_alias("dc").unwrap().friendly_name,
            "Discord"
        );
        assert_eq!(
            resolve_app_alias("wrod").unwrap().friendly_name,
            "Microsoft Word"
        );
        assert_eq!(
            resolve_app_alias("wodr").unwrap().friendly_name,
            "Microsoft Word"
        );
        assert_eq!(
            resolve_app_alias("eksel").unwrap().friendly_name,
            "Microsoft Excel"
        );
        assert_eq!(
            resolve_app_alias("spoti").unwrap().friendly_name,
            "Spotify"
        );
        assert_eq!(
            resolve_app_alias("tele").unwrap().friendly_name,
            "Telegram"
        );
        assert_eq!(
            resolve_app_alias("kalkulator").unwrap().friendly_name,
            "Kalkulator"
        );
        assert_eq!(
            resolve_app_alias("notepad").unwrap().friendly_name,
            "Notepad"
        );
        assert_eq!(
            resolve_app_alias("onenote").unwrap().friendly_name,
            "Microsoft OneNote"
        );
        assert_eq!(
            resolve_app_alias("sticky notes").unwrap().friendly_name,
            "Sticky Notes"
        );
    }

    #[test]
    fn test_is_ambiguous_app_target() {
        assert!(is_ambiguous_app_target("note"));
        assert!(is_ambiguous_app_target("notes"));
        assert!(is_ambiguous_app_target("catatan"));
        assert!(is_ambiguous_app_target("browser"));
        assert!(is_ambiguous_app_target("peramban"));
        assert!(is_ambiguous_app_target("terminal"));
        assert!(is_ambiguous_app_target("office"));

        assert!(!is_ambiguous_app_target("notepad"));
        assert!(!is_ambiguous_app_target("onenote"));
        assert!(!is_ambiguous_app_target("sticky notes"));
        assert!(!is_ambiguous_app_target("chrome"));
        assert!(!is_ambiguous_app_target("brave"));
        assert!(!is_ambiguous_app_target("wa"));
        assert!(!is_ambiguous_app_target("discord"));
    }

    #[test]
    fn test_extract_launch_intent() {
        assert_eq!(
            extract_launch_intent("tolong buka wrod"),
            Some(AppLaunchIntent::OpenApp {
                name: "wrod".to_string(),
                friendly_name: "Microsoft Word".to_string(),
            })
        );

        assert_eq!(
            extract_launch_intent("Tolong buka wa ya"),
            Some(AppLaunchIntent::OpenApp {
                name: "wa".to_string(),
                friendly_name: "WhatsApp".to_string(),
            })
        );

        assert_eq!(
            extract_launch_intent("tolong buka DC dong"),
            Some(AppLaunchIntent::OpenApp {
                name: "dc".to_string(),
                friendly_name: "Discord".to_string(),
            })
        );

        assert_eq!(
            extract_launch_intent("buka spoti"),
            Some(AppLaunchIntent::OpenApp {
                name: "spoti".to_string(),
                friendly_name: "Spotify".to_string(),
            })
        );

        assert_eq!(
            extract_launch_intent("lock pc"),
            Some(AppLaunchIntent::LockWorkstation)
        );

        assert_eq!(
            extract_launch_intent("kunci komputer"),
            Some(AppLaunchIntent::LockWorkstation)
        );

        assert_eq!(
            extract_launch_intent("buka downloads"),
            Some(AppLaunchIntent::OpenExplorer {
                path: "downloads".to_string(),
            })
        );

        assert_eq!(
            extract_launch_intent("tolong buka brave profil A"),
            Some(AppLaunchIntent::OpenBrowserProfile {
                browser: "brave".to_string(),
                profile: "a".to_string(),
            })
        );

        assert_eq!(
            extract_launch_intent("tolong bikin txt baru di folder A"),
            Some(AppLaunchIntent::CreateNewFile {
                folder: "a".to_string(),
                filename: "Catatan_Baru.txt".to_string(),
            })
        );

        // Ambiguous queries must return None so Groq AI asks for clarification
        assert_eq!(extract_launch_intent("buka Note"), None);
        assert_eq!(extract_launch_intent("tolong buka catatan dong"), None);
        assert_eq!(extract_launch_intent("buka browser"), None);
        assert_eq!(extract_launch_intent("buka terminal"), None);

        // Specific queries must be handled fast offline
        assert_eq!(
            extract_launch_intent("buka notepad"),
            Some(AppLaunchIntent::OpenApp {
                name: "notepad".to_string(),
                friendly_name: "Notepad".to_string(),
            })
        );
        assert_eq!(
            extract_launch_intent("buka onenote"),
            Some(AppLaunchIntent::OpenApp {
                name: "onenote".to_string(),
                friendly_name: "Microsoft OneNote".to_string(),
            })
        );
        assert_eq!(
            extract_launch_intent("buka sticky notes"),
            Some(AppLaunchIntent::OpenApp {
                name: "sticky notes".to_string(),
                friendly_name: "Sticky Notes".to_string(),
            })
        );

        assert_eq!(
            extract_launch_intent("buka folder download"),
            Some(AppLaunchIntent::OpenExplorer {
                path: "download".to_string(),
            })
        );

        assert_eq!(
            extract_launch_intent("buka download"),
            Some(AppLaunchIntent::OpenExplorer {
                path: "downloads".to_string(),
            })
        );

        // General chat should not be matched
        assert_eq!(
            extract_launch_intent("siapa penemu bahasa rust?"),
            None
        );
    }
}
