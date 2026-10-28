//! Ultra-fast Fontdue Text Rasterizer with Windows native font fallback
//! Supports Latin, Cyrillic, Greek (Segoe UI) + Korean (Malgun Gothic) + CJK (Microsoft YaHei / Meiryo)

use fontdue::{Font, FontSettings};
use std::fs;
use super::buffer::PixelBuffer;
use super::primitives::Color;

pub struct TextRenderer {
    pub regular: Option<Font>,
    pub bold: Option<Font>,
    pub korean: Option<Font>,
    pub cjk: Option<Font>,
}

impl TextRenderer {
    pub fn new() -> Self {
        let regular = Self::load_font("C:\\Windows\\Fonts\\segoeui.ttf");
        let bold = Self::load_font("C:\\Windows\\Fonts\\segoeuib.ttf");
        let korean = Self::load_font("C:\\Windows\\Fonts\\malgun.ttf");
        let cjk = Self::load_font("C:\\Windows\\Fonts\\msyh.ttc")
            .or_else(|| Self::load_font("C:\\Windows\\Fonts\\meiryo.ttc"));

        Self {
            regular,
            bold,
            korean,
            cjk,
        }
    }

    fn load_font(path: &str) -> Option<Font> {
        let data = fs::read(path).ok()?;
        Font::from_bytes(data, FontSettings::default()).ok()
    }

    /// Selects the best font that contains the glyph for `ch`
    fn find_font_for_char(&self, ch: char, is_bold: bool) -> Option<&Font> {
        let primary = if is_bold {
            self.bold.as_ref().or(self.regular.as_ref())
        } else {
            self.regular.as_ref()
        };

        if let Some(font) = primary {
            if font.lookup_glyph_index(ch) != 0 {
                return Some(font);
            }
        }

        // 1. Fallback to Korean (Malgun Gothic)
        if let Some(kr) = self.korean.as_ref() {
            if kr.lookup_glyph_index(ch) != 0 {
                return Some(kr);
            }
        }

        // 2. Fallback to CJK (Microsoft YaHei / Meiryo)
        if let Some(cjk) = self.cjk.as_ref() {
            if cjk.lookup_glyph_index(ch) != 0 {
                return Some(cjk);
            }
        }

        // Return primary font even if missing (will render default glyph)
        primary
    }

    pub fn measure_text(&self, text: &str, size: f32, is_bold: bool) -> (f32, f32) {
        let mut total_width = 0.0;
        let mut max_height = size;

        for ch in text.chars() {
            if let Some(font) = self.find_font_for_char(ch, is_bold) {
                let metrics = font.metrics(ch, size);
                total_width += metrics.advance_width;
                max_height = max_height.max(metrics.height as f32);
            } else {
                total_width += size * 0.55;
            }
        }

        (total_width, max_height)
    }

    /// Renders text at (x, y) baseline with anti-aliasing and CJK fallback
    pub fn draw_text(
        &self,
        buf: &mut PixelBuffer,
        text: &str,
        start_x: f32,
        baseline_y: f32,
        size: f32,
        color: Color,
        is_bold: bool,
    ) -> f32 {
        let mut current_x = start_x;

        for ch in text.chars() {
            let Some(font) = self.find_font_for_char(ch, is_bold) else {
                current_x += size * 0.55;
                continue;
            };

            let (metrics, bitmap) = font.rasterize(ch, size);
            let char_x = current_x + metrics.xmin as f32;
            let char_y = baseline_y - metrics.ymin as f32 - metrics.height as f32;

            for r in 0..metrics.height {
                let py = (char_y + r as f32).round() as i32;
                if py < 0 || py >= buf.height as i32 {
                    continue;
                }

                for c in 0..metrics.width {
                    let px = (char_x + c as f32).round() as i32;
                    if px < 0 || px >= buf.width as i32 {
                        continue;
                    }

                    let coverage = bitmap[r * metrics.width + c] as f32 / 255.0;
                    if coverage > 0.0 {
                        buf.blend_pixel(
                            px as usize,
                            py as usize,
                            color.r,
                            color.g,
                            color.b,
                            coverage * color.a,
                        );
                    }
                }
            }

            current_x += metrics.advance_width;
        }

        current_x
    }

    /// Renders truncated text with ellipsis ("...") if exceeding max_width
    pub fn draw_text_truncated(
        &self,
        buf: &mut PixelBuffer,
        text: &str,
        start_x: f32,
        baseline_y: f32,
        size: f32,
        max_width: f32,
        color: Color,
        is_bold: bool,
    ) -> f32 {
        let (full_w, _) = self.measure_text(text, size, is_bold);
        if full_w <= max_width {
            return self.draw_text(buf, text, start_x, baseline_y, size, color, is_bold);
        }

        let (ellipsis_w, _) = self.measure_text("...", size, is_bold);
        let target_w = (max_width - ellipsis_w).max(0.0);

        let mut truncated = String::new();
        let mut cur_w = 0.0;

        for ch in text.chars() {
            let (cw, _) = self.measure_text(&ch.to_string(), size, is_bold);
            if cur_w + cw > target_w {
                break;
            }
            truncated.push(ch);
            cur_w += cw;
        }
        truncated.push_str("...");

        self.draw_text(buf, &truncated, start_x, baseline_y, size, color, is_bold)
    }
}
