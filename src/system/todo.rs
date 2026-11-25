//! To-do List Management with persistence, scrolling, and seamless inline input
//! Matches to-do.png in follow_this_design

use std::fs;
use std::path::PathBuf;

pub const WM_APP_TODO_UPDATED: u32 = 0x8005;

#[derive(Clone, Debug, PartialEq)]
pub struct TodoItem {
    pub id: u32,
    pub text: String,
    pub done: bool,
}

pub struct TodoList {
    pub items: Vec<TodoItem>,
    pub next_id: u32,
    pub scroll_offset: usize,
    pub is_input_active: bool,
    pub input: crate::ui::TextInput,
    file_path: PathBuf,
}

impl TodoList {
    pub fn new() -> Self {
        // Determine file path:
        // 1. Portable mode: if tasks.json exists right next to exe, use it!
        // 2. Standard Windows mode: store in %APPDATA%\NotDynamicIsland\tasks.json (keeps Desktop & Downloads clean!)
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        let exe_file = exe_dir.join("tasks.json");

        let file_path = if exe_file.exists() {
            exe_file
        } else {
            let app_data = std::env::var("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|_| exe_dir.clone());
            let island_dir = app_data.join("NotDynamicIsland");
            let _ = fs::create_dir_all(&island_dir);
            island_dir.join("tasks.json")
        };

        let mut list = Self {
            items: Vec::new(),
            next_id: 1,
            scroll_offset: 0,
            is_input_active: false,
            input: crate::ui::TextInput::new(50),
            file_path,
        };
        list.load();
        list
    }

    pub fn start_input(&mut self) {
        self.is_input_active = true;
        self.input.clear();
    }

    pub fn cancel_input(&mut self) {
        self.is_input_active = false;
        self.input.clear();
    }

    pub fn commit_input(&mut self) {
        let clean = self.input.text.trim().to_string();
        if !clean.is_empty() {
            self.add(clean);
        }
        self.is_input_active = false;
        self.input.clear();
    }

    #[allow(dead_code)]
    pub fn input_char(&mut self, c: char) {
        self.input.insert_char(c);
    }

    #[allow(dead_code)]
    pub fn input_backspace(&mut self) {
        self.input.backspace();
    }

    pub fn load(&mut self) {
        if self.file_path.exists() {
            if let Ok(content) = fs::read_to_string(&self.file_path) {
                let mut loaded = Vec::new();
                let mut rest = content.as_str();

                // Scan each JSON object delimited by balanced { and } outside quoted strings
                while let Some(start_obj) = rest.find('{') {
                    let after_start = &rest[start_obj..];
                    let bytes = after_start.as_bytes();
                    let mut depth = 0usize;
                    let mut in_str = false;
                    let mut escape = false;
                    let mut end_obj_opt = None;
                    for (idx, &b) in bytes.iter().enumerate() {
                        if in_str {
                            if escape {
                                escape = false;
                            } else if b == b'\\' {
                                escape = true;
                            } else if b == b'"' {
                                in_str = false;
                            }
                            continue;
                        }
                        match b {
                            b'"' => in_str = true,
                            b'{' => depth += 1,
                            b'}' => {
                                depth = depth.saturating_sub(1);
                                if depth == 0 {
                                    end_obj_opt = Some(idx);
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }

                    if let Some(end_obj) = end_obj_opt {
                        let obj_str = &after_start[..=end_obj];
                        let is_done = obj_str.contains("\"done\":true");
                        let id = extract_json_num(obj_str, "\"id\":").unwrap_or(0);
                        let text = extract_json_str(obj_str, "\"text\":\"").unwrap_or_default();
                        let unescaped = unescape_json_str(&text);
                        if !unescaped.is_empty() {
                            loaded.push(TodoItem {
                                id,
                                text: unescaped,
                                done: is_done,
                            });
                        }
                        rest = &after_start[end_obj + 1..];
                    } else {
                        break;
                    }
                }

                // If tasks.json exists, ALWAYS honor user's state (even if 0 tasks left)
                self.items = loaded;
                self.next_id = self.items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
                return;
            }
        }

        // Initial default task only for brand-new fresh install
        self.items = vec![
            TodoItem {
                id: 1,
                text: "testing, clear this task".to_string(),
                done: false,
            },
        ];
        self.next_id = 2;
        self.save();
    }

    pub fn save(&self) {
        if let Some(parent) = self.file_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let mut out = String::from("[\n");
        for (i, item) in self.items.iter().enumerate() {
            let escaped_text = item
                .text
                .replace('\\', "\\\\")
                .replace('\"', "\\\"")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
                .replace('\t', "\\t");
            let line = format!(
                "  {{\"id\":{},\"done\":{},\"text\":\"{}\"}}{}\n",
                item.id,
                item.done,
                escaped_text,
                if i + 1 < self.items.len() { "," } else { "" }
            );
            out.push_str(&line);
        }
        out.push_str("]\n");
        let _ = fs::write(&self.file_path, out);
    }

    pub fn toggle(&mut self, id: u32) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.done = !item.done;
            self.save();
        }
    }

    pub fn add(&mut self, text: String) {
        let clean = text.trim();
        if clean.is_empty() {
            return;
        }
        if self.items.len() >= 100 {
            self.items.remove(0);
        }
        self.items.push(TodoItem {
            id: self.next_id,
            text: clean.to_string(),
            done: false,
        });
        self.next_id += 1;
        self.save();
    }

    pub fn clear_done(&mut self) {
        self.items.retain(|i| !i.done);
        self.scroll_offset = 0;
        self.save();
    }

    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        if self.scroll_offset + 1 < self.items.len() {
            self.scroll_offset += 1;
        }
    }

    pub fn counts(&self) -> (usize, usize) {
        let done = self.items.iter().filter(|i| i.done).count();
        (done, self.items.len())
    }
}

fn extract_json_num(line: &str, key: &str) -> Option<u32> {
    let start = line.find(key)? + key.len();
    let sub = &line[start..];
    let end = sub.find([',', '}'])?;
    sub[..end].trim().parse().ok()
}

fn extract_json_str(line: &str, key: &str) -> Option<String> {
    let start = line.find(key)? + key.len();
    let sub = &line[start..];
    let mut end = 0;
    let bytes = sub.as_bytes();
    let mut escaped = false;
    while end < bytes.len() {
        if escaped {
            escaped = false;
        } else if bytes[end] == b'\\' {
            escaped = true;
        } else if bytes[end] == b'"' {
            break;
        }
        end += 1;
    }
    if end <= sub.len() {
        Some(sub[..end].to_string())
    } else {
        None
    }
}

fn unescape_json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_todo_save_and_load_roundtrip() {
        let test_path = std::env::temp_dir().join("test_notdynamicisland_tasks.json");
        let _ = fs::remove_file(&test_path);

        let mut list = TodoList {
            items: Vec::new(),
            next_id: 1,
            scroll_offset: 0,
            is_input_active: false,
            input: crate::ui::TextInput::new(50),
            file_path: test_path.clone(),
        };

        // 1. Initial load creates starter task
        list.load();
        assert_eq!(list.items.len(), 1);
        assert_eq!(list.items[0].text, "testing, clear this task");
        assert!(!list.items[0].done);

        // 2. Add custom tasks with special characters (quotes, braces, backslashes)
        list.add("Beli kopi & susu \"Enak\"".to_string());
        list.add("Cek {config} dan \\path".to_string());
        assert_eq!(list.items.len(), 3);

        // 3. Toggle task done
        list.toggle(2);
        assert!(list.items.iter().find(|i| i.id == 2).unwrap().done);

        // 4. Reload from disk into a fresh instance
        let mut reloaded = TodoList {
            items: Vec::new(),
            next_id: 1,
            scroll_offset: 0,
            is_input_active: false,
            input: crate::ui::TextInput::new(50),
            file_path: test_path.clone(),
        };
        reloaded.load();

        assert_eq!(reloaded.items.len(), 3);
        assert_eq!(reloaded.items[1].text, "Beli kopi & susu \"Enak\"");
        assert!(reloaded.items[1].done);
        assert_eq!(reloaded.items[2].text, "Cek {config} dan \\path");
        assert!(!reloaded.items[2].done);

        // 5. Test clear done
        reloaded.clear_done();
        let remaining = reloaded.items.len();

        let mut reloaded_after_clear = TodoList {
            items: Vec::new(),
            next_id: 1,
            scroll_offset: 0,
            is_input_active: false,
            input: crate::ui::TextInput::new(50),
            file_path: test_path.clone(),
        };
        reloaded_after_clear.load();
        assert_eq!(reloaded_after_clear.items.len(), remaining);

        let _ = fs::remove_file(&test_path);
    }

    #[test]
    fn test_todo_scrolling() {
        let mut list = TodoList {
            items: vec![
                TodoItem { id: 1, text: "Task 1".to_string(), done: false },
                TodoItem { id: 2, text: "Task 2".to_string(), done: false },
                TodoItem { id: 3, text: "Task 3".to_string(), done: false },
            ],
            next_id: 4,
            scroll_offset: 0,
            is_input_active: false,
            input: crate::ui::TextInput::new(50),
            file_path: std::env::temp_dir().join("test_scroll.json"),
        };

        // Scroll down step by step
        list.scroll_down();
        assert_eq!(list.scroll_offset, 1);
        list.scroll_down();
        assert_eq!(list.scroll_offset, 2);
        // At end: cannot scroll past last item
        list.scroll_down();
        assert_eq!(list.scroll_offset, 2);

        // Scroll back up
        list.scroll_up();
        assert_eq!(list.scroll_offset, 1);
        list.scroll_up();
        assert_eq!(list.scroll_offset, 0);
        list.scroll_up();
        assert_eq!(list.scroll_offset, 0);
    }
}
