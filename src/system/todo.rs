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
    pub input_buffer: String,
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
            input_buffer: String::new(),
            file_path,
        };
        list.load();
        list
    }

    pub fn start_input(&mut self) {
        self.is_input_active = true;
        self.input_buffer.clear();
    }

    pub fn cancel_input(&mut self) {
        self.is_input_active = false;
        self.input_buffer.clear();
    }

    pub fn commit_input(&mut self) {
        let clean = self.input_buffer.trim().to_string();
        if !clean.is_empty() {
            self.add(clean);
        }
        self.is_input_active = false;
        self.input_buffer.clear();
    }

    pub fn input_char(&mut self, c: char) {
        if self.input_buffer.chars().count() < 35 {
            self.input_buffer.push(c);
        }
    }

    pub fn input_backspace(&mut self) {
        self.input_buffer.pop();
    }

    pub fn load(&mut self) {
        if self.file_path.exists() {
            if let Ok(content) = fs::read_to_string(&self.file_path) {
                let mut loaded = Vec::new();
                let mut rest = content.as_str();

                // Scan each JSON object delimited by { and }
                while let Some(start_obj) = rest.find('{') {
                    let after_start = &rest[start_obj..];
                    if let Some(end_obj) = after_start.find('}') {
                        let obj_str = &after_start[..=end_obj];
                        let is_done = obj_str.contains("\"done\":true");
                        let id = extract_json_num(obj_str, "\"id\":").unwrap_or(0);
                        let text = extract_json_str(obj_str, "\"text\":\"").unwrap_or_default();
                        let unescaped = text.replace("\\\"", "\"").replace("\\\\", "\\");
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
            let escaped_text = item.text.replace('\\', "\\\\").replace('\"', "\\\"");
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
        if self.scroll_offset + 3 < self.items.len() {
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
    while end < bytes.len() {
        if bytes[end] == b'"' && (end == 0 || bytes[end - 1] != b'\\') {
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
            input_buffer: String::new(),
            file_path: test_path.clone(),
        };

        // 1. Initial load creates starter task
        list.load();
        assert_eq!(list.items.len(), 1);
        assert_eq!(list.items[0].text, "testing, clear this task");
        assert!(!list.items[0].done);

        // 2. Add custom tasks with special characters
        list.add("Beli kopi & susu \"Enak\"".to_string());
        list.add("Task nomor dua".to_string());
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
            input_buffer: String::new(),
            file_path: test_path.clone(),
        };
        reloaded.load();

        assert_eq!(reloaded.items.len(), 3);
        assert_eq!(reloaded.items[1].text, "Beli kopi & susu \"Enak\"");
        assert!(reloaded.items[1].done);
        assert_eq!(reloaded.items[2].text, "Task nomor dua");
        assert!(!reloaded.items[2].done);

        // 5. Test clear done
        reloaded.clear_done();
        let remaining = reloaded.items.len();

        let mut reloaded_after_clear = TodoList {
            items: Vec::new(),
            next_id: 1,
            scroll_offset: 0,
            is_input_active: false,
            input_buffer: String::new(),
            file_path: test_path.clone(),
        };
        reloaded_after_clear.load();
        assert_eq!(reloaded_after_clear.items.len(), remaining);

        let _ = fs::remove_file(&test_path);
    }
}
