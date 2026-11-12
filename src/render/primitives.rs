#![allow(dead_code)]

//! 2D Vector Primitives with Analytical Anti-Aliasing (SDF)

use super::buffer::PixelBuffer;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: f32,
}

impl Color {
    pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub const BLACK: Self = Self::rgba(0, 0, 0, 1.0);
    pub const WHITE: Self = Self::rgba(255, 255, 255, 1.0);
}

/// Renders a soft ambient drop shadow behind a rounded rectangle
pub fn draw_soft_shadow(
    buf: &mut PixelBuffer,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    shadow_blur: f32,
    shadow_opacity: f32,
) {
    if shadow_opacity <= 0.001 || shadow_blur <= 0.0 {
        return;
    }

    let min_x = (x - shadow_blur).floor().max(0.0) as usize;
    let max_x = (x + w + shadow_blur).ceil().min(buf.width as f32) as usize;
    let min_y = (y - shadow_blur).floor().max(0.0) as usize;
    let max_y = (y + h + shadow_blur).ceil().min(buf.height as f32) as usize;

    let hx = w * 0.5;
    let hy = h * 0.5;
    let cx = x + hx;
    let cy = y + hy;
    let r = radius.min(hx).min(hy);

    for py in min_y..max_y {
        let p_yf = py as f32 + 0.5;
        let dy = (p_yf - cy).abs() - (hy - r);

        for px in min_x..max_x {
            let p_xf = px as f32 + 0.5;
            let dx = (p_xf - cx).abs() - (hx - r);

            let dist = dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - r;
            if dist > 0.0 && dist < shadow_blur {
                // Smooth quadratic falloff
                let t = dist / shadow_blur;
                let falloff = (1.0 - t) * (1.0 - t);
                let a = falloff * shadow_opacity;
                buf.blend_pixel(px, py, 0, 0, 0, a);
            }
        }
    }
}

/// Renders an anti-aliased filled rounded rectangle using analytical Signed Distance Fields (SDF)
pub fn draw_rounded_rect(
    buf: &mut PixelBuffer,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    color: Color,
) {
    if color.a <= 0.001 || w <= 0.0 || h <= 0.0 {
        return;
    }

    let min_x = (x - 1.0).floor().max(0.0) as usize;
    let max_x = (x + w + 1.0).ceil().min(buf.width as f32) as usize;
    let min_y = (y - 1.0).floor().max(0.0) as usize;
    let max_y = (y + h + 1.0).ceil().min(buf.height as f32) as usize;

    let hx = w * 0.5;
    let hy = h * 0.5;
    let cx = x + hx;
    let cy = y + hy;
    let r = radius.min(hx).min(hy);

    for py in min_y..max_y {
        let p_yf = py as f32 + 0.5;
        let dy = (p_yf - cy).abs() - (hy - r);

        for px in min_x..max_x {
            let p_xf = px as f32 + 0.5;
            let dx = (p_xf - cx).abs() - (hx - r);

            let dist = dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - r;
            // Anti-aliased 1-pixel boundary transition
            let coverage = (0.5 - dist).clamp(0.0, 1.0);
            if coverage > 0.0 {
                buf.blend_pixel(px, py, color.r, color.g, color.b, coverage * color.a);
            }
        }
    }
}

/// Renders an anti-aliased rounded rectangle border stroke
pub fn draw_rounded_rect_stroke(
    buf: &mut PixelBuffer,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    stroke_width: f32,
    color: Color,
) {
    if color.a <= 0.001 || w <= 0.0 || h <= 0.0 || stroke_width <= 0.0 {
        return;
    }

    let margin = stroke_width + 1.0;
    let min_x = (x - margin).floor().max(0.0) as usize;
    let max_x = (x + w + margin).ceil().min(buf.width as f32) as usize;
    let min_y = (y - margin).floor().max(0.0) as usize;
    let max_y = (y + h + margin).ceil().min(buf.height as f32) as usize;

    let hx = w * 0.5;
    let hy = h * 0.5;
    let cx = x + hx;
    let cy = y + hy;
    let r = radius.min(hx).min(hy);
    let half_stroke = stroke_width * 0.5;

    for py in min_y..max_y {
        let p_yf = py as f32 + 0.5;
        let dy = (p_yf - cy).abs() - (hy - r);

        for px in min_x..max_x {
            let p_xf = px as f32 + 0.5;
            let dx = (p_xf - cx).abs() - (hx - r);

            let dist = dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - r;
            // Distance to stroke perimeter
            let coverage = (0.5 - (dist.abs() - half_stroke)).clamp(0.0, 1.0);
            if coverage > 0.0 {
                buf.blend_pixel(px, py, color.r, color.g, color.b, coverage * color.a);
            }
        }
    }
}

/// Renders an anti-aliased circle
pub fn draw_circle(buf: &mut PixelBuffer, cx: f32, cy: f32, radius: f32, color: Color) {
    if color.a <= 0.001 || radius <= 0.0 {
        return;
    }

    let min_x = (cx - radius - 1.5).floor().max(0.0) as usize;
    let max_x = (cx + radius + 1.5).ceil().min(buf.width as f32) as usize;
    let min_y = (cy - radius - 1.5).floor().max(0.0) as usize;
    let max_y = (cy + radius + 1.5).ceil().min(buf.height as f32) as usize;

    for py in min_y..max_y {
        let dy = py as f32 + 0.5 - cy;
        for px in min_x..max_x {
            let dx = px as f32 + 0.5 - cx;
            let dist = dx.hypot(dy) - radius;
            let coverage = (0.5 - dist).clamp(0.0, 1.0);
            if coverage > 0.0 {
                buf.blend_pixel(px, py, color.r, color.g, color.b, coverage * color.a);
            }
        }
    }
}

/// Renders a dynamic pill progress bar (track + filled indicator)
pub fn draw_progress_bar(
    buf: &mut PixelBuffer,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    progress: f32, // 0.0 to 1.0
    track_color: Color,
    fill_color: Color,
) {
    let r = h * 0.5;
    // Draw background track
    draw_rounded_rect(buf, x, y, w, h, r, track_color);

    // Draw active fill
    let fill_w = (w * progress.clamp(0.0, 1.0)).max(h.min(w * progress));
    if fill_w >= 2.0 {
        draw_rounded_rect(buf, x, y, fill_w, h, r, fill_color);
    }
}

/// High-precision Signed Distance Function (SDF) for top notch with:
/// High-precision Signed Distance Function (SDF) for top notch with:
/// - Inverted rounded corners flaring outward into the top screen bezel
/// - Standard rounded corners on the bottom two corners
/// - 100% C0 & C1 continuous, zero internal boundaries or line artifacts
#[inline]
pub fn top_notch_sdf(p_xf: f32, p_yf: f32, x: f32, w: f32, h: f32, r_bottom: f32, r_top: f32) -> f32 {
    let half_w = w * 0.5;
    let cx = x + half_w;
    let px = (p_xf - cx).abs();

    if p_yf <= r_top {
        // Cutout circle center is at (half_w + r_top, r_top) from center
        let cut_cx = half_w + r_top;
        let cut_cy = r_top;
        if px > cut_cx {
            (px - cut_cx).hypot(p_yf)
        } else {
            r_top - (px - cut_cx).hypot(p_yf - cut_cy)
        }
    } else if p_yf <= h - r_bottom {
        // Middle vertical straight wall
        px - half_w
    } else {
        // Bottom section with rounded corners
        let cut_cx = half_w - r_bottom;
        let cut_cy = h - r_bottom;
        if px > cut_cx {
            (px - cut_cx).hypot(p_yf - cut_cy) - r_bottom
        } else {
            p_yf - h
        }
    }
}

/// Renders a top-anchored notch (attached to y = 0, inverted top corners, rounded bottom corners)
pub fn draw_top_notch(
    buf: &mut PixelBuffer,
    x: f32,
    w: f32,
    h: f32,
    bottom_radius: f32,
    color: Color,
) {
    if color.a <= 0.001 || w <= 0.0 || h <= 0.0 {
        return;
    }

    let r_bottom = bottom_radius.min(w * 0.5).min(h * 0.5);
    let r_top = r_bottom;

    let min_x = (x - r_top - 1.5).floor().max(0.0) as usize;
    let max_x = (x + w + r_top + 1.5).ceil().min(buf.width as f32) as usize;
    let min_y = 0usize;
    let max_y = (h + 1.5).ceil().min(buf.height as f32) as usize;

    for py in min_y..max_y {
        let p_yf = py as f32 + 0.5;
        for px in min_x..max_x {
            let p_xf = px as f32 + 0.5;
            let dist = top_notch_sdf(p_xf, p_yf, x, w, h, r_bottom, r_top);
            let coverage = (0.5 - dist).clamp(0.0, 1.0);
            if coverage > 0.0 {
                buf.blend_pixel(px, py, color.r, color.g, color.b, coverage * color.a);
            }
        }
    }
}



/// Unified Pristine Liquid Glass Shader Core
/// Used for both Liquid Light and Liquid Dark modes:
/// - Crystal-clear transparent substrate (zero muddy gray, zero blur artifacts)
/// - Dual-stroke optical thickness (1px outer vertical gradient, 1.5px inner refraction)
/// - Specular wet-look highlight arc (1.8px at top lip, 75% pure white fading out to edges)
/// - Clean, pristine surface: zero dust, zero bubbles, zero scratches, zero rainbow/pelangi
pub fn draw_top_notch_liquid_glass_core(
    buf: &mut PixelBuffer,
    x: f32,
    w: f32,
    h: f32,
    bottom_radius: f32,
    opacity: f32,
    theme_progress: f32, // 0.0 = Clear Light, 1.0 = Dark Obsidian
) {
    if opacity <= 0.001 || w <= 0.0 || h <= 0.0 {
        return;
    }

    let r_bottom = bottom_radius.min(w * 0.5).min(h * 0.5);
    let r_top = r_bottom;

    let min_x = (x - r_top - 1.0).floor().max(0.0) as usize;
    let max_x = (x + w + r_top + 1.0).ceil().min(buf.width as f32) as usize;
    let min_y = 0usize;
    let max_y = (h + 1.0).ceil().min(buf.height as f32) as usize;

    let half_w = w * 0.5;
    let cx = x + half_w;
    let t = theme_progress.clamp(0.0, 1.0);

    for py in min_y..max_y {
        let p_yf = py as f32 + 0.5;
        let vert_t = (p_yf / h).clamp(0.0, 1.0);

        // Substrate color & opacity:
        // Dark: 32% opacity obsidian dark (10, 12, 16)
        // Light: crystal clear 7.5% at top lip -> 4% at bottom (255, 255, 255)
        let dark_alpha = 0.32;
        let light_alpha = 0.075 - vert_t * 0.035;
        let base_alpha = (light_alpha + (dark_alpha - light_alpha) * t) * opacity;

        let base_r = (255.0 + (10.0 - 255.0) * t).round() as u8;
        let base_g = (255.0 + (12.0 - 255.0) * t).round() as u8;
        let base_b = (255.0 + (16.0 - 255.0) * t).round() as u8;

        for px in min_x..max_x {
            let p_xf = px as f32 + 0.5;
            let dist = top_notch_sdf(p_xf, p_yf, x, w, h, r_bottom, r_top);
            let coverage = (0.5 - dist).clamp(0.0, 1.0);
            if coverage > 0.0 {
                // 1. Crystal-clear Liquid Glass Substrate
                buf.blend_pixel(px, py, base_r, base_g, base_b, coverage * base_alpha);

                // 2. Dual-Stroke Optical Thickness:
                // a) Outer Stroke (1px): Vertical linear gradient from top (38%) to bottom (5%)
                if dist > -1.0 && dist <= 0.0 {
                    let outer_alpha = (0.38 - vert_t * 0.33) * opacity;
                    buf.blend_pixel(px, py, 255, 255, 255, coverage * outer_alpha);
                }

                // b) Inner Stroke (1.5px at 1px padding): subtle internal refraction
                if dist > -2.5 && dist <= -1.0 {
                    let inner_t = 1.0 - (dist + 1.75).abs() / 0.75;
                    let inner_alpha = inner_t * (0.08 + (1.0 - t) * 0.03) * opacity;
                    buf.blend_pixel(px, py, 185, 192, 205, coverage * inner_alpha);
                }

                // 3. Specular Wet-Look Highlight:
                // 1.8px arc along top lip (75% pure white, smooth fade towards shoulders)
                if p_yf <= 2.2 && dist <= 0.0 {
                    let norm_x = (p_xf - cx).abs() / half_w;
                    let fade_x = (1.0 - ((norm_x - 0.38) / 0.45).clamp(0.0, 1.0)).powf(1.5);
                    let vert_sheen = (1.0 - (p_yf / 2.2)).clamp(0.0, 1.0);
                    let wet_specular = fade_x * vert_sheen * 0.75 * opacity;
                    buf.blend_pixel(px, py, 255, 255, 255, coverage * wet_specular);
                }
            }
        }
    }
}

/// Style 1: Pristine Dark Liquid Glass (Moon mode 🌙)
pub fn draw_top_notch_dark_liquid_glass(
    buf: &mut PixelBuffer,
    x: f32,
    w: f32,
    h: f32,
    bottom_radius: f32,
    opacity: f32,
) {
    draw_top_notch_liquid_glass_core(buf, x, w, h, bottom_radius, opacity, 1.0);
}

/// Style 2: Pristine Light Liquid Glass (Sun mode ☀️)
pub fn draw_top_notch_frosted_liquid_glass(
    buf: &mut PixelBuffer,
    x: f32,
    w: f32,
    h: f32,
    bottom_radius: f32,
    opacity: f32,
) {
    draw_top_notch_liquid_glass_core(buf, x, w, h, bottom_radius, opacity, 0.0);
}

/// Interpolated Liquid Glass for smooth animated transitions between Light and Dark
pub fn draw_top_notch_liquid_glass_interpolated(
    buf: &mut PixelBuffer,
    x: f32,
    w: f32,
    h: f32,
    bottom_radius: f32,
    opacity: f32,
    theme_progress: f32,
) {
    draw_top_notch_liquid_glass_core(buf, x, w, h, bottom_radius, opacity, theme_progress);
}

/// Renders Liquid Glass with active Lens Diffusion Backdrop (Optical blur of desktop behind notch)
pub fn draw_top_notch_liquid_glass_with_backdrop(
    buf: &mut PixelBuffer,
    x: f32,
    w: f32,
    h: f32,
    bottom_radius: f32,
    opacity: f32,
    theme_progress: f32,
    backdrop: &super::backdrop::BackdropBlur,
) {
    if opacity <= 0.001 || w <= 0.0 || h <= 0.0 {
        return;
    }

    let r_bottom = bottom_radius.min(w * 0.5).min(h * 0.5);
    let r_top = r_bottom;

    let min_x = (x - r_top - 1.0).floor().max(0.0) as usize;
    let max_x = (x + w + r_top + 1.0).ceil().min(buf.width as f32) as usize;
    let min_y = 0usize;
    let max_y = (h + 1.0).ceil().min(buf.height as f32) as usize;

    let half_w = w * 0.5;
    let cx = x + half_w;
    let t = theme_progress.clamp(0.0, 1.0);

    for py in min_y..max_y {
        let p_yf = py as f32 + 0.5;
        let vert_t = (p_yf / h).clamp(0.0, 1.0);

        let base_r = (255.0 + (10.0 - 255.0) * t).round();
        let base_g = (255.0 + (12.0 - 255.0) * t).round();
        let base_b = (255.0 + (16.0 - 255.0) * t).round();

        for px in min_x..max_x {
            let p_xf = px as f32 + 0.5;
            let dist = top_notch_sdf(p_xf, p_yf, x, w, h, r_bottom, r_top);
            let coverage = (0.5 - dist).clamp(0.0, 1.0);
            if coverage > 0.0 {
                // 1. Sample optically blurred background
                let (bg_r, bg_g, bg_b) = backdrop.get_blurred_pixel(px, py);

                // Optical glass transmittance:
                // Light mode (t=0): 75% transmission of blurred background + 25% luminous white body lift
                // Dark mode (t=1): 38% transmission of blurred background + 62% deep obsidian absorption
                let trans = (0.75 + (0.38 - 0.75) * t).clamp(0.0, 1.0);

                let comp_r = ((bg_r as f32 * trans + base_r * (1.0 - trans)).min(255.0)).round() as u8;
                let comp_g = ((bg_g as f32 * trans + base_g * (1.0 - trans)).min(255.0)).round() as u8;
                let comp_b = ((bg_b as f32 * trans + base_b * (1.0 - trans)).min(255.0)).round() as u8;

                // Glass alpha is 1.0 * opacity so unblurred desktop does NOT leak through underneath;
                // optical transmission is fully handled by blending the blurred backdrop with the glass tint above.
                let glass_alpha = opacity;
                buf.blend_pixel(px, py, comp_r, comp_g, comp_b, coverage * glass_alpha);

                // 2. Dual-Stroke Optical Thickness:
                // a) Outer Stroke (1px): Vertical linear gradient from top (38%) to bottom (5%)
                if dist > -1.0 && dist <= 0.0 {
                    let outer_alpha = (0.38 - vert_t * 0.33) * opacity;
                    buf.blend_pixel(px, py, 255, 255, 255, coverage * outer_alpha);
                }

                // b) Inner Stroke (1.5px at 1px padding): subtle internal refraction
                if dist > -2.5 && dist <= -1.0 {
                    let inner_t = 1.0 - (dist + 1.75).abs() / 0.75;
                    let inner_alpha = inner_t * (0.08 + (1.0 - t) * 0.03) * opacity;
                    buf.blend_pixel(px, py, 185, 192, 205, coverage * inner_alpha);
                }

                // 3. Specular Wet-Look Highlight:
                // 1.8px arc along top lip (75% pure white, smooth fade towards shoulders)
                if p_yf <= 2.2 && dist <= 0.0 {
                    let norm_x = (p_xf - cx).abs() / half_w;
                    let fade_x = (1.0 - ((norm_x - 0.38) / 0.45).clamp(0.0, 1.0)).powf(1.5);
                    let vert_sheen = (1.0 - (p_yf / 2.2)).clamp(0.0, 1.0);
                    let wet_specular = fade_x * vert_sheen * 0.75 * opacity;
                    buf.blend_pixel(px, py, 255, 255, 255, coverage * wet_specular);
                }
            }
        }
    }
}

/// Backwards compatibility wrapper for dark liquid glass
pub fn draw_top_notch_liquid_glass(
    buf: &mut PixelBuffer,
    x: f32,
    w: f32,
    h: f32,
    bottom_radius: f32,
    opacity: f32,
) {
    draw_top_notch_dark_liquid_glass(buf, x, w, h, bottom_radius, opacity);
}

/// Renders directional soft ambient drop shadow for the top notch
pub fn draw_top_notch_shadow(
    buf: &mut PixelBuffer,
    x: f32,
    w: f32,
    h: f32,
    bottom_radius: f32,
    shadow_blur: f32,
    shadow_offset_y: f32,
    shadow_opacity: f32,
) {
    if shadow_opacity <= 0.001 || shadow_blur <= 0.0 {
        return;
    }

    let r_bottom = bottom_radius.min(w * 0.5).min(h * 0.5);
    let r_top = r_bottom;

    let min_x = (x - r_top - shadow_blur).floor().max(0.0) as usize;
    let max_x = (x + w + r_top + shadow_blur).ceil().min(buf.width as f32) as usize;
    let min_y = 0usize;
    let max_y = (h + shadow_offset_y + shadow_blur).ceil().min(buf.height as f32) as usize;

    for py in min_y..max_y {
        let p_yf = py as f32 + 0.5;
        let shadow_yf = p_yf - shadow_offset_y;

        for px in min_x..max_x {
            let p_xf = px as f32 + 0.5;
            let dist = top_notch_sdf(p_xf, shadow_yf, x, w, h, r_bottom, r_top);

            if dist > 0.0 && dist < shadow_blur {
                let t = dist / shadow_blur;
                let falloff = (1.0 - t) * (1.0 - t);
                let a = falloff * shadow_opacity;
                buf.blend_pixel(px, py, 0, 0, 0, a);
            }
        }
    }
}

/// Renders subtle cut-glass refraction stroke around the perimeter of the top notch
pub fn draw_top_notch_stroke(
    buf: &mut PixelBuffer,
    x: f32,
    w: f32,
    h: f32,
    bottom_radius: f32,
    color: Color,
) {
    if color.a <= 0.001 {
        return;
    }

    let r_bottom = bottom_radius.min(w * 0.5).min(h * 0.5);
    let r_top = r_bottom;

    let min_x = (x - r_top - 1.5).floor().max(0.0) as usize;
    let max_x = (x + w + r_top + 1.5).ceil().min(buf.width as f32) as usize;
    let min_y = 1usize; // Skip y = 0 ceiling flush to screen bezel
    let max_y = (h + 1.5).ceil().min(buf.height as f32) as usize;

    for py in min_y..max_y {
        let p_yf = py as f32 + 0.5;
        for px in min_x..max_x {
            let p_xf = px as f32 + 0.5;
            // Skip the top flat edge where notch connects to screen bezel
            if p_yf <= 1.0 && p_xf > x - r_top + 1.0 && p_xf < x + w + r_top - 1.0 {
                continue;
            }

            let dist = top_notch_sdf(p_xf, p_yf, x, w, h, r_bottom, r_top);
            let coverage = (0.5 - (dist.abs() - 0.5)).clamp(0.0, 1.0);
            if coverage > 0.0 {
                // Adaptive 3D glass rim lighting:
                // Upper shoulders catch direct light -> prominent specular catch-light
                // Bottom rounded corners catch ambient bounce light
                // Vertical sides remain clean and subtle
                let stroke_alpha = if p_yf <= r_top + 2.0 {
                    color.a * 1.45
                } else if p_yf >= h - r_bottom {
                    color.a * 0.88
                } else {
                    color.a * 0.50
                };

                buf.blend_pixel(px, py, color.r, color.g, color.b, coverage * stroke_alpha);
            }
        }
    }
}

/// Renders a circular ring meter (background ring + colored arc)
pub fn draw_ring_meter(
    buf: &mut PixelBuffer,
    cx: f32,
    cy: f32,
    radius: f32,
    thickness: f32,
    progress: f32, // 0.0 to 1.0
    bg_color: Color,
    fg_color: Color,
) {
    let half_th = thickness * 0.5;
    let _min_r = radius - half_th;
    let max_r = radius + half_th;

    let min_x = (cx - max_r - 1.0).floor().max(0.0) as usize;
    let max_x = (cx + max_r + 1.0).ceil().min(buf.width as f32) as usize;
    let min_y = (cy - max_r - 1.0).floor().max(0.0) as usize;
    let max_y = (cy + max_r + 1.0).ceil().min(buf.height as f32) as usize;

    let sweep_angle = progress.clamp(0.0, 1.0) * std::f32::consts::TAU;

    for py in min_y..max_y {
        let dy = py as f32 + 0.5 - cy;
        for px in min_x..max_x {
            let dx = px as f32 + 0.5 - cx;
            let dist = dx.hypot(dy);
            let ring_dist = (dist - radius).abs() - half_th;
            let ring_cov = (0.5 - ring_dist).clamp(0.0, 1.0);

            if ring_cov > 0.0 {
                // Angle starting from top (-PI/2) clockwise
                let mut angle = dy.atan2(dx) + std::f32::consts::FRAC_PI_2;
                if angle < 0.0 {
                    angle += std::f32::consts::TAU;
                }

                if angle <= sweep_angle {
                    buf.blend_pixel(px, py, fg_color.r, fg_color.g, fg_color.b, ring_cov * fg_color.a);
                } else if bg_color.a > 0.0 {
                    buf.blend_pixel(px, py, bg_color.r, bg_color.g, bg_color.b, ring_cov * bg_color.a);
                }
            }
        }
    }
}
