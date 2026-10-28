#![allow(dead_code)]

//! High-performance CPU 32-bit BGRA Pixel Buffer
//! Designed for zero-overhead Win32 UpdateLayeredWindow per-pixel alpha composition.

#[derive(Clone)]
pub struct PixelBuffer {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u32>, // 0xAARRGGBB in pre-multiplied format
}

impl PixelBuffer {
    pub fn new(width: usize, height: usize) -> Self {
        let size = width * height;
        Self {
            width,
            height,
            data: vec![0u32; size],
        }
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.data.fill(0);
    }

    #[inline(always)]
    pub fn width(&self) -> usize {
        self.width
    }

    #[inline(always)]
    pub fn height(&self) -> usize {
        self.height
    }

    #[inline(always)]
    pub fn as_ptr(&self) -> *const u32 {
        self.data.as_ptr()
    }

    /// Blends a pixel in-place with pre-multiplied source-over Porter-Duff formula.
    /// `alpha` is in range 0.0 ..= 1.0.
    #[inline]
    pub fn blend_pixel(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8, alpha: f32) {
        if x >= self.width || y >= self.height || alpha <= 0.001 {
            return;
        }

        let idx = y * self.width + x;
        let a = (alpha.min(1.0) * 255.0) as u32;
        if a == 0 {
            return;
        }

        // Source pre-multiplied components
        let sr = (r as u32 * a + 127) / 255;
        let sg = (g as u32 * a + 127) / 255;
        let sb = (b as u32 * a + 127) / 255;

        let dst = self.data[idx];
        if dst == 0 {
            // Target is fully transparent
            self.data[idx] = (a << 24) | (sr << 16) | (sg << 8) | sb;
            return;
        }

        // Destination components (already pre-multiplied)
        let da = (dst >> 24) & 0xFF;
        let dr = (dst >> 16) & 0xFF;
        let dg = (dst >> 8) & 0xFF;
        let db = dst & 0xFF;

        // Porter-Duff Source-Over with pre-multiplied alpha:
        // out_c = src_c + dst_c * (1 - src_a)
        let inv_a = 255 - a;
        let out_r = sr + (dr * inv_a + 127) / 255;
        let out_g = sg + (dg * inv_a + 127) / 255;
        let out_b = sb + (db * inv_a + 127) / 255;
        let out_a = a + (da * inv_a + 127) / 255;

        self.data[idx] = ((out_a.min(255)) << 24)
            | ((out_r.min(255)) << 16)
            | ((out_g.min(255)) << 8)
            | (out_b.min(255));
    }

    /// Blends an icon bitmap onto the buffer
    pub fn blend_icon(
        &mut self,
        dest_x: usize,
        dest_y: usize,
        pixels: &[u32],
        icon_w: usize,
        icon_h: usize,
        opacity: f32,
    ) {
        if opacity <= 0.001 {
            return;
        }
        for iy in 0..icon_h {
            let py = dest_y + iy;
            if py >= self.height {
                break;
            }
            for ix in 0..icon_w {
                let px = dest_x + ix;
                if px >= self.width {
                    break;
                }
                let src_px = pixels[iy * icon_w + ix];
                let a = ((src_px >> 24) & 0xFF) as f32 / 255.0 * opacity;
                if a > 0.01 {
                    let r = ((src_px >> 16) & 0xFF) as u8;
                    let g = ((src_px >> 8) & 0xFF) as u8;
                    let b = (src_px & 0xFF) as u8;
                    self.blend_pixel(px, py, r, g, b, a);
                }
            }
        }
    }
}
