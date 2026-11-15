//! Universal Native Text Input Component for NotDynamicIsland
//! Provides standard word-processor / notepad editing capabilities:
//! - Character-by-character cursor navigation (Left / Right)
//! - Word-by-word cursor navigation (Ctrl + Left / Right)
//! - Home / End navigation
//! - Selection with Shift + Arrows / Home / End
//! - Select All (Ctrl + A) with native visual highlight
//! - Copy / Cut / Paste (Ctrl + C, Ctrl + X, Ctrl + V)
//! - Typing or Backspace replaces active selection
//! - Delete key (forward delete) support
//! - Sub-pixel mouse hit-testing to click-position caret
//! - Horizontal smooth viewport scrolling
//! - Zero external dependencies, pure Rust, 0.0% idle CPU overhead

use crate::render::primitives::draw_rounded_rect;
use crate::render::{Color, PixelBuffer, TextRenderer};

#[derive(Clone, Debug, PartialEq)]
pub struct TextInput {
    pub text: String,
    pub cursor: usize,                   // Character index (0..=char_count)
    pub selection_anchor: Option<usize>, // If Some(a), selected range is min(a, cursor)..max(a, cursor)
    pub max_len: usize,                  // Maximum characters allowed
    pub scroll_offset: f32,              // Horizontal viewport scroll in pixels
}

#[allow(dead_code)]
impl TextInput {
    pub fn new(max_len: usize) -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            selection_anchor: None,
            max_len,
            scroll_offset: 0.0,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn char_count(&self) -> usize {
        self.text.chars().count()
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.selection_anchor = None;
        self.scroll_offset = 0.0;
    }

    pub fn set_text(&mut self, new_text: &str) {
        let clean: String = new_text.chars().take(self.max_len).collect();
        self.cursor = clean.chars().count();
        self.selection_anchor = None;
        self.text = clean;
        self.scroll_offset = 0.0;
    }

    /// Returns the active selection as `Some((start, end))` where `start < end` in character indices.
    pub fn selection(&self) -> Option<(usize, usize)> {
        if let Some(anchor) = self.selection_anchor {
            let total = self.char_count();
            let a = anchor.min(total);
            let c = self.cursor.min(total);
            if a != c {
                return Some((a.min(c), a.max(c)));
            }
        }
        None
    }

    pub fn has_selection(&self) -> bool {
        self.selection().is_some()
    }

    pub fn clear_selection(&mut self) {
        self.selection_anchor = None;
    }

    /// Select All (Ctrl + A): selects entire string and positions cursor at the end.
    pub fn select_all(&mut self) {
        let total = self.char_count();
        if total > 0 {
            self.selection_anchor = Some(0);
            self.cursor = total;
        } else {
            self.selection_anchor = None;
            self.cursor = 0;
        }
    }

    /// Returns the currently selected substring, if any.
    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        let chars: Vec<char> = self.text.chars().collect();
        if start < chars.len() {
            let actual_end = end.min(chars.len());
            let slice: String = chars[start..actual_end].iter().collect();
            Some(slice)
        } else {
            None
        }
    }

    /// Deletes the currently selected character range. Returns true if a range was deleted.
    pub fn delete_selection(&mut self) -> bool {
        if let Some((start, end)) = self.selection() {
            let mut chars: Vec<char> = self.text.chars().collect();
            let total = chars.len();
            let s = start.min(total);
            let e = end.min(total);
            if s < e {
                chars.drain(s..e);
                self.text = chars.into_iter().collect();
                self.cursor = s;
                self.selection_anchor = None;
                return true;
            }
        }
        self.selection_anchor = None;
        false
    }

    /// Inserts a single character at cursor. If text is selected, replaces selection.
    pub fn insert_char(&mut self, c: char) {
        self.delete_selection();
        let mut chars: Vec<char> = self.text.chars().collect();
        if chars.len() < self.max_len {
            let pos = self.cursor.min(chars.len());
            chars.insert(pos, c);
            self.cursor = pos + 1;
            self.text = chars.into_iter().collect();
        }
    }

    /// Inserts a string at cursor (e.g. from clipboard paste). If text is selected, replaces selection.
    pub fn insert_text(&mut self, s: &str) {
        self.delete_selection();
        let clean: Vec<char> = s
            .chars()
            .filter(|c| !c.is_control() || *c == ' ' || *c == '\t')
            .collect();
        let mut chars: Vec<char> = self.text.chars().collect();
        let available = self.max_len.saturating_sub(chars.len());
        let count = clean.len().min(available);
        let pos = self.cursor.min(chars.len());
        for (i, ch) in clean.into_iter().take(count).enumerate() {
            chars.insert(pos + i, ch);
        }
        self.cursor = pos + count;
        self.text = chars.into_iter().collect();
    }

    /// Backspace key: deletes selection, or deletes character before cursor.
    pub fn backspace(&mut self) {
        if !self.delete_selection() {
            let mut chars: Vec<char> = self.text.chars().collect();
            if self.cursor > 0 && !chars.is_empty() {
                let pos = self.cursor.min(chars.len());
                if pos > 0 {
                    chars.remove(pos - 1);
                    self.cursor = pos - 1;
                    self.text = chars.into_iter().collect();
                }
            }
        }
    }

    /// Delete key: deletes selection, or deletes character after cursor.
    pub fn delete(&mut self) {
        if !self.delete_selection() {
            let mut chars: Vec<char> = self.text.chars().collect();
            let pos = self.cursor.min(chars.len());
            if pos < chars.len() {
                chars.remove(pos);
                self.text = chars.into_iter().collect();
            }
        }
    }

    /// Left Arrow key navigation: moves cursor left by 1 character.
    pub fn move_left(&mut self, shift: bool) {
        let total = self.char_count();
        self.cursor = self.cursor.min(total);
        if shift {
            if self.selection_anchor.is_none() {
                self.selection_anchor = Some(self.cursor);
            }
            self.cursor = self.cursor.saturating_sub(1);
        } else if let Some((start, _)) = self.selection() {
            self.cursor = start;
            self.selection_anchor = None;
        } else {
            self.cursor = self.cursor.saturating_sub(1);
        }
    }

    /// Right Arrow key navigation: moves cursor right by 1 character.
    pub fn move_right(&mut self, shift: bool) {
        let total = self.char_count();
        self.cursor = self.cursor.min(total);
        if shift {
            if self.selection_anchor.is_none() {
                self.selection_anchor = Some(self.cursor);
            }
            self.cursor = (self.cursor + 1).min(total);
        } else if let Some((_, end)) = self.selection() {
            self.cursor = end;
            self.selection_anchor = None;
        } else {
            self.cursor = (self.cursor + 1).min(total);
        }
    }

    /// Home key navigation: jumps cursor to beginning (index 0).
    pub fn move_home(&mut self, shift: bool) {
        if shift {
            if self.selection_anchor.is_none() {
                self.selection_anchor = Some(self.cursor);
            }
            self.cursor = 0;
        } else {
            self.cursor = 0;
            self.selection_anchor = None;
        }
    }

    /// End key navigation: jumps cursor to end of text.
    pub fn move_end(&mut self, shift: bool) {
        let total = self.char_count();
        if shift {
            if self.selection_anchor.is_none() {
                self.selection_anchor = Some(self.cursor);
            }
            self.cursor = total;
        } else {
            self.cursor = total;
            self.selection_anchor = None;
        }
    }

    /// Ctrl + Left Arrow: jump to start of previous word.
    pub fn move_word_left(&mut self, shift: bool) {
        let chars: Vec<char> = self.text.chars().collect();
        let mut pos = self.cursor.min(chars.len());
        // Skip leading whitespace backwards
        while pos > 0 && chars[pos - 1].is_whitespace() {
            pos -= 1;
        }
        // Skip non-whitespace (word characters)
        while pos > 0 && !chars[pos - 1].is_whitespace() {
            pos -= 1;
        }
        if shift {
            if self.selection_anchor.is_none() {
                self.selection_anchor = Some(self.cursor);
            }
            self.cursor = pos;
        } else {
            self.cursor = pos;
            self.selection_anchor = None;
        }
    }

    /// Ctrl + Right Arrow: jump to start of next word.
    pub fn move_word_right(&mut self, shift: bool) {
        let chars: Vec<char> = self.text.chars().collect();
        let total = chars.len();
        let mut pos = self.cursor.min(total);
        // Skip non-whitespace
        while pos < total && !chars[pos].is_whitespace() {
            pos += 1;
        }
        // Skip trailing whitespace
        while pos < total && chars[pos].is_whitespace() {
            pos += 1;
        }
        if shift {
            if self.selection_anchor.is_none() {
                self.selection_anchor = Some(self.cursor);
            }
            self.cursor = pos;
        } else {
            self.cursor = pos;
            self.selection_anchor = None;
        }
    }

    /// Sub-pixel mouse hit-testing: sets cursor to the closest character boundary.
    pub fn set_cursor_from_x(
        &mut self,
        click_x: f32,
        text_start_x: f32,
        tr: &TextRenderer,
        size: f32,
        is_bold: bool,
        shift: bool,
    ) {
        let total_chars = self.char_count();
        let relative_x = click_x - (text_start_x - self.scroll_offset);
        if relative_x <= 0.0 {
            if shift {
                if self.selection_anchor.is_none() {
                    self.selection_anchor = Some(self.cursor);
                }
                self.cursor = 0;
            } else {
                self.cursor = 0;
                self.selection_anchor = None;
            }
            return;
        }

        let chars: Vec<char> = self.text.chars().collect();
        let mut prev_w = 0.0;
        let mut found_idx = total_chars;

        for (i, _) in chars.iter().enumerate() {
            let prefix: String = chars[..=i].iter().collect();
            let (w, _) = tr.measure_text(&prefix, size, is_bold);
            let midpoint = (prev_w + w) * 0.5;
            if relative_x < midpoint {
                found_idx = i;
                break;
            }
            prev_w = w;
        }

        if shift {
            if self.selection_anchor.is_none() {
                self.selection_anchor = Some(self.cursor);
            }
            self.cursor = found_idx;
        } else {
            self.cursor = found_idx;
            self.selection_anchor = None;
        }
    }

    /// Auto-scroll viewport so caret stays comfortably visible.
    pub fn update_scroll(
        &mut self,
        tr: &TextRenderer,
        size: f32,
        is_bold: bool,
        visible_width: f32,
        margin: f32,
    ) {
        let chars: Vec<char> = self.text.chars().collect();
        let total = chars.len();
        let cur = self.cursor.min(total);

        let prefix: String = chars[..cur].iter().collect();
        let (caret_offset_x, _) = tr.measure_text(&prefix, size, is_bold);
        let (total_w, _) = tr.measure_text(&self.text, size, is_bold);

        if total_w <= visible_width {
            self.scroll_offset = 0.0;
            return;
        }

        let max_visible_x = self.scroll_offset + visible_width - margin;
        let min_visible_x = self.scroll_offset + margin;

        if caret_offset_x > max_visible_x {
            self.scroll_offset = caret_offset_x - (visible_width - margin);
        } else if caret_offset_x < min_visible_x {
            self.scroll_offset = (caret_offset_x - margin).max(0.0);
        }

        let max_scroll = (total_w - visible_width + margin).max(0.0);
        self.scroll_offset = self.scroll_offset.clamp(0.0, max_scroll);
    }

    /// Renders text input: selection rectangle, clipped text, and blinking caret.
    pub fn render(
        &self,
        buf: &mut PixelBuffer,
        tr: &TextRenderer,
        box_x: f32,
        box_y: f32,
        box_w: f32,
        box_h: f32,
        size: f32,
        color: Color,
        placeholder: &str,
        is_focused: bool,
        is_bold: bool,
        opacity: f32,
        anim_time: f32,
    ) {
        let padding_x = 8.0;
        let text_start_x = box_x + padding_x;
        let visible_w = (box_w - padding_x * 2.0).max(10.0);
        let clip_min_x = text_start_x;
        let clip_max_x = box_x + box_w - padding_x;
        let baseline_y = box_y + (box_h + size * 0.7) * 0.5;

        let (total_w, _) = tr.measure_text(&self.text, size, is_bold);
        let scroll_offset = if total_w <= visible_w {
            0.0
        } else {
            self.scroll_offset.clamp(0.0, (total_w - visible_w + 24.0).max(0.0))
        };

        if self.text.is_empty() {
            // Placeholder text
            if !placeholder.is_empty() {
                tr.draw_text_clipped(
                    buf,
                    placeholder,
                    text_start_x,
                    baseline_y,
                    size,
                    Color::rgba(148, 163, 184, 0.65 * opacity),
                    false,
                    clip_min_x,
                    clip_max_x,
                );
            }
            // Blinking caret at position 0
            if is_focused && (anim_time * 2.5).fract() < 0.5 {
                let caret_x = text_start_x.round() as usize;
                let top_y = (box_y + 5.0).round() as usize;
                let bot_y = (box_y + box_h - 5.0).round() as usize;
                for py in top_y..=bot_y {
                    buf.blend_pixel(caret_x, py, 255, 255, 255, 0.95 * opacity);
                    buf.blend_pixel(caret_x + 1, py, 255, 255, 255, 0.95 * opacity);
                }
            }
            return;
        }

        let chars: Vec<char> = self.text.chars().collect();
        let total = chars.len();

        // 1. Draw Selection Highlight if active
        if let Some((sel_start, sel_end)) = self.selection() {
            let start_prefix: String = chars[..sel_start.min(total)].iter().collect();
            let end_prefix: String = chars[..sel_end.min(total)].iter().collect();

            let (w_start, _) = tr.measure_text(&start_prefix, size, is_bold);
            let (w_end, _) = tr.measure_text(&end_prefix, size, is_bold);

            let raw_x1 = text_start_x - scroll_offset + w_start;
            let raw_x2 = text_start_x - scroll_offset + w_end;

            let h_x1 = raw_x1.clamp(clip_min_x, clip_max_x);
            let h_x2 = raw_x2.clamp(clip_min_x, clip_max_x);

            if h_x2 > h_x1 {
                let sel_y = box_y + 4.0;
                let sel_h = (box_h - 8.0).max(12.0);
                draw_rounded_rect(
                    buf,
                    h_x1,
                    sel_y,
                    h_x2 - h_x1,
                    sel_h,
                    3.0,
                    Color::rgba(59, 130, 246, 0.55 * opacity),
                );
            }
        }

        // 2. Draw Clipped Text
        let render_x = text_start_x - scroll_offset;
        tr.draw_text_clipped(
            buf,
            &self.text,
            render_x,
            baseline_y,
            size,
            color,
            is_bold,
            clip_min_x,
            clip_max_x,
        );

        // 3. Draw Blinking Caret
        if is_focused && (anim_time * 2.5).fract() < 0.5 {
            let cur = self.cursor.min(total);
            let cur_prefix: String = chars[..cur].iter().collect();
            let (caret_w, _) = tr.measure_text(&cur_prefix, size, is_bold);
            let caret_f = render_x + caret_w;

            if caret_f >= clip_min_x - 1.0 && caret_f <= clip_max_x + 1.0 {
                let caret_x = caret_f.round() as usize;
                let top_y = (box_y + 5.0).round() as usize;
                let bot_y = (box_y + box_h - 5.0).round() as usize;
                for py in top_y..=bot_y {
                    buf.blend_pixel(caret_x, py, 255, 255, 255, 0.98 * opacity);
                    buf.blend_pixel(caret_x + 1, py, 255, 255, 255, 0.85 * opacity);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_input_insert_and_backspace() {
        let mut input = TextInput::new(100);
        assert_eq!(input.cursor, 0);
        assert!(input.is_empty());

        input.insert_char('H');
        input.insert_char('i');
        assert_eq!(input.text(), "Hi");
        assert_eq!(input.cursor, 2);

        input.backspace();
        assert_eq!(input.text(), "H");
        assert_eq!(input.cursor, 1);
    }

    #[test]
    fn test_text_input_cursor_navigation() {
        let mut input = TextInput::new(100);
        input.set_text("Hello World");
        assert_eq!(input.cursor, 11);

        input.move_left(false);
        assert_eq!(input.cursor, 10);

        input.move_home(false);
        assert_eq!(input.cursor, 0);

        input.move_end(false);
        assert_eq!(input.cursor, 11);

        input.move_word_left(false);
        assert_eq!(input.cursor, 6); // start of "World"

        input.move_word_left(false);
        assert_eq!(input.cursor, 0); // start of "Hello"

        input.move_word_right(false);
        assert_eq!(input.cursor, 6); // start of "World"
    }

    #[test]
    fn test_text_input_select_all_and_replace() {
        let mut input = TextInput::new(100);
        input.set_text("Ganti Semua");
        assert_eq!(input.cursor, 11);

        // Ctrl + A: Select All
        input.select_all();
        assert_eq!(input.selection(), Some((0, 11)));
        assert_eq!(input.selected_text(), Some("Ganti Semua".to_string()));

        // Typing character replaces selected text
        input.insert_char('A');
        assert_eq!(input.text(), "A");
        assert_eq!(input.cursor, 1);
        assert_eq!(input.selection(), None);
    }

    #[test]
    fn test_text_input_selection_with_shift() {
        let mut input = TextInput::new(100);
        input.set_text("Testing Shift Selection");
        input.cursor = 7; // after "Testing"

        input.move_left(true);
        assert_eq!(input.cursor, 6);
        assert_eq!(input.selection(), Some((6, 7)));
        assert_eq!(input.selected_text(), Some("g".to_string()));

        // Backspace deletes selection
        input.backspace();
        assert_eq!(input.text(), "Testin Shift Selection");
        assert_eq!(input.cursor, 6);
        assert_eq!(input.selection(), None);
    }

    #[test]
    fn test_text_input_paste_text() {
        let mut input = TextInput::new(100);
        input.set_text("Halo ");
        input.insert_text("Dunia!");
        assert_eq!(input.text(), "Halo Dunia!");
        assert_eq!(input.cursor, 11);

        // Select "Dunia!" and paste replacement
        input.selection_anchor = Some(5);
        input.cursor = 11;
        assert_eq!(input.selected_text(), Some("Dunia!".to_string()));

        input.insert_text("Kawan");
        assert_eq!(input.text(), "Halo Kawan");
        assert_eq!(input.cursor, 10);
    }

    #[test]
    fn test_text_input_delete_key() {
        let mut input = TextInput::new(100);
        input.set_text("ABCD");
        input.cursor = 1; // before 'B'

        input.delete();
        assert_eq!(input.text(), "ACD");
        assert_eq!(input.cursor, 1);
    }

    #[test]
    fn test_text_input_auto_scroll() {
        let tr = TextRenderer::new();
        let mut input = TextInput::new(200);
        let visible_w = 100.0;
        let margin = 16.0;

        // Type short string within visible_w
        input.set_text("Short");
        input.update_scroll(&tr, 10.5, false, visible_w, margin);
        assert_eq!(input.scroll_offset, 0.0);

        // Type long string exceeding visible_w character by character
        let long_str = "Ini adalah contoh prompt chat AI yang sangat panjang dan melebihi batas lebar input";
        input.clear();
        for ch in long_str.chars() {
            input.insert_char(ch);
            input.update_scroll(&tr, 10.5, false, visible_w, margin);

            // Caret rendered offset must always stay within [0.0, visible_w]
            let chars: Vec<char> = input.text.chars().collect();
            let prefix: String = chars[..input.cursor].iter().collect();
            let (caret_w, _) = tr.measure_text(&prefix, 10.5, false);
            let caret_screen_x = caret_w - input.scroll_offset;
            assert!(
                caret_screen_x >= 0.0 && caret_screen_x <= visible_w + 0.5,
                "Caret screen x ({}) must stay inside visible_w ({}) at cursor {}",
                caret_screen_x, visible_w, input.cursor
            );
        }
        assert!(input.scroll_offset > 0.0);
    }
}
