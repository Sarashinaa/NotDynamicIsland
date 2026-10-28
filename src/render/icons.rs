#![allow(dead_code)]

//! Procedural Crisp Vector Icons
//! Renders sharp icons without relying on external image files.

use super::buffer::PixelBuffer;
use super::primitives::{draw_circle, draw_rounded_rect, Color};

/// Renders a Play triangle
pub fn draw_play_icon(buf: &mut PixelBuffer, x: f32, y: f32, size: f32, color: Color) {
    let min_x = x.floor().max(0.0) as usize;
    let max_x = (x + size).ceil().min(buf.width as f32) as usize;
    let min_y = y.floor().max(0.0) as usize;
    let max_y = (y + size).ceil().min(buf.height as f32) as usize;

    let p0 = (x, y);
    let p1 = (x + size, y + size * 0.5);
    let p2 = (x, y + size);

    for py in min_y..max_y {
        let p_yf = py as f32 + 0.5;
        for px in min_x..max_x {
            let p_xf = px as f32 + 0.5;

            // Barycentric coordinates point-in-triangle test
            let d1 = (p_xf - p1.0) * (p0.1 - p1.1) - (p0.0 - p1.0) * (p_yf - p1.1);
            let d2 = (p_xf - p2.0) * (p1.1 - p2.1) - (p1.0 - p2.0) * (p_yf - p2.1);
            let d3 = (p_xf - p0.0) * (p2.1 - p0.1) - (p2.0 - p0.0) * (p_yf - p0.1);

            let has_neg = (d1 < 0.0) || (d2 < 0.0) || (d3 < 0.0);
            let has_pos = (d1 > 0.0) || (d2 > 0.0) || (d3 > 0.0);

            if !(has_neg && has_pos) {
                buf.blend_pixel(px, py, color.r, color.g, color.b, color.a);
            }
        }
    }
}

/// Renders a Pause icon (two vertical bars)
pub fn draw_pause_icon(buf: &mut PixelBuffer, x: f32, y: f32, size: f32, color: Color) {
    let bar_w = (size * 0.28).max(2.0);
    let gap = size * 0.24;
    let r = bar_w * 0.4;
    draw_rounded_rect(buf, x, y, bar_w, size, r, color);
    draw_rounded_rect(buf, x + bar_w + gap, y, bar_w, size, r, color);
}

/// Renders Next Track icon (triangle + bar)
pub fn draw_next_icon(buf: &mut PixelBuffer, x: f32, y: f32, size: f32, color: Color) {
    let tri_size = size * 0.75;
    draw_play_icon(buf, x, y + (size - tri_size) * 0.5, tri_size, color);
    let bar_w = (size * 0.16).max(2.0);
    draw_rounded_rect(buf, x + tri_size + 2.0, y, bar_w, size, 1.0, color);
}

/// Renders Previous Track icon (bar + left triangle)
pub fn draw_prev_icon(buf: &mut PixelBuffer, x: f32, y: f32, size: f32, color: Color) {
    let bar_w = (size * 0.16).max(2.0);
    draw_rounded_rect(buf, x, y, bar_w, size, 1.0, color);
    let tri_size = size * 0.75;

    let tri_x = x + bar_w + 2.0;
    let tri_y = y + (size - tri_size) * 0.5;

    let min_x = tri_x.floor().max(0.0) as usize;
    let max_x = (tri_x + tri_size).ceil().min(buf.width as f32) as usize;
    let min_y = tri_y.floor().max(0.0) as usize;
    let max_y = (tri_y + tri_size).ceil().min(buf.height as f32) as usize;

    let p0 = (tri_x + tri_size, tri_y);
    let p1 = (tri_x, tri_y + tri_size * 0.5);
    let p2 = (tri_x + tri_size, tri_y + tri_size);

    for py in min_y..max_y {
        let p_yf = py as f32 + 0.5;
        for px in min_x..max_x {
            let p_xf = px as f32 + 0.5;
            let d1 = (p_xf - p1.0) * (p0.1 - p1.1) - (p0.0 - p1.0) * (p_yf - p1.1);
            let d2 = (p_xf - p2.0) * (p1.1 - p2.1) - (p1.0 - p2.0) * (p_yf - p2.1);
            let d3 = (p_xf - p0.0) * (p2.1 - p0.1) - (p2.0 - p0.0) * (p_yf - p0.1);
            let has_neg = (d1 < 0.0) || (d2 < 0.0) || (d3 < 0.0);
            let has_pos = (d1 > 0.0) || (d2 > 0.0) || (d3 > 0.0);
            if !(has_neg && has_pos) {
                buf.blend_pixel(px, py, color.r, color.g, color.b, color.a);
            }
        }
    }
}

/// Renders Speaker Volume Icon
pub fn draw_speaker_icon(
    buf: &mut PixelBuffer,
    x: f32,
    y: f32,
    size: f32,
    muted: bool,
    color: Color,
) {
    // Speaker base box
    let box_w = size * 0.3;
    let box_h = size * 0.45;
    let box_y = y + (size - box_h) * 0.5;
    draw_rounded_rect(buf, x, box_y, box_w, box_h, 1.5, color);

    // Speaker cone trapezoid
    let cone_start_x = x + box_w - 1.0;
    let cone_w = size * 0.35;
    let min_x = cone_start_x.floor().max(0.0) as usize;
    let max_x = (cone_start_x + cone_w).ceil().min(buf.width as f32) as usize;
    let min_y = y.floor().max(0.0) as usize;
    let max_y = (y + size).ceil().min(buf.height as f32) as usize;

    for py in min_y..max_y {
        let p_yf = py as f32 + 0.5;
        for px in min_x..max_x {
            let p_xf = px as f32 + 0.5;
            let progress = (p_xf - cone_start_x) / cone_w;
            if progress >= 0.0 && progress <= 1.0 {
                let top_y = box_y + (y - box_y) * progress;
                let bot_y = (box_y + box_h) + ((y + size) - (box_y + box_h)) * progress;
                if p_yf >= top_y && p_yf <= bot_y {
                    buf.blend_pixel(px, py, color.r, color.g, color.b, color.a);
                }
            }
        }
    }

    if muted {
        // Red cross (muted)
        let cx = x + size * 0.85;
        let cy = y + size * 0.5;
        let s = size * 0.22;
        let r = 1.0;
        let red = Color::rgba(239, 68, 68, color.a);
        for i in 0..10 {
            let t = (i as f32 / 9.0) * 2.0 - 1.0;
            draw_circle(buf, cx + t * s, cy + t * s, r, red);
            draw_circle(buf, cx + t * s, cy - t * s, r, red);
        }
    } else {
        // Sound waves
        let cx = x + size * 0.45;
        let cy = y + size * 0.5;
        for r_ratio in [0.45, 0.7] {
            let r = size * r_ratio;
            for deg in -40..=40 {
                let rad = (deg as f32).to_radians();
                let px = cx + rad.cos() * r;
                let py = cy + rad.sin() * r;
                draw_circle(buf, px, py, 1.0, color);
            }
        }
    }
}

/// Renders Battery Icon
pub fn draw_battery_icon(
    buf: &mut PixelBuffer,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    percentage: u32,
    is_charging: bool,
    _color: Color,
) {
    let r = 3.0;
    // Outer shell
    draw_rounded_rect(buf, x, y, w - 3.0, h, r, Color::rgba(100, 100, 120, 0.4));
    // Terminal bump
    let bump_h = h * 0.4;
    let bump_y = y + (h - bump_h) * 0.5;
    draw_rounded_rect(buf, x + w - 3.0, bump_y, 3.0, bump_h, 1.0, Color::rgba(160, 160, 180, 0.7));

    // Inner fill
    let pad = 2.0;
    let fill_max_w = w - 3.0 - pad * 2.0;
    let fill_w = (fill_max_w * (percentage as f32 / 100.0)).max(2.0);
    let fill_color = if is_charging {
        Color::rgba(52, 211, 153, 0.95) // Emerald
    } else if percentage <= 20 {
        Color::rgba(239, 68, 68, 0.95) // Red
    } else {
        Color::rgba(255, 255, 255, 0.9)
    };

    draw_rounded_rect(buf, x + pad, y + pad, fill_w, h - pad * 2.0, r - 1.0, fill_color);

    if is_charging {
        // Draw small lightning bolt in center
        draw_lightning_icon(buf, x + (w - 3.0) * 0.5 - 4.0, y + h * 0.5 - 6.0, 8.0, 12.0, Color::BLACK);
    }
}

/// Renders a sharp Lightning Bolt
pub fn draw_lightning_icon(
    buf: &mut PixelBuffer,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Color,
) {
    let pts = [
        (x + w * 0.6, y),
        (x + w * 0.1, y + h * 0.52),
        (x + w * 0.45, y + h * 0.52),
        (x + w * 0.35, y + h),
        (x + w * 0.9, y + h * 0.45),
        (x + w * 0.55, y + h * 0.45),
    ];

    for i in 0..pts.len() {
        let p_start = pts[i];
        let p_end = pts[(i + 1) % pts.len()];
        let steps = 15;
        for s in 0..=steps {
            let t = s as f32 / steps as f32;
            let px = p_start.0 + (p_end.0 - p_start.0) * t;
            let py = p_start.1 + (p_end.1 - p_start.1) * t;
            draw_circle(buf, px, py, 1.2, color);
        }
    }
}

/// Renders animated Audio Equalizer Waveform bars
pub fn draw_equalizer_bars(
    buf: &mut PixelBuffer,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    anim_time: f32,
    color: Color,
) {
    let num_bars = 4;
    let bar_w = (w / (num_bars as f32 * 1.8)).max(2.5);
    let gap = (w - (num_bars as f32 * bar_w)) / ((num_bars - 1) as f32).max(1.0);

    for i in 0..num_bars {
        let bar_x = x + (i as f32) * (bar_w + gap);
        // Sinusoidal heights simulating musical animation
        let speed = 6.0;
        let phase = i as f32 * 1.6;
        let raw_h = ((anim_time * speed + phase).sin() * 0.5 + 0.5).clamp(0.2, 1.0);
        let bar_h = (h * raw_h).max(3.0);
        let bar_y = y + (h - bar_h);
        draw_rounded_rect(buf, bar_x, bar_y, bar_w, bar_h, bar_w * 0.5, color);
    }
}

/// Renders a Close (X) circle button
pub fn draw_close_button(buf: &mut PixelBuffer, cx: f32, cy: f32, radius: f32, is_hovered: bool) {
    let bg_color = if is_hovered {
        Color::rgba(255, 255, 255, 0.25)
    } else {
        Color::rgba(255, 255, 255, 0.12)
    };
    draw_circle(buf, cx, cy, radius, bg_color);

    let x_size = radius * 0.45;
    let color = Color::rgba(255, 255, 255, 0.85);
    for i in 0..10 {
        let t = (i as f32 / 9.0) * 2.0 - 1.0;
        draw_circle(buf, cx + t * x_size, cy + t * x_size, 0.8, color);
        draw_circle(buf, cx + t * x_size, cy - t * x_size, 0.8, color);
    }
}

/// Renders a File Shelf Icon
pub fn draw_file_icon(buf: &mut PixelBuffer, x: f32, y: f32, size: f32, color: Color) {
    let w = size * 0.75;
    let h = size;
    let corner = size * 0.25;
    let r = 2.0;

    // Body
    draw_rounded_rect(buf, x, y, w, h, r, color);
    // Inner fold contrast
    draw_rounded_rect(buf, x + w - corner, y, corner, corner, 1.0, Color::rgba(0, 0, 0, 0.3));
}

/// Renders the Mascot Robot Eyes from resting.png
pub fn draw_mascot_eyes(
    buf: &mut PixelBuffer,
    cx: f32,
    cy: f32,
    anim_time: f32,
    look_dx: f32,
    look_dy: f32,
    opacity: f32,
) {
    if opacity <= 0.01 {
        return;
    }

    // Blink simulation every ~3.5 seconds
    let cycle = anim_time % 3.5;
    let blink_scale = if cycle < 0.15 {
        // Closing and opening
        (1.0 - (cycle / 0.15 * std::f32::consts::PI).sin()).max(0.15)
    } else {
        1.0
    };

    let eye_w: f32 = 6.0;
    let eye_h = 10.0 * blink_scale;
    let eye_gap: f32 = 5.0;
    let r = (eye_w * 0.5).min(eye_h * 0.5);

    let offset_x = look_dx.clamp(-1.5, 1.5);
    let offset_y = look_dy.clamp(-1.5, 1.5);

    let left_x = cx - eye_w - eye_gap * 0.5 + offset_x;
    let right_x = cx + eye_gap * 0.5 + offset_x;
    let eye_y = cy - eye_h * 0.5 + offset_y;

    // Glowing cyan ambient aura
    let glow_color = Color::rgba(56, 189, 248, 0.22 * opacity);
    draw_circle(buf, left_x + eye_w * 0.5, eye_y + eye_h * 0.5, 8.0, glow_color);
    draw_circle(buf, right_x + eye_w * 0.5, eye_y + eye_h * 0.5, 8.0, glow_color);

    // Crisp white robot eyes
    let eye_color = Color::rgba(255, 255, 255, 0.98 * opacity);
    draw_rounded_rect(buf, left_x, eye_y, eye_w, eye_h, r, eye_color);
    draw_rounded_rect(buf, right_x, eye_y, eye_w, eye_h, r, eye_color);
}

/// Renders the Circular Battery Ring with support for Energy Saver & Charging states
pub fn draw_battery_circle_ring(
    buf: &mut PixelBuffer,
    cx: f32,
    cy: f32,
    radius: f32,
    percent: u32,
    is_charging: bool,
    is_energy_saver: bool,
    opacity: f32,
) {
    let thickness = 2.5;
    let progress = (percent as f32 / 100.0).clamp(0.05, 1.0);
    let bg_color = Color::rgba(255, 255, 255, 0.15 * opacity);
    let fg_color = if is_energy_saver {
        // User requirement: #EAA300 when Energy Saver is active
        Color::rgba(234, 163, 0, 0.98 * opacity)
    } else if is_charging {
        Color::rgba(34, 197, 94, 0.95 * opacity) // vibrant green
    } else if percent <= 20 {
        Color::rgba(239, 68, 68, 0.95 * opacity) // red low battery
    } else if percent <= 50 {
        // User requirement: #EAA300 when battery <= 50%
        Color::rgba(234, 163, 0, 0.98 * opacity)
    } else {
        Color::rgba(34, 197, 94, 0.95 * opacity)
    };

    super::primitives::draw_ring_meter(buf, cx, cy, radius, thickness, progress, bg_color, fg_color);

    if is_charging {
        draw_lightning_bolt(buf, cx, cy, radius * 0.8, Color::rgba(255, 255, 255, 0.95 * opacity));
    }
}

/// Renders the Circular Download Progress Ring with an elegant downward arrow inside
pub fn draw_download_circle_ring(
    buf: &mut PixelBuffer,
    cx: f32,
    cy: f32,
    radius: f32,
    progress: f32,
    opacity: f32,
) {
    let thickness = 2.5;
    let clamp_prog = progress.clamp(0.04, 1.0);
    let bg_color = Color::rgba(255, 255, 255, 0.15 * opacity);
    // Vibrant electric sky blue
    let fg_color = Color::rgba(56, 189, 248, 0.98 * opacity);

    super::primitives::draw_ring_meter(buf, cx, cy, radius, thickness, clamp_prog, bg_color, fg_color);

    // Downward download arrow inside the ring
    draw_download_arrow(buf, cx, cy, radius * 0.75, Color::rgba(255, 255, 255, 0.95 * opacity));
}

/// Renders a sharp, elegant downward arrow inside the download circle
pub fn draw_download_arrow(buf: &mut PixelBuffer, cx: f32, cy: f32, size: f32, color: Color) {
    let stem_h = size * 0.7;
    let arrow_w = size * 0.6;

    let stem_top = cy - stem_h * 0.5;
    let stem_bot = cy + stem_h * 0.35;
    let steps = 6;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let py = stem_top + t * (stem_bot - stem_top);
        draw_circle(buf, cx, py, 0.9, color);
    }

    let wing_steps = 5;
    for i in 0..=wing_steps {
        let t = i as f32 / wing_steps as f32;
        let lx = (cx - arrow_w) + t * arrow_w;
        let ly = (cy - 0.5) + t * (stem_bot - (cy - 0.5));
        draw_circle(buf, lx, ly, 0.9, color);

        let rx = (cx + arrow_w) - t * arrow_w;
        let ry = (cy - 0.5) + t * (stem_bot - (cy - 0.5));
        draw_circle(buf, rx, ry, 0.9, color);
    }
}

/// Renders a sharp Sun Icon for Display Brightness notification
pub fn draw_sun_icon(buf: &mut PixelBuffer, cx: f32, cy: f32, r: f32, color: Color) {
    // Center circle
    draw_circle(buf, cx, cy, r * 0.52, color);

    // 8 sun rays
    let ray_len = r * 0.35;
    let ray_r = r * 0.72;
    for i in 0..8 {
        let angle = (i as f32) * (std::f32::consts::PI / 4.0);
        let rx1 = cx + angle.cos() * ray_r;
        let ry1 = cy + angle.sin() * ray_r;
        let rx2 = cx + angle.cos() * (ray_r + ray_len);
        let ry2 = cy + angle.sin() * (ray_r + ray_len);
        let mid_x = (rx1 + rx2) * 0.5;
        let mid_y = (ry1 + ry2) * 0.5;
        draw_circle(buf, mid_x, mid_y, 1.2, color);
    }
}

/// Renders a notification Bell icon
pub fn draw_bell_icon(buf: &mut PixelBuffer, cx: f32, cy: f32, size: f32, color: Color) {
    let top_y = cy - size * 0.45;
    let bot_y = cy + size * 0.25;
    // Bell body
    draw_circle(buf, cx, top_y + 3.0, 3.0, color);
    draw_rounded_rect(buf, cx - size * 0.35, cy - 2.0, size * 0.7, size * 0.45, 3.0, color);
    // Bell lip
    draw_rounded_rect(buf, cx - size * 0.48, bot_y - 2.0, size * 0.96, 3.0, 1.5, color);
    // Clapper
    draw_circle(buf, cx, bot_y + 2.5, 2.0, color);
}

/// Renders a sharp Lightning Bolt
pub fn draw_lightning_bolt(buf: &mut PixelBuffer, cx: f32, cy: f32, size: f32, color: Color) {
    // 5-segment zigzag approximation
    let h = size * 0.5;
    for i in 0..6 {
        let t = i as f32 / 5.0;
        let px = cx - 1.5 + t * 1.5;
        let py = cy - h + t * (h * 0.9);
        draw_circle(buf, px, py, 0.9, color);
    }
    for i in 0..6 {
        let t = i as f32 / 5.0;
        let px = cx + 1.5 - t * 2.0;
        let py = cy - 1.0 + t * (h * 1.1);
        draw_circle(buf, px, py, 0.9, color);
    }
}

/// Renders the Monochrome Sound Wave from closed.png
pub fn draw_monochrome_sound_wave(
    buf: &mut PixelBuffer,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    is_playing: bool,
    anim_time: f32,
    opacity: f32,
) {
    let bar_count = 5;
    let bar_w = 2.4;
    let gap = (w - (bar_count as f32 * bar_w)) / (bar_count as f32 - 1.0);
    let color = Color::rgba(255, 255, 255, 0.95 * opacity);

    for i in 0..bar_count {
        let bar_x = x + (i as f32) * (bar_w + gap);
        let bar_h = if is_playing {
            let speed = 7.0;
            let phase = i as f32 * 1.4;
            let wave = ((anim_time * speed + phase).sin() * 0.5 + 0.5).clamp(0.25, 1.0);
            (h * wave).max(3.0)
        } else {
            3.0 // quiet idle dot
        };

        let bar_y = y + (h - bar_h) * 0.5;
        draw_rounded_rect(buf, bar_x, bar_y, bar_w, bar_h, bar_w * 0.5, color);
    }
}

/// Renders Navigation Arrow Circle Button (< or >) from music.png & shelf.png
pub fn draw_nav_arrow_button(
    buf: &mut PixelBuffer,
    cx: f32,
    cy: f32,
    radius: f32,
    is_right: bool,
    is_hovered: bool,
    opacity: f32,
) {
    let bg_color = if is_hovered {
        Color::rgba(255, 255, 255, 0.28 * opacity)
    } else {
        Color::rgba(255, 255, 255, 0.14 * opacity)
    };
    draw_circle(buf, cx, cy, radius, bg_color);

    let arrow_color = Color::rgba(255, 255, 255, 0.95 * opacity);
    let arrow_size = radius * 0.42;

    let dir = if is_right { 1.0 } else { -1.0 };
    for i in 0..7 {
        let t = i as f32 / 6.0;
        let px = cx - dir * arrow_size * 0.5 + dir * t * arrow_size;
        let py_top = cy - (1.0 - t) * arrow_size;
        let py_bot = cy + (1.0 - t) * arrow_size;
        draw_circle(buf, px, py_top, 0.8, arrow_color);
        draw_circle(buf, px, py_bot, 0.8, arrow_color);
    }
}

/// Renders the Bottom Page Indicator Dots (e.g. — • •) from music.png, shelf.png, to-do.png
pub fn draw_page_indicators(
    buf: &mut PixelBuffer,
    cx: f32,
    cy: f32,
    active_idx: usize,
    total: usize,
    opacity: f32,
) {
    let dot_spacing = 11.0;
    let total_w = (total - 1) as f32 * dot_spacing + 12.0;
    let start_x = cx - total_w * 0.5;

    let mut cur_x = start_x;
    for i in 0..total {
        if i == active_idx {
            // Active elongated pill
            let pill_w = 14.0;
            let pill_h = 4.0;
            draw_rounded_rect(
                buf,
                cur_x,
                cy - pill_h * 0.5,
                pill_w,
                pill_h,
                pill_h * 0.5,
                Color::rgba(255, 255, 255, 0.95 * opacity),
            );
            cur_x += pill_w + 6.0;
        } else {
            // Inactive round dot
            draw_circle(
                buf,
                cur_x + 2.0,
                cy,
                2.2,
                Color::rgba(255, 255, 255, 0.35 * opacity),
            );
            cur_x += dot_spacing;
        }
    }
}

/// Renders Lyrics Speech Bubble Button 💬 from music.png & lyrics.png
pub fn draw_lyrics_button(
    buf: &mut PixelBuffer,
    cx: f32,
    cy: f32,
    size: f32,
    is_active: bool,
    is_hovered: bool,
    opacity: f32,
) {
    if is_active || is_hovered {
        let bg = if is_active {
            Color::rgba(56, 189, 248, 0.28 * opacity) // blue glow active
        } else {
            Color::rgba(255, 255, 255, 0.18 * opacity)
        };
        draw_circle(buf, cx, cy, size * 0.65, bg);
    }

    let icon_color = if is_active {
        Color::rgba(56, 189, 248, 0.98 * opacity)
    } else {
        Color::rgba(255, 255, 255, 0.85 * opacity)
    };

    let w = size * 0.8;
    let h = size * 0.6;
    let x = cx - w * 0.5;
    let y = cy - h * 0.5;
    draw_rounded_rect(buf, x, y, w, h, 3.0, icon_color);

    // Inner dark dots inside speech bubble
    let inner_dot_color = Color::rgba(0, 0, 0, 0.8 * opacity);
    draw_circle(buf, cx - 3.0, cy, 1.0, inner_dot_color);
    draw_circle(buf, cx, cy, 1.0, inner_dot_color);
    draw_circle(buf, cx + 3.0, cy, 1.0, inner_dot_color);
}

/// Renders To-Do Checkbox (Green Checkmark or Gray Circle) from to-do.png
pub fn draw_todo_checkbox(
    buf: &mut PixelBuffer,
    cx: f32,
    cy: f32,
    radius: f32,
    done: bool,
    is_hovered: bool,
    opacity: f32,
) {
    if done {
        // Apple-style elegant frosted silver completed badge
        let color = Color::rgba(255, 255, 255, 0.35 * opacity);
        draw_circle(buf, cx, cy, radius, color);

        // Crisp white checkmark glyph
        let check_color = Color::rgba(255, 255, 255, 0.98 * opacity);
        for i in 0..5 {
            let t = i as f32 / 4.0;
            draw_circle(buf, cx - 3.5 + t * 2.0, cy + t * 2.0, 0.8, check_color);
        }
        for i in 0..7 {
            let t = i as f32 / 6.0;
            draw_circle(buf, cx - 1.5 + t * 4.5, cy + 2.0 - t * 5.0, 0.8, check_color);
        }
    } else {
        // Inactive circle with border
        let bg = if is_hovered {
            Color::rgba(255, 255, 255, 0.18 * opacity)
        } else {
            Color::rgba(255, 255, 255, 0.08 * opacity)
        };
        draw_circle(buf, cx, cy, radius, bg);
        let border = Color::rgba(255, 255, 255, 0.4 * opacity);
        draw_circle(buf, cx, cy, radius - 1.5, bg);
        // Draw thin border ring with full bounding coverage (no clipping on bottom or right!)
        let min_x = (cx - radius - 1.5).floor().max(0.0) as usize;
        let max_x = (cx + radius + 1.5).ceil().min(buf.width as f32 - 1.0) as usize;
        let min_y = (cy - radius - 1.5).floor().max(0.0) as usize;
        let max_y = (cy + radius + 1.5).ceil().min(buf.height as f32 - 1.0) as usize;
        for py in min_y..=max_y {
            for px in min_x..=max_x {
                let d = ((px as f32 + 0.5 - cx).hypot(py as f32 + 0.5 - cy) - radius).abs();
                if d < 1.0 {
                    buf.blend_pixel(px, py, border.r, border.g, border.b, (1.0 - d) * border.a);
                }
            }
        }
    }
}

/// Renders Headphones Badge from headphones.png
pub fn draw_headphones_badge(buf: &mut PixelBuffer, cx: f32, cy: f32, radius: f32, opacity: f32) {
    let bg = Color::rgba(30, 41, 59, 0.7 * opacity);
    draw_circle(buf, cx, cy, radius, bg);

    let ring = Color::rgba(56, 189, 248, 0.5 * opacity);
    // Draw outer subtle cyan ring
    for py in (cy - radius) as usize..(cy + radius) as usize {
        for px in (cx - radius) as usize..(cx + radius) as usize {
            let d = ((px as f32 + 0.5 - cx).hypot(py as f32 + 0.5 - cy) - (radius - 1.5)).abs();
            if d < 1.0 {
                buf.blend_pixel(px, py, ring.r, ring.g, ring.b, (1.0 - d) * ring.a);
            }
        }
    }

    // Headphones headband arc & earcups
    let hp_color = Color::rgba(255, 255, 255, 0.95 * opacity);
    let arc_r = radius * 0.5;
    for i in 0..16 {
        let angle = std::f32::consts::PI + (i as f32 / 15.0) * std::f32::consts::PI;
        let px = cx + angle.cos() * arc_r;
        let py = cy - 2.0 + angle.sin() * arc_r;
        draw_circle(buf, px, py, 1.2, hp_color);
    }
    // Left & right earcups
    draw_rounded_rect(buf, cx - arc_r - 2.0, cy - 3.0, 3.5, 7.0, 1.5, hp_color);
    draw_rounded_rect(buf, cx + arc_r - 1.5, cy - 3.0, 3.5, 7.0, 1.5, hp_color);
}

/// Renders Seek Backward Button (↺ 5) for scrubbing -5 seconds
pub fn draw_seek_backward_button(
    buf: &mut PixelBuffer,
    text_renderer: &super::text::TextRenderer,
    cx: f32,
    cy: f32,
    size: f32,
    is_hovered: bool,
    opacity: f32,
) {
    if is_hovered {
        draw_circle(buf, cx, cy, size * 0.55, Color::rgba(255, 255, 255, 0.16 * opacity));
    }

    let color = Color::rgba(255, 255, 255, if is_hovered { 0.98 } else { 0.88 } * opacity);
    let r = size * 0.36;

    // Draw circular arc leaving top open (-0.35 PI to 1.35 PI)
    let steps = 22;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let angle = -0.35 * std::f32::consts::PI + t * 1.50 * std::f32::consts::PI;
        let px = cx + angle.cos() * r;
        let py = cy + angle.sin() * r;
        draw_circle(buf, px, py, 0.70, color);
    }

    // Arrowhead at top-left pointing down-left
    let arrow_x = cx + (-0.35 * std::f32::consts::PI).cos() * r;
    let arrow_y = cy + (-0.35 * std::f32::consts::PI).sin() * r;
    for i in 0..4 {
        let t = i as f32 * 0.75;
        draw_circle(buf, arrow_x - t * 0.8, arrow_y + t * 0.3, 0.65, color);
        draw_circle(buf, arrow_x + t * 0.3, arrow_y + t * 0.8, 0.65, color);
    }

    // Digit "5" centered in the arc
    text_renderer.draw_text(
        buf,
        "5",
        cx - 2.5,
        cy + 3.0,
        8.5,
        color,
        true,
    );
}

/// Renders Seek Forward Button (↻ 5) for scrubbing +5 seconds
pub fn draw_seek_forward_button(
    buf: &mut PixelBuffer,
    text_renderer: &super::text::TextRenderer,
    cx: f32,
    cy: f32,
    size: f32,
    is_hovered: bool,
    opacity: f32,
) {
    if is_hovered {
        draw_circle(buf, cx, cy, size * 0.55, Color::rgba(255, 255, 255, 0.16 * opacity));
    }

    let color = Color::rgba(255, 255, 255, if is_hovered { 0.98 } else { 0.88 } * opacity);
    let r = size * 0.36;

    // Draw circular arc leaving top open (flipped horizontally)
    let steps = 22;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let angle = -0.35 * std::f32::consts::PI + t * 1.50 * std::f32::consts::PI;
        let px = cx - angle.cos() * r;
        let py = cy + angle.sin() * r;
        draw_circle(buf, px, py, 0.70, color);
    }

    // Arrowhead at top-right pointing down-right
    let arrow_x = cx - (-0.35 * std::f32::consts::PI).cos() * r;
    let arrow_y = cy + (-0.35 * std::f32::consts::PI).sin() * r;
    for i in 0..4 {
        let t = i as f32 * 0.75;
        draw_circle(buf, arrow_x + t * 0.8, arrow_y + t * 0.3, 0.65, color);
        draw_circle(buf, arrow_x - t * 0.3, arrow_y + t * 0.8, 0.65, color);
    }

    // Digit "5" centered in the arc
    text_renderer.draw_text(
        buf,
        "5",
        cx - 2.5,
        cy + 3.0,
        8.5,
        color,
        true,
    );
}

/// Renders Camera Icon (Apple Style)
pub fn draw_camera_icon(buf: &mut PixelBuffer, cx: f32, cy: f32, size: f32, color: Color) {
    let w = size;
    let h = size * 0.75;
    let r = size * 0.18;
    // Camera body
    draw_rounded_rect(buf, cx - w * 0.5, cy - h * 0.4, w, h, r, color);
    // Top flash bump
    draw_rounded_rect(buf, cx - w * 0.25, cy - h * 0.4 - 2.0, w * 0.25, 2.5, 1.0, color);
    // Center lens circle cutout (dark center, border color)
    draw_circle(buf, cx, cy + h * 0.1, size * 0.24, Color::rgba(0, 0, 0, 0.7 * color.a));
    draw_circle(buf, cx, cy + h * 0.1, size * 0.12, color);
}

/// Renders Microphone Icon with optional mute slash (Apple Style)
pub fn draw_microphone_icon(buf: &mut PixelBuffer, cx: f32, cy: f32, size: f32, muted: bool, color: Color) {
    let capsule_w = size * 0.42;
    let capsule_h = size * 0.72;
    let r = capsule_w * 0.5;
    // Center mic capsule
    draw_rounded_rect(buf, cx - r, cy - size * 0.48, capsule_w, capsule_h, r, color);
    
    // U-stand arc around the capsule
    let u_r = size * 0.38;
    for i in 0..12 {
        let t = i as f32 / 11.0;
        let angle = std::f32::consts::PI * t;
        let px = cx + angle.cos() * u_r;
        let py = cy - size * 0.05 + angle.sin() * u_r;
        draw_circle(buf, px, py, 0.7, color);
    }
    // Vertical stem & base stand
    draw_rounded_rect(buf, cx - 0.75, cy + u_r - size * 0.05, 1.5, size * 0.28, 0.7, color);
    draw_rounded_rect(buf, cx - size * 0.3, cy + u_r + size * 0.20, size * 0.6, 1.5, 0.7, color);

    // Red diagonal slash if muted
    if muted {
        let slash_color = Color::rgba(239, 68, 68, color.a);
        for i in 0..20 {
            let t = (i as f32 / 19.0) - 0.5;
            let px = cx + t * size * 0.9;
            let py = cy - t * size * 0.9;
            draw_circle(buf, px, py, 1.0, slash_color);
        }
    }
}

/// Renders Stopwatch / Focus Timer Icon (Apple Style)
pub fn draw_timer_icon(buf: &mut PixelBuffer, cx: f32, cy: f32, size: f32, color: Color) {
    let r = size * 0.42;
    // Top stem / button
    draw_rounded_rect(buf, cx - 1.0, cy - r - 3.5, 2.0, 3.5, 0.8, color);
    draw_rounded_rect(buf, cx - 3.0, cy - r - 4.5, 6.0, 1.5, 0.7, color);
    // Outer clock ring
    for py in (cy - r - 1.0) as usize..=(cy + r + 1.0) as usize {
        for px in (cx - r - 1.0) as usize..=(cx + r + 1.0) as usize {
            let d = ((px as f32 + 0.5 - cx).hypot(py as f32 + 0.5 - cy) - r).abs();
            if d < 1.0 {
                buf.blend_pixel(px, py, color.r, color.g, color.b, (1.0 - d) * color.a);
            }
        }
    }
    // Clock hands (needle pointing to 2 o'clock)
    draw_circle(buf, cx, cy, 1.2, color);
    for i in 1..=6 {
        let t = i as f32 * (r * 0.12);
        draw_circle(buf, cx + t * 0.7, cy - t * 0.7, 0.7, color);
        if i <= 4 {
            draw_circle(buf, cx, cy - t * 0.8, 0.7, color);
        }
    }
}

/// Renders Eyedropper / Color Picker Icon
pub fn draw_eyedropper_icon(buf: &mut PixelBuffer, cx: f32, cy: f32, size: f32, color: Color) {
    let s = size * 0.4;
    // Angled dropper body (45 degrees)
    for i in 0..12 {
        let t = (i as f32 / 11.0) * s * 1.6;
        let px = cx - s * 0.8 + t;
        let py = cy + s * 0.8 - t;
        draw_circle(buf, px, py, 1.2, color);
    }
    // Dropper tip
    draw_circle(buf, cx - s * 0.9, cy + s * 0.9, 0.8, color);
    // Reservoir bulb at top-right
    draw_circle(buf, cx + s * 0.9, cy - s * 0.9, s * 0.45, color);
}

/// Renders Screen Snip / Crop Icon
pub fn draw_snip_icon(buf: &mut PixelBuffer, cx: f32, cy: f32, size: f32, color: Color) {
    let r = size * 0.38;
    // Top-left corner
    draw_rounded_rect(buf, cx - r, cy - r, r * 1.2, 1.5, 0.7, color);
    draw_rounded_rect(buf, cx - r, cy - r, 1.5, r * 1.2, 0.7, color);
    // Bottom-right corner
    draw_rounded_rect(buf, cx + r - r * 1.2, cy + r - 1.5, r * 1.2, 1.5, 0.7, color);
    draw_rounded_rect(buf, cx + r - 1.5, cy + r - r * 1.2, 1.5, r * 1.2, 0.7, color);
    // Center crosshair
    draw_circle(buf, cx, cy, 1.0, color);
}
