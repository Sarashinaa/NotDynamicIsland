//! App launcher and Windows automation engine with typo tolerance, alias resolution,
//! process decoupling, and natural language command parsing.
//! 0.0% idle CPU, zero heavyweight GUI dependencies, purely Win32 API.

use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
    KEY_READ,
};
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
    (
        &["steam", "stim"],
        AppAlias {
            search_keyword: "steam",
            executable: "steam.exe",
            uri_scheme: Some("steam:"),
            friendly_name: "Steam",
        },
    ),
    (
        &["anydesk", "any desk", "anidesk"],
        AppAlias {
            search_keyword: "anydesk",
            executable: "anydesk.exe",
            uri_scheme: None,
            friendly_name: "AnyDesk",
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

/// Searches Windows Start Menu shortcut directories for an installed application matching query
pub fn find_start_menu_shortcut(query: &str) -> Option<PathBuf> {
    let lower_query = query.trim().to_lowercase();
    if lower_query.is_empty() {
        return None;
    }
    let query_norm = lower_query.replace(' ', "").replace('-', "").replace('_', "");

    let mut search_dirs = Vec::new();
    if let Ok(prog_data) = std::env::var("ProgramData") {
        search_dirs.push(
            PathBuf::from(prog_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }
    if let Ok(app_data) = std::env::var("APPDATA") {
        search_dirs.push(
            PathBuf::from(app_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }

    let mut exact_match: Option<PathBuf> = None;
    let mut starts_match: Option<PathBuf> = None;
    let mut contains_match: Option<PathBuf> = None;

    for base_dir in search_dirs {
        if !base_dir.exists() {
            continue;
        }
        collect_lnk_recursive(
            &base_dir,
            &lower_query,
            &query_norm,
            &mut exact_match,
            &mut starts_match,
            &mut contains_match,
        );
        if exact_match.is_some() {
            return exact_match;
        }
    }

    exact_match.or(starts_match).or(contains_match)
}

fn collect_lnk_recursive(
    dir: &std::path::Path,
    query: &str,
    query_norm: &str,
    exact_match: &mut Option<PathBuf>,
    starts_match: &mut Option<PathBuf>,
    contains_match: &mut Option<PathBuf>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_lnk_recursive(
                &path,
                query,
                query_norm,
                exact_match,
                starts_match,
                contains_match,
            );
            if exact_match.is_some() {
                return;
            }
        } else if path
            .extension()
            .and_then(|s| s.to_str())
            .map(|e| e.eq_ignore_ascii_case("lnk"))
            .unwrap_or(false)
        {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                let lower_stem = stem.to_lowercase();
                // Ignore uninstallation and documentation shortcuts
                if lower_stem.contains("uninstall")
                    || lower_stem.contains("help")
                    || lower_stem.contains("readme")
                    || lower_stem.contains("documentation")
                    || lower_stem.contains("website")
                    || lower_stem.contains("license")
                {
                    continue;
                }

                let stem_norm = lower_stem.replace(' ', "").replace('-', "").replace('_', "");

                if lower_stem == query || stem_norm == query_norm {
                    *exact_match = Some(path.clone());
                    return;
                }
                if starts_match.is_none()
                    && (lower_stem.starts_with(query) || stem_norm.starts_with(query_norm))
                {
                    *starts_match = Some(path.clone());
                } else if contains_match.is_none()
                    && (lower_stem.contains(query) || stem_norm.contains(query_norm))
                {
                    *contains_match = Some(path.clone());
                }
            }
        }
    }
}

/// Searches Windows Registry App Paths for registered executables
pub fn find_registry_app_path(query: &str) -> Option<PathBuf> {
    let clean = query.trim().trim_end_matches(".exe");
    let exe_name = format!("{}.exe", clean);
    let subkey = format!(
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{}",
        exe_name
    );
    let wide_subkey: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();

    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let mut hkey = HKEY::default();
        if unsafe { RegOpenKeyExW(root, PCWSTR(wide_subkey.as_ptr()), 0, KEY_READ, &mut hkey) }
            .is_ok()
        {
            let mut buf = [0u16; 1024];
            let mut buf_size = (buf.len() * 2) as u32;
            let res = unsafe {
                RegQueryValueExW(
                    hkey,
                    PCWSTR::null(),
                    None,
                    None,
                    Some(buf.as_mut_ptr() as *mut _),
                    Some(&mut buf_size),
                )
            };
            let _ = unsafe { RegCloseKey(hkey) };
            if res.is_ok() && buf_size > 2 {
                let char_len = ((buf_size / 2) as usize).saturating_sub(1);
                let path_str = String::from_utf16_lossy(&buf[..char_len]);
                let p = PathBuf::from(path_str.trim().trim_matches('"'));
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn resolve_system_root() -> PathBuf {
    std::env::var("SystemRoot")
        .or_else(|_| std::env::var("windir"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(r"C:\Windows"))
}

fn enumerate_secondary_fixed_drives() -> Vec<PathBuf> {
    let sys_letter = std::env::var("SystemDrive")
        .ok()
        .and_then(|s| s.chars().next())
        .map(|c| c.to_ascii_uppercase())
        .unwrap_or('C');
    let mut drives = Vec::new();
    let mask = unsafe { GetLogicalDrives() };
    for bit in 0..26u32 {
        if (mask & (1 << bit)) != 0 {
            let letter = (b'A' + bit as u8) as char;
            if letter == sys_letter {
                continue;
            }
            let root = format!("{}:\\", letter);
            let root_w: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
            if unsafe { GetDriveTypeW(PCWSTR(root_w.as_ptr())) } == 3 {
                drives.push(PathBuf::from(root));
            }
        }
    }
    drives
}

pub fn sanitize_browser_profile(profile: &str) -> String {
    let clean: String = profile
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
        .take(64)
        .collect();
    let trimmed = clean.trim_matches(|c: char| c == ' ' || c == '-');
    if trimmed.is_empty() {
        "Default".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn sanitize_text_filename(raw_name: &str) -> String {
    let base = raw_name
        .trim()
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("");
    let cleaned: String = base
        .chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
        .collect();
    let no_traversal = cleaned.replace("..", "");
    let trimmed = no_traversal.trim_matches(|c: char| c == ' ' || c == '.');
    if trimmed.is_empty() {
        "Catatan_Baru.txt".to_string()
    } else if trimmed.to_ascii_lowercase().ends_with(".txt") {
        trimmed.to_string()
    } else {
        format!("{}.txt", trimmed)
    }
}

/// Searches common installation roots across system and secondary fixed drives
pub fn find_common_install_path(query: &str) -> Option<PathBuf> {
    let clean = query.trim().trim_end_matches(".exe");
    let exe_name = format!("{}.exe", clean);

    let mut candidate_dirs = Vec::new();

    for drive in enumerate_secondary_fixed_drives() {
        candidate_dirs.push(drive.join("Programs").join(clean));
        candidate_dirs.push(drive.join("Program Files").join(clean));
        candidate_dirs.push(drive.join("Program Files (x86)").join(clean));
    }

    if let Ok(pf) = std::env::var("ProgramFiles") {
        candidate_dirs.push(PathBuf::from(&pf).join(clean));
    }
    if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
        candidate_dirs.push(PathBuf::from(&pf86).join(clean));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        candidate_dirs.push(PathBuf::from(&local).join("Programs").join(clean));
        candidate_dirs.push(PathBuf::from(&local).join(clean));
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        candidate_dirs.push(PathBuf::from(&appdata).join(clean));
    }

    for dir in candidate_dirs {
        let direct_exe = dir.join(&exe_name);
        if direct_exe.exists() {
            return Some(direct_exe);
        }
        if dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.extension()
                        .map(|e| e.eq_ignore_ascii_case("exe"))
                        .unwrap_or(false)
                    {
                        if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                            if stem.eq_ignore_ascii_case(clean) {
                                return Some(p);
                            }
                        }
                    }
                }
            }
        }
    }
    None
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

    // 4. Explicit Windows drive path anywhere in prompt (e.g. "C:\Projects\demo", "ada di C:\...", "buka C:\Projects")
    if let Some(drive_idx) = prompt.find(":\\").or_else(|| prompt.find(":/")) {
        if drive_idx > 0 {
            let before = &prompt[..drive_idx];
            if let Some(drive_letter) = before.chars().last() {
                if drive_letter.is_ascii_alphabetic() {
                    let start = drive_idx - 1;
                    let mut candidate = prompt[start..].trim();
                    for suffix in &[" dong", " ya", " please", " tolong", " sekarang"] {
                        if candidate.to_lowercase().ends_with(suffix) {
                            candidate = candidate[..candidate.len() - suffix.len()].trim();
                        }
                    }
                    if !candidate.is_empty() {
                        return Some(AppLaunchIntent::OpenExplorer {
                            path: candidate.to_string(),
                        });
                    }
                }
            }
        }
    }

    // 5. Open specific explorer folder
    // Check for flexible folder commands (e.g. "minta buka folder downloads", "buka downloads", "bukain unduhan")
    let has_open_folder_verb = lower.contains("buka")
        || lower.contains("open")
        || lower.contains("lihat")
        || lower.contains("masuk")
        || lower.contains("tampilkan")
        || lower.contains("folder")
        || lower.contains("direktori")
        || lower.contains("bukain")
        || lower.contains("minta")
        || lower.contains("tolong");

    if has_open_folder_verb
        || lower == "downloads"
        || lower == "download"
        || lower == "unduhan"
        || lower == "documents"
        || lower == "dokumen"
        || lower == "screenshots"
        || lower == "screenshot"
        || lower == "tangkapan layar"
    {
        if lower.contains("download") || lower.contains("unduhan") {
            return Some(AppLaunchIntent::OpenExplorer {
                path: "downloads".to_string(),
            });
        }
        if lower.contains("screenshot") || lower.contains("tangkapan layar") {
            return Some(AppLaunchIntent::OpenExplorer {
                path: "screenshots".to_string(),
            });
        }
        if lower.contains("document") || lower.contains("dokumen") {
            return Some(AppLaunchIntent::OpenExplorer {
                path: "documents".to_string(),
            });
        }
        if lower.contains("desktop") && !lower.contains("txt") && !lower.contains("file") {
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

    // Explicit folder path: "folder ...", "direktori ..."
    if let Some(idx) = lower.find("folder ").or_else(|| lower.find("direktori ")) {
        let skip = if lower[idx..].starts_with("folder ") { 7 } else { 10 };
        let mut target = lower[idx + skip..].trim();
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

    // 5. Open App with typos / aliases / universal discovery
    // Ambiguous targets (e.g. "note", "catatan", "browser", "terminal", "office")
    // must NOT be eagerly launched. They require Groq AI clarification!
    if is_ambiguous_app_target(&lower) {
        return None;
    }

    // Direct alias match without prefix: "wa", "dc", "spoti", "wrod", "eksel", "tele", "kalkulator", "steam", "anydesk"
    if let Some(alias) = resolve_app_alias(&lower) {
        return Some(AppLaunchIntent::OpenApp {
            name: alias.search_keyword.to_string(),
            friendly_name: alias.friendly_name.to_string(),
        });
    }

    // Prefixed match with broad Indonesian and English conversational phrases:
    let prefixes = [
        "minta tolong bukain ",
        "minta tolong buka ",
        "minta tolong jalankan ",
        "tolong bukain ",
        "tolong buka ",
        "tolong jalankan ",
        "minta bukain ",
        "minta buka ",
        "minta jalankan ",
        "coba bukain ",
        "coba buka ",
        "bisa bukain ",
        "bisa buka ",
        "mau buka ",
        "pengen buka ",
        "bukain ",
        "buka ",
        "open ",
        "jalankan ",
        "launch ",
        "start ",
        "nyalakan ",
        "hidupkan ",
    ];

    for prefix in &prefixes {
        if lower.starts_with(prefix) {
            let mut target = lower[prefix.len()..].trim();
            // Strip trailing conversational words
            for suffix in &[
                " dong", " ya", " please", " tolong", " sekarang", " ya tolong", " dulu",
                " nya", " deh", " bro", " gan", " min",
            ] {
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

    // 1. Standard special folders
    if lower.contains("download") || lower.contains("unduhan") {
        return base.join("Downloads");
    } else if lower.contains("screenshot") || lower.contains("tangkapan layar") {
        for candidate in [
            base.join("Pictures").join("Screenshots"),
            base.join("OneDrive").join("Pictures").join("Screenshots"),
            base.join("OneDrive").join("Gambar").join("Screenshots"),
            base.join("OneDrive").join("Gambar").join("Screenshot"),
            base.join("Screenshots"),
        ] {
            if candidate.exists() {
                return candidate;
            }
        }
        return base.join("Pictures").join("Screenshots");
    } else if lower.contains("document") || lower.contains("dokumen") {
        return base.join("Documents");
    } else if lower.contains("desktop") {
        return base.join("Desktop");
    } else if lower.contains("picture") || lower.contains("gambar") || lower.contains("foto") {
        return base.join("Pictures");
    } else if lower.contains("video") {
        return base.join("Videos");
    } else if lower.contains("music") || lower.contains("musik") || lower.contains("lagu") {
        return base.join("Music");
    }

    let clean = hint.trim();
    let p = PathBuf::from(clean);
    if p.is_absolute() {
        return p;
    }

    // 2. Search common user subfolders
    if !user_profile.is_empty() {
        let candidates = [
            base.join(clean),
            base.join("Pictures").join(clean),
            base.join("Documents").join(clean),
            base.join("Desktop").join(clean),
            base.join("Downloads").join(clean),
            base.join("Videos").join(clean),
            base.join("Music").join(clean),
            base.join("OneDrive").join(clean),
            base.join("OneDrive").join("Dokumen").join(clean),
            base.join("OneDrive").join("Gambar").join(clean),
        ];
        for candidate in candidates {
            if candidate.exists() {
                return candidate;
            }
        }

        // 3. Search secondary fixed drives and 1-level deep subdirectories
        for drive in enumerate_secondary_fixed_drives() {
            let direct = drive.join(clean);
            if direct.exists() {
                return direct;
            }
            let programs = drive.join("Programs").join(clean);
            if programs.exists() {
                return programs;
            }
            if let Ok(entries) = std::fs::read_dir(&drive) {
                for entry in entries.flatten() {
                    if let Ok(file_type) = entry.file_type() {
                        if file_type.is_dir() {
                            let candidate = entry.path().join(clean);
                            if candidate.exists() {
                                return candidate;
                            }
                        }
                    }
                }
            }
        }

        base.join(clean)
    } else {
        p
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

    let raw_name = file_name.unwrap_or("Catatan_Baru.txt");
    let clean_name = sanitize_text_filename(raw_name);

    let mut file_path = folder.join(&clean_name);
    if file_path.parent() != Some(folder.as_path()) {
        return Err("Nama file tidak valid".to_string());
    }
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

    // Launch Notepad asynchronously with explicit system path and quoted file argument
    let notepad_exe = resolve_system_root().join("System32").join("notepad.exe");
    let wide_notepad: Vec<u16> = notepad_exe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let quoted_arg = format!("\"{}\"", file_path.display());
    let wide_path: Vec<u16> = quoted_arg.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let _ = ShellExecuteW(
            None,
            windows::core::w!("open"),
            PCWSTR(wide_notepad.as_ptr()),
            PCWSTR(wide_path.as_ptr()),
            None,
            SW_SHOWNORMAL,
        );
    }

    Ok(file_path)
}

/// Opens Windows File Explorer at the target folder or path
pub fn open_explorer_path(folder_or_path: &str) -> bool {
    if folder_or_path.trim().starts_with("\\\\") {
        return false;
    }
    let resolved = resolve_folder_path(folder_or_path);
    if !resolved.exists() || !resolved.is_dir() {
        return false;
    }

    let wide_path: Vec<u16> = resolved
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let explorer_exe = resolve_system_root().join("explorer.exe");
    let wide_explorer: Vec<u16> = explorer_exe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        // Direct shell open for folders first (the official Windows shell way)
        let res = ShellExecuteW(
            None,
            windows::core::w!("open"),
            PCWSTR(wide_path.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
        if (res.0 as usize) <= 32 {
            // Fallback: launch %SystemRoot%\explorer.exe directly with target folder
            let res2 = ShellExecuteW(
                None,
                windows::core::w!("open"),
                PCWSTR(wide_explorer.as_ptr()),
                PCWSTR(wide_path.as_ptr()),
                None,
                SW_SHOWNORMAL,
            );
            (res2.0 as usize) > 32
        } else {
            true
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

    let clean_profile = sanitize_browser_profile(profile);
    let param = format!("--profile-directory=\"{}\"", clean_profile);
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
        // App is not running: launch via multi-tier discovery
        let mut launched = false;

        // 1. Try Windows Start Menu shortcut (.lnk) - covers almost all installed desktop apps
        if let Some(lnk_path) = find_start_menu_shortcut(clean) {
            let wide_lnk: Vec<u16> = lnk_path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            unsafe {
                let res = ShellExecuteW(
                    None,
                    windows::core::w!("open"),
                    PCWSTR(wide_lnk.as_ptr()),
                    None,
                    None,
                    SW_SHOWNORMAL,
                );
                if (res.0 as usize) > 32 {
                    launched = true;
                }
            }
        }

        // 2. Try Windows Registry App Paths
        if !launched {
            if let Some(reg_exe) = find_registry_app_path(clean) {
                let wide_exe: Vec<u16> = reg_exe
                    .as_os_str()
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                unsafe {
                    let res = ShellExecuteW(
                        None,
                        windows::core::w!("open"),
                        PCWSTR(wide_exe.as_ptr()),
                        None,
                        None,
                        SW_SHOWNORMAL,
                    );
                    if (res.0 as usize) > 32 {
                        launched = true;
                    }
                }
            }
        }

        // 3. Try Common Program File Paths across drives (C:, D:)
        if !launched {
            if let Some(dir_exe) = find_common_install_path(clean) {
                let wide_exe: Vec<u16> = dir_exe
                    .as_os_str()
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                unsafe {
                    let res = ShellExecuteW(
                        None,
                        windows::core::w!("open"),
                        PCWSTR(wide_exe.as_ptr()),
                        None,
                        None,
                        SW_SHOWNORMAL,
                    );
                    if (res.0 as usize) > 32 {
                        launched = true;
                    }
                }
            }
        }

        // 4. Try known URI scheme (e.g. "steam:", "spotify:", "whatsapp:", etc.)
        if !launched {
            let target_uri = uri_scheme.map(|s| s.to_string()).or_else(|| {
                if clean.eq_ignore_ascii_case("steam") {
                    Some("steam:".to_string())
                } else if clean.eq_ignore_ascii_case("spotify") {
                    Some("spotify:".to_string())
                } else {
                    None
                }
            });
            if let Some(uri) = target_uri {
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
        }

        // 5. Fallback: ShellExecuteW direct executable name (for tools in PATH / Windows system32)
        if !launched
            && !ctx.exe_name.starts_with("\\\\")
            && !ctx.exe_name.contains("..")
            && !ctx.exe_name.contains("://")
        {
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
        assert_eq!(
            resolve_app_alias("steam").unwrap().friendly_name,
            "Steam"
        );
        assert_eq!(
            resolve_app_alias("anydesk").unwrap().friendly_name,
            "AnyDesk"
        );
    }

    #[test]
    fn test_find_start_menu_shortcuts_real() {
        assert!(find_start_menu_shortcut("nonexistent_app_xyz_999").is_none());
        let _ = find_start_menu_shortcut("notepad");
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
                path: "downloads".to_string(),
            })
        );

        assert_eq!(
            extract_launch_intent("buka download"),
            Some(AppLaunchIntent::OpenExplorer {
                path: "downloads".to_string(),
            })
        );

        assert_eq!(
            extract_launch_intent("minta buka steam"),
            Some(AppLaunchIntent::OpenApp {
                name: "steam".to_string(),
                friendly_name: "Steam".to_string(),
            })
        );

        assert_eq!(
            extract_launch_intent("minta buka anydesk"),
            Some(AppLaunchIntent::OpenApp {
                name: "anydesk".to_string(),
                friendly_name: "AnyDesk".to_string(),
            })
        );

        assert_eq!(
            extract_launch_intent("minta buka folder downloads"),
            Some(AppLaunchIntent::OpenExplorer {
                path: "downloads".to_string(),
            })
        );

        assert_eq!(
            extract_launch_intent("tolong bukain downloads dong"),
            Some(AppLaunchIntent::OpenExplorer {
                path: "downloads".to_string(),
            })
        );

        assert_eq!(
            extract_launch_intent("buka folder Screenshots"),
            Some(AppLaunchIntent::OpenExplorer {
                path: "screenshots".to_string(),
            })
        );

        assert_eq!(
            extract_launch_intent("tolong buka screenshot"),
            Some(AppLaunchIntent::OpenExplorer {
                path: "screenshots".to_string(),
            })
        );

        assert_eq!(
            extract_launch_intent(r"ada di C:\Projects\demo"),
            Some(AppLaunchIntent::OpenExplorer {
                path: r"C:\Projects\demo".to_string(),
            })
        );

        // General chat should not be matched
        assert_eq!(
            extract_launch_intent("siapa penemu bahasa rust?"),
            None
        );
    }

    #[test]
    fn test_resolve_folder_path() {
        let screenshots = resolve_folder_path("screenshots");
        assert!(screenshots.to_string_lossy().to_lowercase().contains("screenshot"));
        let downloads = resolve_folder_path("downloads");
        assert!(downloads.to_string_lossy().to_lowercase().contains("downloads"));
    }

    #[test]
    fn test_sanitize_browser_profile() {
        assert_eq!(sanitize_browser_profile("Profile 1"), "Profile 1");
        assert_eq!(
            sanitize_browser_profile("Default\" --gpu-launcher=calc"),
            "Default --gpu-launchercalc"
        );
        assert_eq!(sanitize_browser_profile("---\"\"---"), "Default");
    }

    #[test]
    fn test_sanitize_text_filename() {
        assert_eq!(sanitize_text_filename("notes"), "notes.txt");
        assert_eq!(sanitize_text_filename(r"..\..\evil.bat"), "evil.bat.txt");
        assert_eq!(sanitize_text_filename("C:\\Windows\\System32\\cmd"), "cmd.txt");
        assert_eq!(sanitize_text_filename("..."), "Catatan_Baru.txt");
    }
}
