//! Real-time Backdrop Lens Diffusion (Optical Background Blur)
//! Captures desktop behind the layered window and applies an in-house fast optical blur
//! with zero noise, zero frosted gray haze, and strict sub-pixel SDF curve masking.

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
    GetDC, GetDIBits, ReleaseDC, SelectObject, SetStretchBltMode, StretchBlt,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, COLORONCOLOR, DIB_RGB_COLORS, HDC, SRCCOPY,
};
use std::mem::size_of;

pub struct BackdropBlur {
    pub raw_buffer: Vec<u32>,
    pub blurred_buffer: Vec<u32>,
    pub temp_buffer: Vec<u32>,
    pub width: usize,
    pub height: usize,
    pub scale: usize,
    hdc_mem: HDC,
    hbitmap: windows::Win32::Graphics::Gdi::HBITMAP,
}

unsafe impl Send for BackdropBlur {}

impl BackdropBlur {
    pub fn new(full_width: usize, full_height: usize) -> Self {
        let scale = 2;
        let width = full_width / scale;
        // The top notch never exceeds 160px height; cap capture height to 160px for maximum speed
        let height = (full_height.min(160)) / scale;
        let size = width * height;
        unsafe {
            let hdc_screen = GetDC(HWND::default());
            let hdc_mem = CreateCompatibleDC(hdc_screen);
            let hbitmap = CreateCompatibleBitmap(hdc_screen, width as i32, height as i32);
            SelectObject(hdc_mem, hbitmap);
            ReleaseDC(HWND::default(), hdc_screen);

            Self {
                raw_buffer: vec![0; size],
                blurred_buffer: vec![0; size],
                temp_buffer: vec![0; size],
                width,
                height,
                scale,
                hdc_mem,
                hbitmap,
            }
        }
    }

    /// Captures the desktop region directly behind (x, y) with hardware-assisted 2x downsampling
    pub fn capture(&mut self, screen_x: i32, screen_y: i32) {
        unsafe {
            let hdc_screen = GetDC(HWND::default());
            let _ = SetStretchBltMode(self.hdc_mem, COLORONCOLOR);
            let _ = StretchBlt(
                self.hdc_mem,
                0,
                0,
                self.width as i32,
                self.height as i32,
                hdc_screen,
                screen_x,
                screen_y,
                (self.width * self.scale) as i32,
                (self.height * self.scale) as i32,
                SRCCOPY,
            );
            ReleaseDC(HWND::default(), hdc_screen);

            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: self.width as i32,
                    biHeight: -(self.height as i32), // Top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };

            GetDIBits(
                self.hdc_mem,
                self.hbitmap,
                0,
                self.height as u32,
                Some(self.raw_buffer.as_mut_ptr() as *mut _),
                &mut bmi,
                DIB_RGB_COLORS,
            );
        }

        // Apply 2-stage optical blur (radius 4 on 2x downscaled buffer equals radius 8 at full resolution)
        self.apply_optical_blur(4);
    }

    /// Fast optical lens diffusion: 2-stage cascaded box blur approximating Gaussian filter (O(N) per pixel)
    pub fn apply_optical_blur(&mut self, radius: usize) {
        let w = self.width;
        let h = self.height;
        let r = radius.max(1);

        // Stage 1: raw_buffer -> temp_buffer (H) -> blurred_buffer (V)
        Self::box_blur_h(&self.raw_buffer, &mut self.temp_buffer, w, h, r);
        Self::box_blur_v(&self.temp_buffer, &mut self.blurred_buffer, w, h, r);

        // Stage 2: blurred_buffer -> temp_buffer (H) -> blurred_buffer (V)
        // Two cascaded box blurs yield a smooth quadratic B-spline blur (silky glass diffusion)
        Self::box_blur_h(&self.blurred_buffer, &mut self.temp_buffer, w, h, r);
        Self::box_blur_v(&self.temp_buffer, &mut self.blurred_buffer, w, h, r);
    }

    #[inline(always)]
    fn box_blur_h(src: &[u32], dst: &mut [u32], w: usize, h: usize, r: usize) {
        let div = (2 * r + 1) as u32;
        for y in 0..h {
            let row_offset = y * w;
            let first_pixel = src[row_offset];
            let f_r = (first_pixel >> 16) & 0xFF;
            let f_g = (first_pixel >> 8) & 0xFF;
            let f_b = first_pixel & 0xFF;

            let mut sum_r = f_r * (r as u32 + 1);
            let mut sum_g = f_g * (r as u32 + 1);
            let mut sum_b = f_b * (r as u32 + 1);

            for x in 1..=r.min(w - 1) {
                let px = src[row_offset + x];
                sum_r += (px >> 16) & 0xFF;
                sum_g += (px >> 8) & 0xFF;
                sum_b += px & 0xFF;
            }

            for x in 0..w {
                let avg_r = sum_r / div;
                let avg_g = sum_g / div;
                let avg_b = sum_b / div;
                dst[row_offset + x] = (0xFF << 24) | (avg_r << 16) | (avg_g << 8) | avg_b;

                let left_idx = if x >= r { x - r } else { 0 };
                let right_idx = (x + r + 1).min(w - 1);

                let left_px = src[row_offset + left_idx];
                let right_px = src[row_offset + right_idx];

                sum_r = sum_r + ((right_px >> 16) & 0xFF) - ((left_px >> 16) & 0xFF);
                sum_g = sum_g + ((right_px >> 8) & 0xFF) - ((left_px >> 8) & 0xFF);
                sum_b = sum_b + (right_px & 0xFF) - (left_px & 0xFF);
            }
        }
    }

    #[inline(always)]
    fn box_blur_v(src: &[u32], dst: &mut [u32], w: usize, h: usize, r: usize) {
        let div = (2 * r + 1) as u32;
        for x in 0..w {
            let first_pixel = src[x];
            let f_r = (first_pixel >> 16) & 0xFF;
            let f_g = (first_pixel >> 8) & 0xFF;
            let f_b = first_pixel & 0xFF;

            let mut sum_r = f_r * (r as u32 + 1);
            let mut sum_g = f_g * (r as u32 + 1);
            let mut sum_b = f_b * (r as u32 + 1);

            for y in 1..=r.min(h - 1) {
                let px = src[y * w + x];
                sum_r += (px >> 16) & 0xFF;
                sum_g += (px >> 8) & 0xFF;
                sum_b += px & 0xFF;
            }

            for y in 0..h {
                let avg_r = sum_r / div;
                let avg_g = sum_g / div;
                let avg_b = sum_b / div;
                dst[y * w + x] = (0xFF << 24) | (avg_r << 16) | (avg_g << 8) | avg_b;

                let top_idx = if y >= r { y - r } else { 0 };
                let bottom_idx = (y + r + 1).min(h - 1);

                let top_px = src[top_idx * w + x];
                let bottom_px = src[bottom_idx * w + x];

                sum_r = sum_r + ((bottom_px >> 16) & 0xFF) - ((top_px >> 16) & 0xFF);
                sum_g = sum_g + ((bottom_px >> 8) & 0xFF) - ((top_px >> 8) & 0xFF);
                sum_b = sum_b + (bottom_px & 0xFF) - (top_px & 0xFF);
            }
        }
    }

    #[inline(always)]
    pub fn get_blurred_pixel(&self, x: usize, y: usize) -> (u8, u8, u8) {
        let down_x = if self.scale > 1 { x / self.scale } else { x };
        let down_y = if self.scale > 1 { y / self.scale } else { y };
        if down_x < self.width && down_y < self.height {
            let px = self.blurred_buffer[down_y * self.width + down_x];
            (
                ((px >> 16) & 0xFF) as u8,
                ((px >> 8) & 0xFF) as u8,
                (px & 0xFF) as u8,
            )
        } else {
            (0, 0, 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::buffer::PixelBuffer;
    use crate::render::primitives::*;
    use crate::render::text::TextRenderer;
    use std::fs::File;
    use std::io::Write;

    fn save_bmp(path: &str, buf: &PixelBuffer) {
        let w = buf.width as u32;
        let h = buf.height as u32;
        let file_size = 54 + (w * h * 4);
        let mut header = [0u8; 54];
        header[0] = b'B';
        header[1] = b'M';
        header[2..6].copy_from_slice(&file_size.to_le_bytes());
        header[10..14].copy_from_slice(&54u32.to_le_bytes());
        header[14..18].copy_from_slice(&40u32.to_le_bytes());
        header[18..22].copy_from_slice(&(w as i32).to_le_bytes());
        header[22..26].copy_from_slice(&(-(h as i32)).to_le_bytes()); // Top-down
        header[26..28].copy_from_slice(&1u16.to_le_bytes());
        header[28..30].copy_from_slice(&32u16.to_le_bytes());
        header[30..34].copy_from_slice(&0u32.to_le_bytes());
        header[34..38].copy_from_slice(&(w * h * 4).to_le_bytes());

        let mut f = File::create(path).unwrap();
        f.write_all(&header).unwrap();
        let slice: &[u8] = unsafe {
            std::slice::from_raw_parts(buf.data.as_ptr() as *const u8, buf.data.len() * 4)
        };
        f.write_all(slice).unwrap();
    }

    #[test]
    fn test_render_lens_diffusion_comparison() {
        let w = 820;
        let h = 340;

        // 1. Create a simulated desktop background with wallpaper gradient + distracting text
        let mut bg = PixelBuffer::new(w, h);
        for y in 0..h {
            let t = y as f32 / h as f32;
            let r = (20.0 + t * 30.0) as u32;
            let g = (40.0 + t * 50.0) as u32;
            let b = (90.0 + t * 80.0) as u32;
            for x in 0..w {
                bg.data[y * w + x] = (0xFF << 24) | (r << 16) | (g << 8) | b;
            }
        }

        let tr = TextRenderer::new();
        // High-contrast white and yellow text that would normally clash behind the notch
        tr.draw_text(&mut bg, "CONFIDENTIAL REPORT 2026: FINANCIAL ARCHITECTURE", 60.0, 45.0, 22.0, Color::WHITE, true);
        tr.draw_text(&mut bg, "Quarterly Earnings Overview & Executive Strategy Planning Document", 80.0, 85.0, 20.0, Color::rgba(255, 230, 120, 1.0), false);
        tr.draw_text(&mut bg, "All departments must submit their reports by end of this week", 100.0, 125.0, 19.0, Color::WHITE, false);
        tr.draw_text(&mut bg, "System benchmark metrics: Latency < 1ms, Memory footprint 6.2 MB", 70.0, 165.0, 21.0, Color::rgba(140, 220, 255, 1.0), true);
        tr.draw_text(&mut bg, "Do not share this link with unauthorized external parties", 90.0, 205.0, 18.0, Color::WHITE, false);

        // 2. Prepare BackdropBlur with this background
        let mut bd = BackdropBlur {
            raw_buffer: bg.data.clone(),
            blurred_buffer: vec![0; w * h],
            temp_buffer: vec![0; w * h],
            width: w,
            height: h,
            scale: 1,
            hdc_mem: windows::Win32::Graphics::Gdi::HDC::default(),
            hbitmap: windows::Win32::Graphics::Gdi::HBITMAP::default(),
        };
        bd.apply_optical_blur(8);

        // 3. Render 3 test images:
        // A) Current Light Mode (Without blur)
        let mut img_light_noblur = bg.clone();
        let mut notch_buf = PixelBuffer::new(w, h);
        draw_top_notch_liquid_glass_core(&mut notch_buf, 110.0, 600.0, 220.0, 36.0, 1.0, 0.0);
        tr.draw_text(&mut notch_buf, "Happily ever after", 200.0, 60.0, 24.0, Color::WHITE, true);
        tr.draw_text(&mut notch_buf, "Shota Nakagawa", 200.0, 95.0, 16.0, Color::rgba(255, 255, 255, 0.7), false);
        tr.draw_text(&mut notch_buf, "kazoe kirenai hoshikuzu no naka", 430.0, 80.0, 18.0, Color::WHITE, false);
        // Composite onto bg
        for py in 0..h {
            for px in 0..w {
                let p = notch_buf.data[py * w + px];
                let a = ((p >> 24) & 0xFF) as f32 / 255.0;
                if a > 0.0 {
                    img_light_noblur.blend_pixel(px, py, ((p >> 16) & 0xFF) as u8, ((p >> 8) & 0xFF) as u8, (p & 0xFF) as u8, a);
                }
            }
        }
        save_bmp("C:\\Users\\localAccount\\.gemini\\antigravity\\brain\\f483ed7d-707b-4895-ac6e-27d84a8fd9f2\\thumbs\\compare_light_NO_blur.bmp", &img_light_noblur);

        // B) Experimental Light Mode (WITH Lens Diffusion Blur)
        let mut img_light_blur = bg.clone();
        let mut notch_blur_buf = PixelBuffer::new(w, h);
        draw_top_notch_liquid_glass_with_backdrop(&mut notch_blur_buf, 110.0, 600.0, 220.0, 36.0, 1.0, 0.0, &bd);
        tr.draw_text(&mut notch_blur_buf, "Happily ever after", 200.0, 60.0, 24.0, Color::WHITE, true);
        tr.draw_text(&mut notch_blur_buf, "Shota Nakagawa", 200.0, 95.0, 16.0, Color::rgba(255, 255, 255, 0.7), false);
        tr.draw_text(&mut notch_blur_buf, "kazoe kirenai hoshikuzu no naka", 430.0, 80.0, 18.0, Color::WHITE, false);
        for py in 0..h {
            for px in 0..w {
                let p = notch_blur_buf.data[py * w + px];
                let a = ((p >> 24) & 0xFF) as f32 / 255.0;
                if a > 0.0 {
                    img_light_blur.blend_pixel(px, py, ((p >> 16) & 0xFF) as u8, ((p >> 8) & 0xFF) as u8, (p & 0xFF) as u8, a);
                }
            }
        }
        save_bmp("C:\\Users\\localAccount\\.gemini\\antigravity\\brain\\f483ed7d-707b-4895-ac6e-27d84a8fd9f2\\thumbs\\compare_light_WITH_blur.bmp", &img_light_blur);

        // C) Experimental Dark Mode (WITH Lens Diffusion Blur)
        let mut img_dark_blur = bg.clone();
        let mut notch_dark_buf = PixelBuffer::new(w, h);
        draw_top_notch_liquid_glass_with_backdrop(&mut notch_dark_buf, 110.0, 600.0, 220.0, 36.0, 1.0, 1.0, &bd);
        tr.draw_text(&mut notch_dark_buf, "Happily ever after", 200.0, 60.0, 24.0, Color::WHITE, true);
        tr.draw_text(&mut notch_dark_buf, "Shota Nakagawa", 200.0, 95.0, 16.0, Color::rgba(255, 255, 255, 0.7), false);
        tr.draw_text(&mut notch_dark_buf, "kazoe kirenai hoshikuzu no naka", 430.0, 80.0, 18.0, Color::WHITE, false);
        for py in 0..h {
            for px in 0..w {
                let p = notch_dark_buf.data[py * w + px];
                let a = ((p >> 24) & 0xFF) as f32 / 255.0;
                if a > 0.0 {
                    img_dark_blur.blend_pixel(px, py, ((p >> 16) & 0xFF) as u8, ((p >> 8) & 0xFF) as u8, (p & 0xFF) as u8, a);
                }
            }
        }
        save_bmp("C:\\Users\\localAccount\\.gemini\\antigravity\\brain\\f483ed7d-707b-4895-ac6e-27d84a8fd9f2\\thumbs\\compare_dark_WITH_blur.bmp", &img_dark_blur);
    }
}

impl Drop for BackdropBlur {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.hbitmap);
            let _ = DeleteDC(self.hdc_mem);
        }
    }
}
