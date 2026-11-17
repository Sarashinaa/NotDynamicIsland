//! AI Assistant integration using Groq Cloud API (Free Tier LPU) with fallback to Gemini CLI
//! Zero external runtime, 0.0% idle CPU, native WinHttp HTTPS execution.

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Networking::WinHttp::*;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

pub const WM_APP_ASSISTANT_REPLY: u32 = 0x8015;
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Clone, Debug, PartialEq)]
pub struct ChatMessage {
    pub is_user: bool,
    pub text: String,
    pub timestamp: Instant,
}

#[derive(Clone, Debug)]
pub struct AssistantState {
    pub input: crate::ui::TextInput,
    pub is_input_active: bool,
    pub is_thinking: bool,
    pub last_prompt: String,
    pub last_response: String,
    pub error_message: Option<String>,
    pub messages: Vec<ChatMessage>,
    #[allow(dead_code)]
    pub thinking_anim: f32,
    pub scroll_offset: usize,
    pub max_scroll: usize,
    pub is_groq: bool,
}

impl Default for AssistantState {
    fn default() -> Self {
        Self {
            input: crate::ui::TextInput::new(500),
            is_input_active: false,
            is_thinking: false,
            last_prompt: String::new(),
            last_response: String::new(),
            error_message: None,
            messages: Vec::new(),
            thinking_anim: 0.0,
            scroll_offset: 0,
            max_scroll: 0,
            is_groq: has_groq_key(),
        }
    }
}

#[allow(dead_code)]
impl AssistantState {
    pub fn input_char(&mut self, c: char) {
        self.input.insert_char(c);
    }

    pub fn input_backspace(&mut self) {
        self.input.backspace();
    }

    pub fn paste_text(&mut self, text: &str) {
        self.input.insert_text(text);
    }

    pub fn start_input(&mut self) {
        self.is_input_active = true;
    }

    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    pub fn scroll_input_left(&mut self) {
        self.input.scroll_offset = (self.input.scroll_offset - 20.0).max(0.0);
    }

    pub fn scroll_input_right(&mut self) {
        self.input.scroll_offset += 20.0;
    }

    pub fn scroll_down(&mut self) {
        if self.scroll_offset < self.max_scroll {
            self.scroll_offset += 1;
        }
    }

    pub fn commit_prompt(&mut self) -> Option<String> {
        let trimmed = self.input.text.trim().to_string();
        if trimmed.is_empty() {
            return None;
        }
        self.input.clear();
        self.is_input_active = false;
        self.last_prompt = trimmed.clone();
        self.is_thinking = true;
        self.error_message = None;
        self.scroll_offset = 0;
        self.max_scroll = 0;
        self.messages.push(ChatMessage {
            is_user: true,
            text: trimmed.clone(),
            timestamp: Instant::now(),
        });
        Some(trimmed)
    }

    pub fn clear(&mut self) {
        self.input.clear();
        self.messages.clear();
        self.last_prompt.clear();
        self.last_response.clear();
        self.error_message = None;
        self.is_thinking = false;
        self.scroll_offset = 0;
        self.max_scroll = 0;
    }

    pub fn on_reply(&mut self, response: String, error: Option<String>) {
        self.is_thinking = false;
        self.scroll_offset = 0;
        self.is_groq = has_groq_key();
        if let Some(err) = error {
            self.error_message = Some(err.clone());
            self.last_response = err;
        } else {
            self.error_message = None;
            self.last_response = response.clone();
            self.messages.push(ChatMessage {
                is_user: false,
                text: response,
                timestamp: Instant::now(),
            });
        }
    }
}

pub struct AssistantManager {
    state: Arc<Mutex<AssistantState>>,
    pending_reply: Arc<Mutex<Option<(String, Option<String>)>>>,
}

impl AssistantManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(AssistantState::default())),
            pending_reply: Arc::new(Mutex::new(None)),
        }
    }

    #[allow(dead_code)]
    pub fn get_state(&self) -> AssistantState {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn submit_prompt(&self, prompt: String, history: Vec<ChatMessage>, hwnd: HWND) {
        let trimmed = prompt.trim().to_string();
        if trimmed.is_empty() {
            return;
        }

        // Add user message to state and set thinking flag
        if let Ok(mut s) = self.state.lock() {
            s.is_thinking = true;
            s.last_prompt = trimmed.clone();
            s.error_message = None;
            s.scroll_offset = 0;
            s.max_scroll = 0;
            s.is_groq = has_groq_key();
            s.messages.push(ChatMessage {
                is_user: true,
                text: trimmed.clone(),
                timestamp: Instant::now(),
            });
        }

        let pending = Arc::clone(&self.pending_reply);
        let hwnd_val = hwnd.0 as usize;

        thread::Builder::new()
            .name("AssistantWorker".to_string())
            .spawn(move || {
                let hwnd = HWND(hwnd_val as *mut _);
                let (response, error) = execute_query(&trimmed, &history);

                if let Ok(mut p) = pending.lock() {
                    *p = Some((response, error));
                }

                unsafe {
                    let _ = PostMessageW(
                        hwnd,
                        WM_APP_ASSISTANT_REPLY,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            })
            .ok();
    }

    pub fn take_reply(&self) -> Option<(String, Option<String>)> {
        if let Ok(mut p) = self.pending_reply.lock() {
            p.take()
        } else {
            None
        }
    }
}

/// Checks if Groq API key is present either via file or env var
pub fn has_groq_key() -> bool {
    load_groq_api_key().is_some()
}

/// Discovers Groq API key from environment or local config files
pub fn load_groq_api_key() -> Option<String> {
    if let Ok(key) = std::env::var("GROQ_API_KEY") {
        let trimmed = key.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }

    let search_paths = [
        std::path::PathBuf::from("groq_key.txt"),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("groq_key.txt")))
            .unwrap_or_default(),
        std::env::var("APPDATA")
            .map(|a| std::path::PathBuf::from(a).join("NotDynamicIsland").join("groq_key.txt"))
            .unwrap_or_default(),
        std::env::var("USERPROFILE")
            .map(|u| std::path::PathBuf::from(u).join(".groq").join("api_key"))
            .unwrap_or_default(),
        std::env::var("USERPROFILE")
            .map(|u| std::path::PathBuf::from(u).join(".gemini").join("groq_api_key.txt"))
            .unwrap_or_default(),
    ];

    for path in &search_paths {
        if path.as_os_str().is_empty() {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(path) {
            let trimmed = content.trim().to_string();
            if !trimmed.is_empty() && trimmed.starts_with("gsk_") {
                return Some(trimmed);
            }
        }
    }

    None
}

/// Helper to escape characters for valid JSON strings
pub fn escape_json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// Dispatches query to Groq (if key available) or falls back to Gemini CLI
fn execute_query(prompt: &str, history: &[ChatMessage]) -> (String, Option<String>) {
    if let Some(groq_key) = load_groq_api_key() {
        match execute_groq_query(&groq_key, history, prompt) {
            Ok(resp) => return (clean_markdown_for_display(&resp), None),
            Err(e) => {
                // If Groq fails, return friendly error
                return (
                    String::new(),
                    Some(format!("⚠️ Groq API: {}. Silakan coba sesaat lagi.", e)),
                );
            }
        }
    }

    // Fallback: Gemini CLI
    let (resp, err) = execute_gemini_query(history, prompt);
    (clean_markdown_for_display(&resp), err)
}

/// Native WinHttp HTTPS POST to Groq Cloud API (Free Tier LPU) with multi-turn history & disambiguation
fn execute_groq_query(api_key: &str, history: &[ChatMessage], prompt: &str) -> Result<String, String> {
    let host = "api.groq.com";
    let path = "/openai/v1/chat/completions";

    let host_w: Vec<u16> = host.encode_utf16().chain(std::iter::once(0)).collect();
    let path_w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

    let system_content = "Kamu adalah asisten AI Dynamic Island Windows. Berikan jawaban padat, akurat, to-the-point, dan ramah dalam 1-3 baris dalam Bahasa Indonesia. Pahami typo, singkatan, slang, dan berbagai variasi perintah bahasa Indonesia (contoh: minta buka, tolong bukain, coba buka, buka, jalankan; wa->whatsapp, dc->discord, wrod/word->winword, eksel->excel, spoti->spotify, tele->telegram, kalkulator->calc, krom->chrome, brv->brave, wt->terminal, cmd->cmd, pwsh->powershell, steam, anydesk, vlc, obs, figma, dll).\\n\\n[KLARIFIKASI PENTING]: Jika pengguna meminta untuk membuka sesuatu yang tidak spesifik, ambigu, atau memiliki banyak alternatif di Windows (contoh: 'note' / 'catatan' bisa Notepad, OneNote, atau Sticky Notes; 'browser' bisa Chrome, Brave, atau Edge; 'terminal' bisa Windows Terminal, PowerShell, atau CMD; 'office' bisa Word, Excel, atau PowerPoint):\\nJANGAN langsung membuka dan JANGAN keluarkan tag aksi! Tanyakan klarifikasi dengan ramah dan sebutkan 2-3 pilihan aplikasi tersebut secara singkat (contoh: 'Ada beberapa aplikasi catatan: Notepad, OneNote, atau Sticky Notes. Yang mana yang ingin dibuka?').\\n\\n[EKSEKUSI AKSI]: Jika permintaan sudah jelas/spesifik (membuka aplikasi apapun yang terpasang di Windows atau membuka folder), atau pengguna telah memilih dari pertanyaan klarifikasi, berikan konfirmasi singkat dan WAJIB sertakan SATU tag aksi di baris paling akhir:\\n[ACTION:OPEN:app_name] (contoh: [ACTION:OPEN:steam], [ACTION:OPEN:anydesk], [ACTION:OPEN:notepad], [ACTION:OPEN:whatsapp], [ACTION:OPEN:discord], [ACTION:OPEN:winword], [ACTION:OPEN:excel], [ACTION:OPEN:chrome])\\n[ACTION:EXPLORER:folder] (contoh: [ACTION:EXPLORER:downloads], [ACTION:EXPLORER:documents], [ACTION:EXPLORER:desktop], [ACTION:EXPLORER:pictures])\\n[ACTION:BROWSER:browser:profile] (contoh: [ACTION:BROWSER:brave:Default])\\n[ACTION:FILE_NEW:folder:file] (contoh: [ACTION:FILE_NEW:desktop:catatan.txt])\\n[ACTION:LOCK]\\n[ACTION:TODO:teks]\\n[ACTION:TIMER:menit]\\n[ACTION:MUTE]\\n[ACTION:SNIP]\\n[ACTION:WEATHER]";

    let mut messages_json = Vec::new();
    messages_json.push(format!(
        r#"{{"role":"system","content":"{}"}}"#,
        system_content
    ));

    // Include up to 6 recent messages from history (multi-turn context)
    let recent_history = if history.len() > 6 {
        &history[history.len() - 6..]
    } else {
        history
    };

    for msg in recent_history {
        let role = if msg.is_user { "user" } else { "assistant" };
        let escaped = escape_json_str(&msg.text);
        if !escaped.is_empty() {
            messages_json.push(format!(
                r#"{{"role":"{}","content":"{}"}}"#,
                role, escaped
            ));
        }
    }

    let escaped_prompt = escape_json_str(prompt);
    messages_json.push(format!(
        r#"{{"role":"user","content":"{}"}}"#,
        escaped_prompt
    ));

    let messages_array = messages_json.join(",");
    let json_body = format!(
        r#"{{"model":"qwen/qwen3.8-27b","messages":[{}],"max_tokens":350}}"#,
        messages_array
    );
    let body_bytes = json_body.as_bytes();

    let user_agent = windows::core::w!("Mozilla/5.0 (Windows NT 10.0; Win64; x64) NotDynamicIsland/2.0");
    let headers_str = format!(
        "Authorization: Bearer {}\r\nContent-Type: application/json\r\n",
        api_key
    );
    let headers_w: Vec<u16> = headers_str.encode_utf16().collect();

    unsafe {
        let session = WinHttpOpen(
            user_agent,
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        );
        if session.is_null() {
            return Err("WinHttpOpen failed".to_string());
        }

        let connect = WinHttpConnect(session, PCWSTR(host_w.as_ptr()), 443, 0);
        if connect.is_null() {
            let _ = WinHttpCloseHandle(session);
            return Err("WinHttpConnect failed".to_string());
        }

        let verb = windows::core::w!("POST");
        let request = WinHttpOpenRequest(
            connect,
            verb,
            PCWSTR(path_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        );
        if request.is_null() {
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return Err("WinHttpOpenRequest failed".to_string());
        }

        // Set timeouts: 10s connect, 15s send, 15s receive
        let _ = WinHttpSetTimeouts(request, 10000, 15000, 15000, 20000);

        let add_res = WinHttpAddRequestHeaders(
            request,
            &headers_w,
            WINHTTP_ADDREQ_FLAG_ADD | WINHTTP_ADDREQ_FLAG_REPLACE,
        );
        if add_res.is_err() {
            let err = windows::Win32::Foundation::GetLastError();
            let _ = WinHttpCloseHandle(request);
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return Err(format!("WinHttpAddRequestHeaders failed: {:?}", err));
        }

        let send_res = WinHttpSendRequest(
            request,
            None,
            Some(body_bytes.as_ptr() as *const _),
            body_bytes.len() as u32,
            body_bytes.len() as u32,
            0,
        );
        if send_res.is_err() {
            let err = windows::Win32::Foundation::GetLastError();
            let _ = WinHttpCloseHandle(request);
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return Err(format!("WinHttpSendRequest failed with error: {:?}", err));
        }

        let recv_res = WinHttpReceiveResponse(request, std::ptr::null_mut());
        if recv_res.is_err() {
            let _ = WinHttpCloseHandle(request);
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return Err("WinHttpReceiveResponse failed".to_string());
        }

        let mut response_bytes: Vec<u8> = Vec::new();
        loop {
            let mut size_to_read = 0u32;
            if WinHttpQueryDataAvailable(request, &mut size_to_read).is_err() || size_to_read == 0 {
                break;
            }
            let mut temp_buf = vec![0u8; size_to_read as usize];
            let mut downloaded = 0u32;
            if WinHttpReadData(
                request,
                temp_buf.as_mut_ptr() as *mut _,
                size_to_read,
                &mut downloaded,
            )
            .is_ok()
                && downloaded > 0
            {
                response_bytes.extend_from_slice(&temp_buf[..downloaded as usize]);
            } else {
                break;
            }
        }

        let _ = WinHttpCloseHandle(request);
        let _ = WinHttpCloseHandle(connect);
        let _ = WinHttpCloseHandle(session);

        let resp_str = String::from_utf8_lossy(&response_bytes).to_string();

        if let Some(content) = extract_chat_completion_content(&resp_str) {
            Ok(content)
        } else if resp_str.contains("rate_limit_exceeded") {
            Err("Batas kecepatan Groq Free Tier tercapai. Coba beberapa detik lagi.".to_string())
        } else {
            Err(format!("Gagal membaca respons Groq: {}", resp_str.chars().take(120).collect::<String>()))
        }
    }
}

/// Zero-dependency JSON extraction for OpenAI/Groq chat completion "content": "..."
pub fn extract_chat_completion_content(json: &str) -> Option<String> {
    let key = "\"content\":";
    let idx = json.find(key)?;
    let after = json[idx + key.len()..].trim_start();
    if !after.starts_with('"') {
        return None;
    }
    let after_quote = &after[1..];
    let mut result = String::new();
    let mut chars = after_quote.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(escaped) = chars.next() {
                match escaped {
                    '"' => result.push('"'),
                    '\\' => result.push('\\'),
                    '/' => result.push('/'),
                    'n' => result.push('\n'),
                    'r' => result.push('\r'),
                    't' => result.push('\t'),
                    'u' => {
                        let hex: String = chars.by_ref().take(4).collect();
                        if let Ok(code) = u32::from_str_radix(&hex, 16) {
                            if let Some(ch) = char::from_u32(code) {
                                result.push(ch);
                            }
                        }
                    }
                    other => {
                        result.push('\\');
                        result.push(other);
                    }
                }
            }
        } else if c == '"' {
            break;
        } else {
            result.push(c);
        }
    }
    Some(result.trim().to_string())
}

/// Cleans markdown symbols (such as `**`, `###`, etc.) so plain text renders cleanly
pub fn clean_markdown_for_display(text: &str) -> String {
    let mut cleaned = text.replace("**", "").replace("__", "");
    // Strip leading markdown headers like `### `
    let lines: Vec<&str> = cleaned.lines().map(|line| {
        let t = line.trim_start();
        if t.starts_with("### ") {
            &t[4..]
        } else if t.starts_with("## ") {
            &t[3..]
        } else if t.starts_with("# ") {
            &t[2..]
        } else {
            line
        }
    }).collect();
    cleaned = lines.join("\n");
    cleaned.trim().to_string()
}

/// Executes gemini headless query via CLI subprocess (fallback)
fn execute_gemini_query(history: &[ChatMessage], prompt: &str) -> (String, Option<String>) {
    let mut history_context = String::new();
    let recent_history = if history.len() > 6 {
        &history[history.len() - 6..]
    } else {
        history
    };
    for msg in recent_history {
        let sender = if msg.is_user { "Pengguna" } else { "Asisten" };
        history_context.push_str(&format!("{}: {}\n", sender, msg.text));
    }

    let concise_prompt = if history_context.is_empty() {
        format!(
            "Jawab pertanyaan berikut secara langsung, ringkas, dan to-the-point dalam 1-3 kalimat dalam Bahasa Indonesia. Pahami variasi perintah (minta buka, tolong bukain, coba buka, buka, jalankan; steam, anydesk, wa, dc, wrod). Jika ambigu (contoh 'note', 'browser', 'terminal'), jangan buka dulu, tanyakan klarifikasi 2-3 pilihan. Jika jelas/memilih, sertakan tag di baris akhir: [ACTION:OPEN:app], [ACTION:EXPLORER:folder], [ACTION:BROWSER:browser:profile], [ACTION:FILE_NEW:folder:file], [ACTION:LOCK], [ACTION:TODO:teks], [ACTION:TIMER:menit]: {}",
            prompt
        )
    } else {
        format!(
            "Riwayat percakapan:\n{}\nPengguna: {}\nJika ambigu, tanyakan klarifikasi. Jika jelas/memilih, sertakan tag aksi di baris akhir [ACTION:OPEN:app], [ACTION:EXPLORER:folder], dll. Jawab ringkas 1-3 kalimat Bahasa Indonesia:",
            history_context, prompt
        )
    };

    let (tx, rx) = std::sync::mpsc::channel();
    let prompt_owned = concise_prompt;

    thread::spawn(move || {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "gemini.cmd", "--allowed-mcp-server-names", "none", "--skip-trust", "-p", &prompt_owned, "-o", "json"]);
        cmd.current_dir(std::env::temp_dir());
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let res = cmd.output();
        let _ = tx.send(res);
    });

    // 18s timeout to prevent hanging when 429 rate limit triggers exponential backoff
    match rx.recv_timeout(std::time::Duration::from_secs(18)) {
        Ok(Ok(output)) => {
            let stdout_str = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr_str = String::from_utf8_lossy(&output.stderr).to_string();
            let combined = format!("{}\n{}", stdout_str, stderr_str);

            if let Some(resp) = extract_assistant_response(&stdout_str) {
                (resp, None)
            } else if combined.contains("exhausted your daily quota")
                || combined.contains("Quota exceeded")
                || combined.contains("TerminalQuotaError")
                || combined.contains("too_many_requests")
                || combined.contains("429")
            {
                (
                    String::new(),
                    Some("⚠️ Kuota harian model (20 req/hari Free Tier) habis. Tunggu hingga reset atau gunakan perintah cepat.".to_string()),
                )
            } else if combined.contains("503") || combined.contains("UNAVAILABLE") {
                (
                    String::new(),
                    Some("⚠️ Server Gemini sedang sibuk (503). Silakan coba sesaat lagi.".to_string()),
                )
            } else {
                let cleaned: String = stdout_str
                    .lines()
                    .filter(|l| {
                        !l.starts_with("Warning:")
                            && !l.starts_with("Ripgrep")
                            && !l.starts_with("MCP issues")
                            && !l.starts_with("{")
                            && !l.starts_with("}")
                            && !l.starts_with("\"")
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim()
                    .to_string();

                if !cleaned.is_empty() {
                    (cleaned, None)
                } else if !stderr_str.trim().is_empty() {
                    (String::new(), Some(stderr_str.trim().to_string()))
                } else {
                    (String::new(), Some("Tidak ada respons dari Gemini CLI.".to_string()))
                }
            }
        }
        Ok(Err(e)) => {
            (
                String::new(),
                Some(format!(
                    "Gagal menjalankan Gemini CLI (gemini.cmd): {}. Pastikan Antigravity CLI terpasang.",
                    e
                )),
            )
        }
        Err(_) => {
            (
                String::new(),
                Some("⚠️ Waktu tunggu habis (18 detik). Kuota harian API kemungkinan sedang terkena rate limit throttling.".to_string()),
            )
        }
    }
}

/// Zero-dependency JSON string extraction for the "response" key (Gemini CLI)
pub fn extract_assistant_response(json: &str) -> Option<String> {
    let key = "\"response\":";
    let idx = json.find(key)?;
    let after = json[idx + key.len()..].trim_start();
    if !after.starts_with('"') {
        return None;
    }
    let after_quote = &after[1..];
    let mut result = String::new();
    let mut chars = after_quote.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(escaped) = chars.next() {
                match escaped {
                    '"' => result.push('"'),
                    '\\' => result.push('\\'),
                    '/' => result.push('/'),
                    'n' => result.push('\n'),
                    'r' => result.push('\r'),
                    't' => result.push('\t'),
                    'u' => {
                        let hex: String = chars.by_ref().take(4).collect();
                        if let Ok(code) = u32::from_str_radix(&hex, 16) {
                            if let Some(ch) = char::from_u32(code) {
                                result.push(ch);
                            }
                        }
                    }
                    other => {
                        result.push('\\');
                        result.push(other);
                    }
                }
            }
        } else if c == '"' {
            break;
        } else {
            result.push(c);
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_chat_completion_content() {
        let sample = r#"{"id":"chatcmpl-123","choices":[{"index":0,"message":{"role":"assistant","content":"Halo! Ini jawaban dari Groq Qwen."},"finish_reason":"stop"}]}"#;
        let res = extract_chat_completion_content(sample);
        assert_eq!(res, Some("Halo! Ini jawaban dari Groq Qwen.".to_string()));
    }

    #[test]
    fn test_clean_markdown_for_display() {
        let raw = "Buka **Settings** > **System** > **Display**.\n### Langkah 1\nPilih opsi.";
        let cleaned = clean_markdown_for_display(raw);
        assert_eq!(cleaned, "Buka Settings > System > Display.\nLangkah 1\nPilih opsi.");
    }

    #[test]
    fn test_assistant_state_scrolling() {
        let mut state = AssistantState::default();
        state.max_scroll = 5;
        assert_eq!(state.scroll_offset, 0);
        state.scroll_down();
        assert_eq!(state.scroll_offset, 1);
        state.scroll_up();
        assert_eq!(state.scroll_offset, 0);
        state.scroll_up();
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn test_extract_assistant_response_json() {
        let sample = r#"{
            "session_id": "test-uuid",
            "response": "Halo! Ada yang bisa saya bantu dengan proyek NotDynamicIsland ini?",
            "stats": {}
        }"#;

        let res = extract_assistant_response(sample);
        assert_eq!(
            res,
            Some("Halo! Ada yang bisa saya bantu dengan proyek NotDynamicIsland ini?".to_string())
        );
    }

    #[test]
    fn test_live_groq_winhttp_query() {
        if let Some(key) = load_groq_api_key() {
            let res = execute_groq_query(&key, &[], "halo");
            println!("Groq WinHttp live query result: {:?}", res);
            assert!(res.is_ok(), "Groq query failed: {:?}", res);
        }
    }

    #[test]
    fn test_groq_disambiguation_multi_turn() {
        if let Some(key) = load_groq_api_key() {
            // Turn 1: Ambiguous request
            let turn1 = execute_groq_query(&key, &[], "tolong buka note");
            assert!(turn1.is_ok());
            let turn1_resp = turn1.unwrap();
            println!("Turn 1 disambiguation response: {}", turn1_resp);
            // Must ask clarification and NOT eagerly execute a blind action tag
            let lower1 = turn1_resp.to_lowercase();
            assert!(
                lower1.contains("notepad") || lower1.contains("onenote") || lower1.contains("sticky") || lower1.contains("catatan"),
                "Expected clarification question mentioning options, got: {}",
                turn1_resp
            );

            // Turn 2: User responds with selection using multi-turn context
            let history = [
                ChatMessage {
                    is_user: true,
                    text: "tolong buka note".to_string(),
                    timestamp: Instant::now(),
                },
                ChatMessage {
                    is_user: false,
                    text: turn1_resp,
                    timestamp: Instant::now(),
                },
            ];
            let turn2 = execute_groq_query(&key, &history, "yang pertama");
            assert!(turn2.is_ok());
            let turn2_resp = turn2.unwrap();
            println!("Turn 2 selection response: {}", turn2_resp);
            assert!(
                turn2_resp.contains("[ACTION:OPEN:") || turn2_resp.to_lowercase().contains("notepad"),
                "Expected action tag or confirmation for notepad, got: {}",
                turn2_resp
            );
        }
    }
}
