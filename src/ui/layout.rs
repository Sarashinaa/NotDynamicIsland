//! Island Layout, Component Rendering, and Interaction Hit-Testing
//! Faithful implementation of follow_this_design with high-precision spacing and scrubbing

use crate::physics::SpringRect;
use crate::render::{
    draw_battery_circle_ring, draw_bell_icon, draw_camera_icon, draw_circle, draw_close_button,
    draw_eyedropper_icon, draw_headphones_badge, draw_lyrics_button, draw_microphone_icon, draw_monochrome_sound_wave,
    draw_nav_arrow_button, draw_next_icon, draw_page_indicators, draw_pause_icon, draw_play_icon, draw_prev_icon,
    draw_progress_bar, draw_rounded_rect, draw_rounded_rect_stroke, draw_seek_backward_button,
    draw_seek_forward_button, draw_snip_icon, draw_speaker_icon, draw_sun_icon, draw_timer_icon,
    draw_todo_checkbox, draw_top_notch, draw_top_notch_liquid_glass, draw_top_notch_shadow, draw_top_notch_stroke,
    Color, PixelBuffer, TextRenderer,
};
use crate::system::lyrics::SyncedLyrics;
use crate::ui::state::{ExpandedPage, IslandMode, IslandState, NotificationKind};

#[derive(Clone, Debug, PartialEq)]
pub enum ClickAction {
    TogglePlayPause,
    SkipNext,
    SkipPrev,
    SeekRelative(i32),
    SeekMedia(u32),
    ToggleLyrics,
    PrevPage,
    NextPage,
    SelectPage(ExpandedPage),
    OpenShelfItem(usize),
    RemoveShelfItem(usize),
    ToggleTodo(u32),
    PromptAddTask,
    CommitAddTask,
    CancelAddTask,
    ClearDoneTodo,
    ToggleTimer,
    ResetTimer,
    SetTimerMode(crate::system::timer::TimerMode),
    AdjustTimerMins(i32),
    ToggleMicMute,
    TriggerSnip,
    StartColorPicker,
}

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

#[derive(Clone, Debug)]
pub struct LyricLineLayout<'a> {
    pub timestamp: f32,
    pub y_offset: f32,
    pub is_wrapped: bool,
    pub line1: &'a str,
    pub line2: Option<&'a str>,
}

pub struct LayoutManager {
    pub click_targets: Vec<(Rect, ClickAction)>,
}

impl LayoutManager {
    pub fn new() -> Self {
        Self {
            click_targets: Vec::new(),
        }
    }

    /// Pre-computes variable-height offsets for synced lyrics.
    /// Ensures paragraph gaps between any two sentences are mathematically uniform (28.0px),
    /// while wrapped lines within a sentence stay tightly grouped (18.0px line-height).
    pub fn compute_lyric_layouts<'a>(
        text_renderer: &TextRenderer,
        lyrics: &'a SyncedLyrics,
        lyrics_max_w: f32,
    ) -> Vec<LyricLineLayout<'a>> {
        let mut layouts = Vec::with_capacity(lyrics.lines.len());
        let mut cur_y = 0.0f32;
        let line_step = 18.0f32;
        let item_gap = 28.0f32;

        for (timestamp, text) in &lyrics.lines {
            let (l1, l2_opt) = Self::wrap_lyric_line(text_renderer, text, 13.0, lyrics_max_w, true);
            let is_wrapped = l2_opt.is_some();
            let y = cur_y;

            if is_wrapped {
                cur_y += line_step + item_gap;
            } else {
                cur_y += item_gap;
            }

            layouts.push(LyricLineLayout {
                timestamp: *timestamp,
                y_offset: y,
                is_wrapped,
                line1: l1,
                line2: l2_opt,
            });
        }

        layouts
    }

    /// Calculates the smooth scroll target Y that centers the active lyric line at 74.0px.
    pub fn get_lyric_target_y(layouts: &[LyricLineLayout], active_idx: usize) -> f32 {
        if layouts.is_empty() {
            return 0.0;
        }
        let idx = active_idx.min(layouts.len() - 1);
        let item = &layouts[idx];
        if item.is_wrapped {
            item.y_offset + 9.0
        } else {
            item.y_offset
        }
    }

    /// Computes target dimensions anchored to the very top edge of the screen (y = 0.0)
    pub fn compute_target_geometry(
        &self,
        canvas_w: f32,
        state: &IslandState,
    ) -> (f32, f32, f32, f32, f32) {
        let (w, h, r) = match &state.mode {
            // Resting: Thinner, compact notch showing green battery ring + date (center) + time (right)
            IslandMode::Resting => (236.0, 32.0, 16.0),
            // Closed: Battery ring + centered date & time + sound wave
            IslandMode::Closed => (236.0, 32.0, 16.0),
            // Music: Increased height to 150px to provide generous breathing room above bottom dots
            IslandMode::Expanded(ExpandedPage::Music) => (392.0, 150.0, 24.0),
            // Shelf:
            IslandMode::Expanded(ExpandedPage::Shelf) => (426.0, 146.0, 24.0),
            // To-do: Increased height to 148px for beautiful typography spacing
            IslandMode::Expanded(ExpandedPage::ToDo) => (446.0, 148.0, 24.0),
            // Timer & Focus: Spacious 400x150, matches design aesthetic
            IslandMode::Expanded(ExpandedPage::Timer) => (400.0, 150.0, 24.0),
            // Tools & Control Center: 3 Apple-style utility cards (420x148)
            IslandMode::Expanded(ExpandedPage::Tools) => (420.0, 148.0, 24.0),
            // Lyrics: Generous width for synchronized scrolling lyrics
            IslandMode::Lyrics => (600.0, 150.0, 24.0),
            // Notifications:
            IslandMode::Notification(NotificationKind::Charging { .. }) => (330.0, 48.0, 22.0),
            IslandMode::Notification(NotificationKind::ChargerDisconnected { .. }) => (330.0, 48.0, 22.0),
            IslandMode::Notification(NotificationKind::Brightness { .. }) => (310.0, 44.0, 21.0),
            IslandMode::Notification(NotificationKind::Headphones { .. }) => (340.0, 48.0, 22.0),
            IslandMode::Notification(NotificationKind::Volume { .. }) => (310.0, 44.0, 21.0),
            IslandMode::Notification(NotificationKind::CapsLock { .. }) => (260.0, 42.0, 20.0),
            IslandMode::Notification(NotificationKind::Banner { .. }) => (440.0, 56.0, 24.0),
            IslandMode::Notification(NotificationKind::ColorPicked { .. }) => (320.0, 44.0, 21.0),
            IslandMode::Notification(NotificationKind::PrivacyAlert { .. }) => (340.0, 48.0, 22.0),
            IslandMode::Notification(NotificationKind::TimerCompleted { .. }) => (340.0, 48.0, 22.0),
        };

        let x = (canvas_w - w) * 0.5;
        let y = 0.0;
        (x, y, w, h, r)
    }

    /// Exact hit test geometry calculation for shelf cards (guaranteed 100% match with render_shelf)
    pub fn get_shelf_card_rect(
        &self,
        index: usize,
        file_count: usize,
        cur_x: f32,
        cur_w: f32,
    ) -> Rect {
        let box_x = cur_x + 44.0;
        let box_y = 36.0;
        let box_w = cur_w - 88.0;
        let box_h = 76.0;
        let card_w = (box_w - 16.0) / (file_count as f32).max(1.0);
        let card_x = box_x + 8.0 + (index as f32) * card_w;
        Rect {
            x: card_x,
            y: box_y + 6.0,
            w: (card_w - 6.0).max(10.0),
            h: box_h - 12.0,
        }
    }

pub const USE_DARK_LIQUID_GLASS: bool = true;

    /// Renders the complete dynamic island frame into pixel buffer
    pub fn render(
        &mut self,
        buf: &mut PixelBuffer,
        text_renderer: &TextRenderer,
        springs: &SpringRect,
        state: &IslandState,
        lyrics: &SyncedLyrics,
        cursor_pos: Option<(f32, f32)>,
    ) {
        self.click_targets.clear();

        let cur_x = springs.x.current;
        let cur_w = springs.width.current;
        let cur_h = springs.height.current;
        let cur_r = springs.corner_radius.current;
        let opacity = springs.opacity.current.clamp(0.0, 1.0);

        if cur_w <= 10.0 || cur_h <= 10.0 || opacity <= 0.01 {
            return;
        }

        if Self::USE_DARK_LIQUID_GLASS {
            // Style 1: Dark Liquid Glass with specular light reflection and double ambient depth
            draw_top_notch_shadow(buf, cur_x, cur_w, cur_h, cur_r, 22.0, 0.46 * opacity);
            draw_top_notch_shadow(buf, cur_x, cur_w, cur_h, cur_r, 8.0, 0.30 * opacity);

            draw_top_notch_liquid_glass(buf, cur_x, cur_w, cur_h, cur_r, opacity);

            // Refraction cut glass rim highlight
            let rim_color = Color::rgba(255, 255, 255, 0.20 * opacity);
            draw_top_notch_stroke(buf, cur_x, cur_w, cur_h, cur_r, rim_color);
        } else {
            // Style 2: Classic solid #07080c
            draw_top_notch_shadow(buf, cur_x, cur_w, cur_h, cur_r, 18.0, 0.42 * opacity);
            let body_color = Color::rgba(7, 8, 12, 0.98 * opacity);
            draw_top_notch(buf, cur_x, cur_w, cur_h, cur_r, body_color);
            let rim_color = Color::rgba(255, 255, 255, 0.15 * opacity);
            draw_top_notch_stroke(buf, cur_x, cur_w, cur_h, cur_r, rim_color);
        }

        // 4. Render component contents based on state
        match &state.mode {
            IslandMode::Resting => {
                self.render_resting(buf, text_renderer, cur_x, cur_w, cur_h, state, opacity);
            }
            IslandMode::Closed => {
                self.render_closed(buf, text_renderer, cur_x, cur_w, cur_h, state, opacity);
            }
            IslandMode::Expanded(ExpandedPage::Music) => {
                self.render_music(buf, text_renderer, cur_x, cur_w, cur_h, state, opacity, cursor_pos);
            }
            IslandMode::Expanded(ExpandedPage::Shelf) => {
                self.render_shelf(buf, text_renderer, cur_x, cur_w, cur_h, state, opacity, cursor_pos);
            }
            IslandMode::Expanded(ExpandedPage::ToDo) => {
                self.render_todo(buf, text_renderer, cur_x, cur_w, cur_h, state, opacity, cursor_pos);
            }
            IslandMode::Expanded(ExpandedPage::Timer) => {
                self.render_timer(buf, text_renderer, cur_x, cur_w, cur_h, state, opacity, cursor_pos);
            }
            IslandMode::Expanded(ExpandedPage::Tools) => {
                self.render_tools(buf, text_renderer, cur_x, cur_w, cur_h, state, opacity, cursor_pos);
            }
            IslandMode::Lyrics => {
                self.render_lyrics(buf, text_renderer, cur_x, cur_w, cur_h, state, lyrics, opacity, cursor_pos);
            }
            IslandMode::Notification(NotificationKind::Charging { percent }) => {
                self.render_charging(buf, text_renderer, cur_x, cur_w, cur_h, *percent, true, opacity);
            }
            IslandMode::Notification(NotificationKind::ChargerDisconnected { percent }) => {
                self.render_charging(buf, text_renderer, cur_x, cur_w, cur_h, *percent, false, opacity);
            }
            IslandMode::Notification(NotificationKind::Brightness { level }) => {
                self.render_brightness(buf, text_renderer, cur_x, cur_w, cur_h, *level, opacity);
            }
            IslandMode::Notification(NotificationKind::Headphones { name, battery, connected }) => {
                self.render_headphones(buf, text_renderer, cur_x, cur_w, cur_h, name, *battery, *connected, opacity);
            }
            IslandMode::Notification(NotificationKind::Volume { level, muted }) => {
                self.render_volume(buf, text_renderer, cur_x, cur_w, cur_h, *level, *muted, opacity);
            }
            IslandMode::Notification(NotificationKind::CapsLock { enabled }) => {
                self.render_caps_lock(buf, text_renderer, cur_x, cur_w, cur_h, *enabled, opacity);
            }
            IslandMode::Notification(NotificationKind::Banner { app_name, title, message }) => {
                self.render_banner(buf, text_renderer, cur_x, cur_w, cur_h, app_name, title, message, opacity);
            }
            IslandMode::Notification(NotificationKind::ColorPicked { hex, rgb }) => {
                self.render_color_picked(buf, text_renderer, cur_x, cur_w, cur_h, hex, rgb, opacity);
            }
            IslandMode::Notification(NotificationKind::PrivacyAlert { is_camera, app_name }) => {
                self.render_privacy_alert(buf, text_renderer, cur_x, cur_w, cur_h, *is_camera, app_name, opacity);
            }
            IslandMode::Notification(NotificationKind::TimerCompleted { message }) => {
                self.render_timer_completed(buf, text_renderer, cur_x, cur_w, cur_h, message, opacity);
            }
        }
    }

    /// Mode 1: Resting - Compact notch: Battery ring (left) + Date (center) + Time (right)
    /// If download is active: Download ring (left) + File Name (center) + Download % (right)
    fn render_resting(
        &mut self,
        buf: &mut PixelBuffer,
        text_renderer: &TextRenderer,
        x: f32,
        w: f32,
        h: f32,
        state: &IslandState,
        opacity: f32,
    ) {
        let cy = h * 0.5;

        // Left: Green Battery Ring
        let bat_cx = x + 18.0;
        draw_battery_circle_ring(
            buf,
            bat_cx,
            cy,
            6.0,
            state.battery_percent,
            state.is_charging,
            state.is_energy_saver,
            opacity,
        );

            let (date_str, time_str) = get_formatted_date_parts();

            // Rightmost: Privacy Dots (Camera Green Dot, Mic Orange Dot)
            let mut priv_x = x + w - 10.0;
            if state.privacy.camera_active {
                draw_circle(buf, priv_x, cy, 3.0, Color::rgba(34, 197, 94, 0.98 * opacity));
                priv_x -= 8.0;
            }
            if state.privacy.mic_active {
                let mic_color = if state.privacy.mic_muted {
                    Color::rgba(239, 68, 68, 0.98 * opacity)
                } else {
                    Color::rgba(249, 115, 22, 0.98 * opacity)
                };
                draw_circle(buf, priv_x, cy, 3.0, mic_color);
                let mic_rect = Rect { x: priv_x - 6.0, y: cy - 6.0, w: 12.0, h: 12.0 };
                self.click_targets.push((mic_rect, ClickAction::ToggleMicMute));
                priv_x -= 8.0;
            }

            let privacy_width = (x + w - 10.0) - priv_x;

            // Right: Local Time (Bold white, right-aligned before privacy dots)
            let (time_w, _) = text_renderer.measure_text(&time_str, 11.5, true);
            let time_x = x + w - 16.0 - privacy_width - time_w;
            text_renderer.draw_text(
                buf,
                &time_str,
                time_x,
                cy + 4.5,
                11.5,
                Color::rgba(255, 255, 255, 0.98 * opacity),
                true,
            );

            // Center: Day, Date, Year (or Network Speed if active transfer > 150 KB/s)
            let display_str = if state.network_speed.download_bytes_sec > 150_000 {
                format!("↓ {}", state.network_speed.format_download())
            } else {
                date_str
            };

            let (disp_w, _) = text_renderer.measure_text(&display_str, 10.5, false);
            let left_bound = bat_cx + 12.0;
            let right_bound = time_x - 6.0;
            let date_x = left_bound + (right_bound - left_bound - disp_w) * 0.5;
            text_renderer.draw_text(
                buf,
                &display_str,
                date_x,
                cy + 4.0,
                10.5,
                Color::rgba(203, 213, 225, 0.90 * opacity),
                false,
            );

        // Clicking expands to active page
        self.click_targets.push((Rect { x, y: 0.0, w, h }, ClickAction::NextPage));
    }

    /// Mode 2: Closed - Battery ring (left) + Centered Date & Time + Live Sound Wave (right)
    /// If download is active: Download ring (left) + Centered File & % + Live Sound Wave (right)
    fn render_closed(
        &mut self,
        buf: &mut PixelBuffer,
        text_renderer: &TextRenderer,
        x: f32,
        w: f32,
        h: f32,
        state: &IslandState,
        opacity: f32,
    ) {
        let cy = h * 0.5;

        // Right: Privacy dots & Live Monochrome Sound Wave
        let mut right_edge = x + w - 38.0;

        if state.privacy.camera_active || state.privacy.mic_active {
            let mut p_x = x + w - 10.0;
            if state.privacy.camera_active {
                draw_circle(buf, p_x, cy, 3.0, Color::rgba(34, 197, 94, 0.98 * opacity));
                p_x -= 8.0;
            }
            if state.privacy.mic_active {
                let mic_color = if state.privacy.mic_muted {
                    Color::rgba(239, 68, 68, 0.98 * opacity)
                } else {
                    Color::rgba(249, 115, 22, 0.98 * opacity)
                };
                draw_circle(buf, p_x, cy, 3.0, mic_color);
                let mic_rect = Rect { x: p_x - 6.0, y: cy - 6.0, w: 12.0, h: 12.0 };
                self.click_targets.push((mic_rect, ClickAction::ToggleMicMute));
                p_x -= 8.0;
            }
            right_edge = p_x - 24.0;
        }

        let wave_x = right_edge;
        let wave_y = cy - 6.0;
        draw_monochrome_sound_wave(
            buf,
            wave_x,
            wave_y,
            22.0,
            12.0,
            state.media_info.is_playing,
            state.anim_time,
            opacity,
        );

        // Left: Green Battery Ring
        let bat_cx = x + 18.0;
        draw_battery_circle_ring(
            buf,
            bat_cx,
            cy,
            6.0,
            state.battery_percent,
            state.is_charging,
            state.is_energy_saver,
            opacity,
        );

        // Center: Day, Date, Year & Time centered between battery & sound wave
        let (date_str, time_str) = get_formatted_date_parts();
        let full_date_time = format!("{} {}", date_str, time_str);
        let (text_w, _) = text_renderer.measure_text(&full_date_time, 10.5, false);

        let left_bound = bat_cx + 12.0;
        let right_bound = wave_x - 8.0;
        let text_x = left_bound + (right_bound - left_bound - text_w) * 0.5;

        text_renderer.draw_text(
            buf,
            &full_date_time,
            text_x,
            cy + 4.0,
            10.5,
            Color::rgba(255, 255, 255, 0.96 * opacity),
            false,
        );

        // Clicking expands to active page
        self.click_targets.push((Rect { x, y: 0.0, w, h }, ClickAction::NextPage));
    }

    /// Mode 3: Music - Complete media controls, album artwork, interactive seek bar, and page navigation
    fn render_music(
        &mut self,
        buf: &mut PixelBuffer,
        text_renderer: &TextRenderer,
        x: f32,
        w: f32,
        h: f32,
        state: &IslandState,
        opacity: f32,
        cursor_pos: Option<(f32, f32)>,
    ) {
        let _ = h;
        // Top Header: Battery Ring + Date & Time + Soundwave
        self.render_header_bar(buf, text_renderer, x, w, state, opacity);

        // Middle Row: Album Artwork Tile
        let art_size = 46.0;
        let art_x = x + 16.0;
        let art_y = 36.0;
        render_album_art_tile(
            buf,
            art_x,
            art_y,
            art_size,
            state.media_info.artwork_pixels.as_deref(),
            state.media_info.artwork_w,
            state.media_info.artwork_h,
            opacity,
        );

        // Track Title & Artist
        let text_x = art_x + art_size + 14.0;
        let text_max_w = w - text_x + x - 48.0;

        let has_track = state.media_info.has_media && !state.media_info.title.is_empty();
        let (title_buf, artist_buf) = if has_track {
            format_media_display(&state.media_info.title, &state.media_info.artist)
        } else {
            ("No Track Playing".to_string(), "Music Player".to_string())
        };
        let title = &title_buf;
        let artist = &artist_buf;

        text_renderer.draw_text_truncated(
            buf,
            title,
            text_x,
            art_y + 16.0,
            13.5,
            text_max_w,
            Color::rgba(255, 255, 255, 0.98 * opacity),
            true,
        );
        text_renderer.draw_text_truncated(
            buf,
            artist,
            text_x,
            art_y + 32.0,
            11.5,
            text_max_w,
            Color::rgba(156, 163, 175, 0.85 * opacity),
            false,
        );

        // Right Nav Arrow Button (>) to cycle to Shelf
        let next_page_btn_x = x + w - 26.0;
        let next_page_btn_y = art_y + 20.0;
        let next_page_rect = Rect {
            x: next_page_btn_x - 12.0,
            y: next_page_btn_y - 12.0,
            w: 24.0,
            h: 24.0,
        };
        let next_page_hover = cursor_pos.map(|(px, py)| next_page_rect.contains(px, py)).unwrap_or(false);
        draw_nav_arrow_button(buf, next_page_btn_x, next_page_btn_y, 11.0, true, next_page_hover, opacity);
        self.click_targets.push((next_page_rect, ClickAction::NextPage));

        // Interactive Progress Bar Row
        let bar_y = 90.0;
        let progress_x = x + 56.0;
        let progress_w = w - 112.0;

        let bar_rect = Rect {
            x: progress_x - 6.0,
            y: bar_y - 8.0,
            w: progress_w + 12.0,
            h: 16.0,
        };
        let is_bar_hovered = cursor_pos.map(|(mx, my)| bar_rect.contains(mx, my)).unwrap_or(false);
        let is_bar_active = is_bar_hovered || state.is_dragging_slider;

        let effective_secs = if state.is_dragging_slider {
            state.drag_seek_secs.unwrap_or(state.media_info.position_secs)
        } else {
            state.media_info.position_secs
        };

        let cur_time_str = format_seconds(effective_secs);
        let rem_time_str = if state.media_info.duration_secs > effective_secs {
            format!("-{}", format_seconds(state.media_info.duration_secs - effective_secs))
        } else {
            "-00:00".to_string()
        };

        text_renderer.draw_text(
            buf,
            &cur_time_str,
            x + 16.0,
            bar_y + 4.0,
            10.0,
            Color::rgba(156, 163, 175, 0.8 * opacity),
            false,
        );

        let progress_val = if state.media_info.duration_secs > 0 {
            (effective_secs as f32 / state.media_info.duration_secs as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // When hovered or dragged: thicker and brighter with visible circular thumb knob!
        let bar_h = if is_bar_active { 6.0 } else { 3.5 };
        let bar_bg = if is_bar_active {
            Color::rgba(255, 255, 255, 0.28 * opacity)
        } else {
            Color::rgba(255, 255, 255, 0.16 * opacity)
        };
        let bar_fill = if is_bar_active {
            Color::rgba(255, 255, 255, 1.0 * opacity)
        } else {
            Color::rgba(255, 255, 255, 0.88 * opacity)
        };

        // Render progress bar
        draw_progress_bar(
            buf,
            progress_x,
            bar_y - (bar_h * 0.5),
            progress_w,
            bar_h,
            progress_val,
            bar_bg,
            bar_fill,
        );

        // Prominent circular thumb knob for pulling/dragging the slider
        if (is_bar_active || is_bar_hovered) && state.media_info.duration_secs > 0 {
            let thumb_x = progress_x + progress_val * progress_w;
            let thumb_y = bar_y;
            // Outer halo glow
            draw_circle(buf, thumb_x, thumb_y, 7.5, Color::rgba(255, 255, 255, 0.38 * opacity));
            // Inner solid white knob
            draw_circle(buf, thumb_x, thumb_y, 5.5, Color::rgba(255, 255, 255, 1.0 * opacity));
        }

        // Progress bar click & seek target
        if let Some((mx, _)) = cursor_pos {
            if bar_rect.contains(mx, bar_y) && state.media_info.duration_secs > 0 {
                let frac = ((mx - progress_x) / progress_w).clamp(0.0, 1.0);
                let seek_secs = (frac * state.media_info.duration_secs as f32) as u32;
                self.click_targets.push((bar_rect, ClickAction::SeekMedia(seek_secs)));
            }
        }

        text_renderer.draw_text(
            buf,
            &rem_time_str,
            x + w - 48.0,
            bar_y + 4.0,
            10.0,
            Color::rgba(156, 163, 175, 0.8 * opacity),
            false,
        );

        // Bottom Controls Row: Positioned at ctrl_y = 112px
        let ctrl_y = 112.0;

        // Lyrics Button (speech bubble 💬) on left
        let lyrics_rect = Rect {
            x: x + 16.0,
            y: ctrl_y - 12.0,
            w: 24.0,
            h: 24.0,
        };
        let lyrics_hover = cursor_pos.map(|(px, py)| lyrics_rect.contains(px, py)).unwrap_or(false);
        draw_lyrics_button(buf, x + 28.0, ctrl_y, 18.0, false, lyrics_hover, opacity);
        self.click_targets.push((lyrics_rect, ClickAction::ToggleLyrics));

        // Center Media Controls: Previous, Play/Pause, Next
        let center_x = x + w * 0.5;

        // Prev Button
        let prev_rect = Rect {
            x: center_x - 48.0,
            y: ctrl_y - 12.0,
            w: 24.0,
            h: 24.0,
        };
        let prev_hover = cursor_pos.map(|(px, py)| prev_rect.contains(px, py)).unwrap_or(false);
        if prev_hover {
            draw_circle(buf, center_x - 36.0, ctrl_y, 12.0, Color::rgba(255, 255, 255, 0.14));
        }
        draw_prev_icon(buf, center_x - 43.0, ctrl_y - 6.0, 13.0, Color::rgba(255, 255, 255, 0.95 * opacity));
        self.click_targets.push((prev_rect, ClickAction::SkipPrev));

        // Play/Pause Button
        let play_rect = Rect {
            x: center_x - 14.0,
            y: ctrl_y - 14.0,
            w: 28.0,
            h: 28.0,
        };
        let play_hover = cursor_pos.map(|(px, py)| play_rect.contains(px, py)).unwrap_or(false);
        let play_bg = if play_hover {
            Color::rgba(255, 255, 255, 0.28 * opacity)
        } else {
            Color::rgba(255, 255, 255, 0.16 * opacity)
        };
        draw_circle(buf, center_x, ctrl_y, 14.0, play_bg);
        if state.media_info.is_playing {
            draw_pause_icon(buf, center_x - 5.0, ctrl_y - 6.0, 12.0, Color::rgba(255, 255, 255, 0.98 * opacity));
        } else {
            draw_play_icon(buf, center_x - 4.5, ctrl_y - 6.0, 12.0, Color::rgba(255, 255, 255, 0.98 * opacity));
        }
        self.click_targets.push((play_rect, ClickAction::TogglePlayPause));

        // Next Button
        let next_rect = Rect {
            x: center_x + 24.0,
            y: ctrl_y - 12.0,
            w: 24.0,
            h: 24.0,
        };
        let next_hover = cursor_pos.map(|(px, py)| next_rect.contains(px, py)).unwrap_or(false);
        if next_hover {
            draw_circle(buf, center_x + 36.0, ctrl_y, 12.0, Color::rgba(255, 255, 255, 0.14));
        }
        draw_next_icon(buf, center_x + 29.0, ctrl_y - 6.0, 13.0, Color::rgba(255, 255, 255, 0.95 * opacity));
        self.click_targets.push((next_rect, ClickAction::SkipNext));

        // Seek Backward 5s Button (↺ 5) on right side (opposite to video button)
        let seek_back_rect = Rect {
            x: x + w - 68.0,
            y: ctrl_y - 12.0,
            w: 24.0,
            h: 24.0,
        };
        let seek_back_hover = cursor_pos.map(|(px, py)| seek_back_rect.contains(px, py)).unwrap_or(false);
        draw_seek_backward_button(buf, text_renderer, x + w - 56.0, ctrl_y, 18.0, seek_back_hover, opacity);
        self.click_targets.push((seek_back_rect, ClickAction::SeekRelative(-5)));

        // Seek Forward 5s Button (↻ 5) on rightmost edge (opposite to lyrics button)
        let seek_fwd_rect = Rect {
            x: x + w - 40.0,
            y: ctrl_y - 12.0,
            w: 24.0,
            h: 24.0,
        };
        let seek_fwd_hover = cursor_pos.map(|(px, py)| seek_fwd_rect.contains(px, py)).unwrap_or(false);
        draw_seek_forward_button(buf, text_renderer, x + w - 28.0, ctrl_y, 18.0, seek_fwd_hover, opacity);
        self.click_targets.push((seek_fwd_rect, ClickAction::SeekRelative(5)));

        // Bottom Page Indicator Dots (— • • • •) positioned safely at cy = 138.0 with plenty of breathing room!
        let indicator_y = 138.0;
        draw_page_indicators(buf, center_x, indicator_y, 0, 5, opacity);
        self.register_page_dot_targets(center_x, indicator_y, 5);
    }

/// Intelligently wraps a single lyric line into up to two sub-lines if it exceeds max_width.
/// Prefers splitting at natural punctuation (, ; - — ... :) near the middle if both parts fit,
/// otherwise performs greedy word wrap on whitespace boundaries.
fn wrap_lyric_line<'a>(
    text_renderer: &TextRenderer,
    text: &'a str,
    font_size: f32,
    max_w: f32,
    is_bold: bool,
) -> (&'a str, Option<&'a str>) {
    let (full_w, _) = text_renderer.measure_text(text, font_size, is_bold);
    if full_w <= max_w {
        return (text, None);
    }

    // 1. Try punctuation-aware split (commas, semicolons, dashes)
    let punc_patterns = [", ", "; ", " - ", "—", "... ", ": "];
    for punc in punc_patterns {
        let mut search_from = 0;
        while let Some(rel_pos) = text[search_from..].find(punc) {
            let pos = search_from + rel_pos;
            let split_at = pos + punc.len();
            let ratio = split_at as f32 / text.len().max(1) as f32;
            if (0.25..=0.75).contains(&ratio) {
                let l1 = text[..split_at].trim();
                let l2 = text[split_at..].trim();
                let (w1, _) = text_renderer.measure_text(l1, font_size, is_bold);
                let (w2, _) = text_renderer.measure_text(l2, font_size, is_bold);
                if w1 <= max_w && w2 <= max_w {
                    return (l1, Some(l2));
                }
            }
            search_from = pos + punc.len();
            if search_from >= text.len() {
                break;
            }
        }
    }

    // 2. Greedy word-by-word wrapping
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= 1 {
        return (text, None);
    }

    let mut best_split = 0;
    let mut byte_pos = 0;

    for word in &words {
        let cand_end = byte_pos + word.len();
        if cand_end <= text.len() {
            let cand = text[..cand_end].trim();
            let (cw, _) = text_renderer.measure_text(cand, font_size, is_bold);
            if cw <= max_w {
                best_split = cand_end;
            } else {
                break;
            }
        }
        byte_pos += word.len() + 1;
    }

    if best_split > 0 && best_split < text.len() {
        let l1 = text[..best_split].trim();
        let l2 = text[best_split..].trim();
        if !l2.is_empty() {
            return (l1, Some(l2));
        }
    }

    // Fallback: split at nearest whitespace before half
    if let Some(space_idx) = text[..text.len() / 2].rfind(' ') {
        let l1 = text[..space_idx].trim();
        let l2 = text[space_idx..].trim();
        return (l1, Some(l2));
    }

    (text, None)
}

    /// Mode 4: Lyrics - Compact player on left, live synced lyrics on right with clean fallback
    fn render_lyrics(
        &mut self,
        buf: &mut PixelBuffer,
        text_renderer: &TextRenderer,
        x: f32,
        w: f32,
        h: f32,
        state: &IslandState,
        lyrics: &SyncedLyrics,
        opacity: f32,
        cursor_pos: Option<(f32, f32)>,
    ) {
        let _ = h;
        let left_w = 260.0;
        let art_size = 42.0;
        let art_x = x + 16.0;
        let art_y = 18.0;

        // Left: Album Art Tile
        render_album_art_tile(
            buf,
            art_x,
            art_y,
            art_size,
            state.media_info.artwork_pixels.as_deref(),
            state.media_info.artwork_w,
            state.media_info.artwork_h,
            opacity,
        );

        // Title & Artist with robust fallback (no hardcoded songs!)
        let text_x = art_x + art_size + 12.0;
        let text_max_w = left_w - (text_x - x) - 8.0;

        let has_track = state.media_info.has_media && !state.media_info.title.is_empty();
        let (title_buf, artist_buf) = if has_track {
            format_media_display(&state.media_info.title, &state.media_info.artist)
        } else {
            ("No Track Playing".to_string(), "Music Player".to_string())
        };
        let title = &title_buf;
        let artist = &artist_buf;

        text_renderer.draw_text_truncated(
            buf,
            title,
            text_x,
            art_y + 16.0,
            13.0,
            text_max_w,
            Color::rgba(255, 255, 255, 0.98 * opacity),
            true,
        );
        text_renderer.draw_text_truncated(
            buf,
            artist,
            text_x,
            art_y + 30.0,
            11.0,
            text_max_w,
            Color::rgba(156, 163, 175, 0.85 * opacity),
            false,
        );

        // Progress bar with hover thickness and interactive dragging knob
        let bar_y = 78.0;
        let p_x = x + 50.0;
        let p_w = 110.0;

        let bar_rect = Rect {
            x: p_x - 4.0,
            y: bar_y - 8.0,
            w: p_w + 8.0,
            h: 16.0,
        };
        let is_bar_hovered = cursor_pos.map(|(mx, my)| bar_rect.contains(mx, my)).unwrap_or(false);
        let is_bar_active = is_bar_hovered || state.is_dragging_slider;

        let effective_secs = if state.is_dragging_slider {
            state.drag_seek_secs.unwrap_or(state.media_info.position_secs)
        } else {
            state.media_info.position_secs
        };

        let cur_time_str = format_seconds(effective_secs);
        let rem_time_str = if state.media_info.duration_secs > effective_secs {
            format!("-{}", format_seconds(state.media_info.duration_secs - effective_secs))
        } else {
            "-00:00".to_string()
        };

        text_renderer.draw_text(buf, &cur_time_str, x + 14.0, bar_y + 4.0, 9.5, Color::rgba(156, 163, 175, 0.8 * opacity), false);

        let p_val = if state.media_info.duration_secs > 0 {
            (effective_secs as f32 / state.media_info.duration_secs as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let bar_h = if is_bar_active { 5.5 } else { 3.5 };
        let bar_bg = if is_bar_active {
            Color::rgba(255, 255, 255, 0.28 * opacity)
        } else {
            Color::rgba(255, 255, 255, 0.16 * opacity)
        };
        let bar_fill = if is_bar_active {
            Color::rgba(255, 255, 255, 1.0 * opacity)
        } else {
            Color::rgba(255, 255, 255, 0.95 * opacity)
        };

        draw_progress_bar(buf, p_x, bar_y - (bar_h * 0.5), p_w, bar_h, p_val, bar_bg, bar_fill);

        if (is_bar_active || is_bar_hovered) && state.media_info.duration_secs > 0 {
            let thumb_x = p_x + p_val * p_w;
            let thumb_y = bar_y;
            draw_circle(buf, thumb_x, thumb_y, 6.5, Color::rgba(255, 255, 255, 0.38 * opacity));
            draw_circle(buf, thumb_x, thumb_y, 4.5, Color::rgba(255, 255, 255, 1.0 * opacity));
        }

        if let Some((mx, _)) = cursor_pos {
            if bar_rect.contains(mx, bar_y) && state.media_info.duration_secs > 0 {
                let frac = ((mx - p_x) / p_w).clamp(0.0, 1.0);
                let seek_secs = (frac * state.media_info.duration_secs as f32) as u32;
                self.click_targets.push((bar_rect, ClickAction::SeekMedia(seek_secs)));
            }
        }

        text_renderer.draw_text(buf, &rem_time_str, p_x + p_w + 6.0, bar_y + 4.0, 9.5, Color::rgba(156, 163, 175, 0.8 * opacity), false);

        // Controls Row
        let ctrl_y = 112.0;

        // Lyrics Button
        let lyr_rect = Rect { x: x + 12.0, y: ctrl_y - 12.0, w: 24.0, h: 24.0 };
        draw_lyrics_button(buf, x + 24.0, ctrl_y, 18.0, true, false, opacity);
        self.click_targets.push((lyr_rect, ClickAction::ToggleLyrics));

        // Playback Controls Cluster centered around pb_center = x + 150.0
        let pb_center = x + 150.0;

        // Seek Backward (-5s)
        let seek_back_rect = Rect { x: pb_center - 68.0, y: ctrl_y - 12.0, w: 24.0, h: 24.0 };
        let seek_back_hover = cursor_pos.map(|(px, py)| seek_back_rect.contains(px, py)).unwrap_or(false);
        draw_seek_backward_button(buf, text_renderer, pb_center - 56.0, ctrl_y, 18.0, seek_back_hover, opacity);
        self.click_targets.push((seek_back_rect, ClickAction::SeekRelative(-5)));

        // Prev Track
        let prev_rect = Rect { x: pb_center - 40.0, y: ctrl_y - 12.0, w: 24.0, h: 24.0 };
        let prev_hover = cursor_pos.map(|(px, py)| prev_rect.contains(px, py)).unwrap_or(false);
        if prev_hover {
            draw_circle(buf, pb_center - 28.0, ctrl_y, 11.0, Color::rgba(255, 255, 255, 0.14));
        }
        draw_prev_icon(buf, pb_center - 34.0, ctrl_y - 5.0, 11.0, Color::rgba(255, 255, 255, 0.9 * opacity));
        self.click_targets.push((prev_rect, ClickAction::SkipPrev));

        // Play/Pause
        let play_rect = Rect { x: pb_center - 13.0, y: ctrl_y - 13.0, w: 26.0, h: 26.0 };
        let play_hover = cursor_pos.map(|(px, py)| play_rect.contains(px, py)).unwrap_or(false);
        let play_bg = if play_hover {
            Color::rgba(255, 255, 255, 0.28 * opacity)
        } else {
            Color::rgba(255, 255, 255, 0.16 * opacity)
        };
        draw_circle(buf, pb_center, ctrl_y, 13.0, play_bg);
        if state.media_info.is_playing {
            draw_pause_icon(buf, pb_center - 5.0, ctrl_y - 5.0, 11.0, Color::rgba(255, 255, 255, 0.98 * opacity));
        } else {
            draw_play_icon(buf, pb_center - 4.5, ctrl_y - 5.0, 11.0, Color::rgba(255, 255, 255, 0.98 * opacity));
        }
        self.click_targets.push((play_rect, ClickAction::TogglePlayPause));

        // Next Track
        let next_rect = Rect { x: pb_center + 16.0, y: ctrl_y - 12.0, w: 24.0, h: 24.0 };
        let next_hover = cursor_pos.map(|(px, py)| next_rect.contains(px, py)).unwrap_or(false);
        if next_hover {
            draw_circle(buf, pb_center + 28.0, ctrl_y, 11.0, Color::rgba(255, 255, 255, 0.14));
        }
        draw_next_icon(buf, pb_center + 23.0, ctrl_y - 5.0, 11.0, Color::rgba(255, 255, 255, 0.9 * opacity));
        self.click_targets.push((next_rect, ClickAction::SkipNext));

        // Seek Forward (+5s)
        let seek_fwd_rect = Rect { x: pb_center + 44.0, y: ctrl_y - 12.0, w: 24.0, h: 24.0 };
        let seek_fwd_hover = cursor_pos.map(|(px, py)| seek_fwd_rect.contains(px, py)).unwrap_or(false);
        draw_seek_forward_button(buf, text_renderer, pb_center + 56.0, ctrl_y, 18.0, seek_fwd_hover, opacity);
        self.click_targets.push((seek_fwd_rect, ClickAction::SeekRelative(5)));

        // Right Column: Synced Lyrics Display (clean borderless transition, zero divider line)
        let lyrics_x = x + left_w + 20.0;
        let lyrics_max_w = w - left_w - 36.0;

        if !has_track || lyrics.lines.is_empty() {
            // Clean fallback state when no track is playing or lyrics not found
            text_renderer.draw_text_truncated(
                buf,
                "No lyrics available",
                lyrics_x,
                70.0,
                14.0,
                lyrics_max_w,
                Color::rgba(255, 255, 255, 0.95 * opacity),
                true,
            );
            text_renderer.draw_text_truncated(
                buf,
                "Play music on Spotify or your browser to view lyrics",
                lyrics_x,
                94.0,
                11.0,
                lyrics_max_w,
                Color::rgba(148, 163, 184, 0.7 * opacity),
                false,
            );
            return;
        }

        let playback_time = state.get_current_playback_time();
        let active_idx = lyrics.get_active_index(playback_time);

        let layouts = Self::compute_lyric_layouts(text_renderer, lyrics, lyrics_max_w);
        let scroll_y = state.lyrics_scroll_y;
        let line_step = 18.0f32;

        for (i, item) in layouts.iter().enumerate() {
            let line_y = 74.0 + (item.y_offset - scroll_y);

            // Safe vertical bounds: culls lines crossing top or bottom border
            if line_y < 10.0 || line_y > 122.0 {
                continue;
            }

            let dist = if item.is_wrapped {
                (line_y + 9.0 - 74.0).abs()
            } else {
                (line_y - 74.0).abs()
            };
            let is_active = i == active_idx;

            // Opacity fades smoothly to zero near top and bottom borders (Apple Music style)
            let fade = if dist > 44.0 {
                0.0
            } else {
                (1.0 - (dist / 44.0).powi(2)).clamp(0.0, 1.0)
            };

            if fade <= 0.01 && !is_active {
                continue;
            }

            let (font_size, color) = if is_active {
                (13.5, Color::rgba(255, 255, 255, 0.98 * opacity))
            } else {
                (11.0, Color::rgba(148, 163, 184, (0.75 * fade) * opacity))
            };

            match item.line2 {
                None => {
                    let line_rect = Rect {
                        x: lyrics_x - 8.0,
                        y: line_y - 12.0,
                        w: lyrics_max_w + 16.0,
                        h: 24.0,
                    };
                    self.click_targets.push((line_rect, ClickAction::SeekMedia(item.timestamp as u32)));

                    text_renderer.draw_text_truncated(
                        buf,
                        item.line1,
                        lyrics_x,
                        line_y + 4.0,
                        font_size,
                        lyrics_max_w,
                        color,
                        is_active,
                    );
                }
                Some(l2) => {
                    let line_rect = Rect {
                        x: lyrics_x - 8.0,
                        y: line_y - 12.0,
                        w: lyrics_max_w + 16.0,
                        h: 38.0,
                    };
                    self.click_targets.push((line_rect, ClickAction::SeekMedia(item.timestamp as u32)));

                    text_renderer.draw_text_truncated(
                        buf,
                        item.line1,
                        lyrics_x,
                        line_y + 4.0,
                        font_size,
                        lyrics_max_w,
                        color,
                        is_active,
                    );
                    if line_y + 4.0 + line_step <= 124.0 {
                        text_renderer.draw_text_truncated(
                            buf,
                            l2,
                            lyrics_x,
                            line_y + 4.0 + line_step,
                            font_size,
                            lyrics_max_w,
                            color,
                            is_active,
                        );
                    }
                }
            }
        }
    }

    /// Mode 5: Shelf - Drag-and-drop file shelf, copy to clipboard, launch file
    fn render_shelf(
        &mut self,
        buf: &mut PixelBuffer,
        text_renderer: &TextRenderer,
        x: f32,
        w: f32,
        h: f32,
        state: &IslandState,
        opacity: f32,
        cursor_pos: Option<(f32, f32)>,
    ) {
        let _ = h;
        // Top Header
        let cy = 18.0;
        text_renderer.draw_text(buf, "Shelf", x + 16.0, cy + 5.0, 13.0, Color::rgba(255, 255, 255, 0.95 * opacity), true);

        // Center Time & Battery
        draw_battery_circle_ring(buf, x + w * 0.5 - 72.0, cy, 6.0, state.battery_percent, state.is_charging, state.is_energy_saver, opacity);
        let header_time = get_formatted_header_time();
        text_renderer.draw_text(buf, &header_time, x + w * 0.5 - 58.0, cy + 4.0, 11.5, Color::rgba(255, 255, 255, 0.95 * opacity), true);

        // Nav Arrows: < to Music, > to ToDo
        let prev_btn_rect = Rect { x: x + 12.0, y: 56.0, w: 24.0, h: 24.0 };
        let prev_hover = cursor_pos.map(|(px, py)| prev_btn_rect.contains(px, py)).unwrap_or(false);
        draw_nav_arrow_button(buf, x + 24.0, 68.0, 11.0, false, prev_hover, opacity);
        self.click_targets.push((prev_btn_rect, ClickAction::PrevPage));

        let next_btn_rect = Rect { x: x + w - 36.0, y: 56.0, w: 24.0, h: 24.0 };
        let next_hover = cursor_pos.map(|(px, py)| next_btn_rect.contains(px, py)).unwrap_or(false);
        draw_nav_arrow_button(buf, x + w - 24.0, 68.0, 11.0, true, next_hover, opacity);
        self.click_targets.push((next_btn_rect, ClickAction::NextPage));

        // Shelf Container Box
        let box_x = x + 44.0;
        let box_y = 36.0;
        let box_w = w - 88.0;
        let box_h = 76.0;

        draw_rounded_rect(buf, box_x, box_y, box_w, box_h, 12.0, Color::rgba(255, 255, 255, 0.05 * opacity));
        draw_rounded_rect_stroke(buf, box_x, box_y, box_w, box_h, 12.0, 1.0, Color::rgba(255, 255, 255, 0.15 * opacity));

        if state.shelf.items.is_empty() {
            let center_box_x = box_x + box_w * 0.5;
            draw_rounded_rect(buf, center_box_x - 10.0, box_y + 12.0, 20.0, 10.0, 2.0, Color::rgba(255, 255, 255, 0.7 * opacity));
            draw_circle(buf, center_box_x, box_y + 10.0, 2.0, Color::rgba(255, 255, 255, 0.9 * opacity));

            text_renderer.draw_text(
                buf,
                "Drop files or copy text here",
                center_box_x - 70.0,
                box_y + 40.0,
                11.5,
                Color::rgba(255, 255, 255, 0.92 * opacity),
                true,
            );
            text_renderer.draw_text(
                buf,
                "Drag out anytime · click to copy & use",
                center_box_x - 88.0,
                box_y + 56.0,
                9.5,
                Color::rgba(148, 163, 184, 0.75 * opacity),
                false,
            );
        } else {
            let item_count = state.shelf.items.len();

            for (i, item) in state.shelf.items.iter().enumerate() {
                let card_rect = self.get_shelf_card_rect(i, item_count, x, w);
                let is_hovered = cursor_pos.map(|(px, py)| card_rect.contains(px, py)).unwrap_or(false);

                let card_bg = if is_hovered {
                    Color::rgba(255, 255, 255, 0.20 * opacity)
                } else {
                    Color::rgba(255, 255, 255, 0.08 * opacity)
                };
                draw_rounded_rect(buf, card_rect.x, card_rect.y, card_rect.w, card_rect.h, 8.0, card_bg);

                let icon_x = (card_rect.x + (card_rect.w - 32.0) * 0.5) as usize;
                let icon_y = (card_rect.y + 6.0) as usize;
                if item.icon_width > 0 && item.icon_height > 0 && !item.icon_pixels.is_empty() {
                    buf.blend_icon(icon_x, icon_y, &item.icon_pixels, item.icon_width, item.icon_height, opacity);
                }

                text_renderer.draw_text_truncated(
                    buf,
                    &item.name,
                    card_rect.x + 4.0,
                    card_rect.y + 48.0,
                    9.0,
                    card_rect.w - 8.0,
                    Color::rgba(255, 255, 255, 0.95 * opacity),
                    true,
                );
                text_renderer.draw_text_truncated(
                    buf,
                    &item.size_formatted,
                    card_rect.x + 4.0,
                    card_rect.y + 58.0,
                    8.0,
                    card_rect.w - 8.0,
                    Color::rgba(148, 163, 184, 0.8 * opacity),
                    false,
                );

                // Clicking card copies to clipboard (ready to Ctrl+V into WhatsApp/Discord/Notepad)!
                self.click_targets.push((card_rect, ClickAction::OpenShelfItem(i)));

                if is_hovered {
                    let remove_rect = Rect {
                        x: card_rect.x + card_rect.w - 14.0,
                        y: card_rect.y + 2.0,
                        w: 12.0,
                        h: 12.0,
                    };
                    draw_close_button(buf, remove_rect.x + 6.0, remove_rect.y + 6.0, 5.5, true);
                    self.click_targets.push((remove_rect, ClickAction::RemoveShelfItem(i)));
                }
            }
        }

        // Bottom Page Indicator Dots (• — • • •)
        let indicator_y = 134.0;
        draw_page_indicators(buf, x + w * 0.5, indicator_y, 1, 5, opacity);
        self.register_page_dot_targets(x + w * 0.5, indicator_y, 5);
    }

    /// Mode 6: To-do - Clean typography spacing, task statistics, scrollable task items
    fn render_todo(
        &mut self,
        buf: &mut PixelBuffer,
        text_renderer: &TextRenderer,
        x: f32,
        w: f32,
        h: f32,
        state: &IslandState,
        opacity: f32,
        cursor_pos: Option<(f32, f32)>,
    ) {
        let _ = h;
        // Top Header
        let cy = 18.0;
        text_renderer.draw_text(buf, "TodoList", x + 16.0, cy + 5.0, 13.0, Color::rgba(34, 197, 94, 0.95 * opacity), true);

        // Center Time & Battery
        draw_battery_circle_ring(buf, x + w * 0.5 - 72.0, cy, 6.0, state.battery_percent, state.is_charging, state.is_energy_saver, opacity);
        let header_time = get_formatted_header_time();
        text_renderer.draw_text(buf, &header_time, x + w * 0.5 - 58.0, cy + 4.0, 11.5, Color::rgba(255, 255, 255, 0.95 * opacity), true);

        // Prev Arrow Button (<) to cycle to Shelf
        let prev_btn_rect = Rect { x: x + 12.0, y: 56.0, w: 24.0, h: 24.0 };
        let prev_hover = cursor_pos.map(|(px, py)| prev_btn_rect.contains(px, py)).unwrap_or(false);
        draw_nav_arrow_button(buf, x + 24.0, 68.0, 11.0, false, prev_hover, opacity);
        self.click_targets.push((prev_btn_rect, ClickAction::PrevPage));

        // Left Column: Statistics "X/Y Task Done Today" with clean vertical spacing
        let left_x = x + 44.0;
        let (done_cnt, total_cnt) = state.todo.counts();
        let score_str = format!("{}/{}", done_cnt, total_cnt);

        // Fixed spacing from image 2: score at y = 54.0, subtitle at y = 74.0
        text_renderer.draw_text(
            buf,
            &score_str,
            left_x,
            54.0,
            20.0,
            Color::rgba(255, 255, 255, 0.98 * opacity),
            true,
        );
        text_renderer.draw_text(
            buf,
            "Task Done Today",
            left_x,
            74.0,
            10.5,
            Color::rgba(148, 163, 184, 0.85 * opacity),
            false,
        );

        // Interactive Task Input Mode or Action Buttons (+ Add Task & ✎ Clear)
        if state.todo.is_input_active {
            let input_rect = Rect { x: left_x, y: 92.0, w: 140.0, h: 26.0 };
            // Dark input pill with green accent outline
            draw_rounded_rect(buf, input_rect.x, input_rect.y, input_rect.w, input_rect.h, 6.0, Color::rgba(18, 22, 28, 0.95 * opacity));
            draw_rounded_rect_stroke(buf, input_rect.x, input_rect.y, input_rect.w, input_rect.h, 6.0, 1.2, Color::rgba(34, 197, 94, 0.85 * opacity));

            let text_val = &state.todo.input_buffer;
            let max_text_w = input_rect.w - 40.0;

            if text_val.is_empty() {
                text_renderer.draw_text(buf, "Type task...", input_rect.x + 8.0, input_rect.y + 17.0, 10.0, Color::rgba(148, 163, 184, 0.7 * opacity), false);
                let cursor_visible = (state.anim_time * 2.5).fract() < 0.5;
                if cursor_visible {
                    let cursor_x = (input_rect.x + 8.0) as usize;
                    let min_y = (input_rect.y + 6.0) as usize;
                    let max_y = (input_rect.y + 19.0) as usize;
                    for py in min_y..=max_y {
                        buf.blend_pixel(cursor_x, py, 34, 197, 94, 0.95 * opacity);
                    }
                }
            } else {
                // Ensure text stays inside input box: calculate visible suffix if longer than box
                let mut visible_text = text_val.as_str();
                while !visible_text.is_empty() && text_renderer.measure_text(visible_text, 10.0, false).0 > max_text_w {
                    let mut chars = visible_text.chars();
                    chars.next();
                    visible_text = chars.as_str();
                }

                text_renderer.draw_text(buf, visible_text, input_rect.x + 8.0, input_rect.y + 17.0, 10.0, Color::rgba(255, 255, 255, 0.98 * opacity), false);

                // Precise blinking cursor positioned using exact font advance width
                let cursor_visible = (state.anim_time * 2.5).fract() < 0.5;
                if cursor_visible {
                    let (vis_w, _) = text_renderer.measure_text(visible_text, 10.0, false);
                    let cursor_x = (input_rect.x + 8.0 + vis_w + 1.0) as usize;
                    let min_y = (input_rect.y + 6.0) as usize;
                    let max_y = (input_rect.y + 19.0) as usize;
                    for py in min_y..=max_y {
                        buf.blend_pixel(cursor_x, py, 34, 197, 94, 0.95 * opacity);
                    }
                }
            }

            // Save button (✓ vector checkmark)
            let ok_rect = Rect { x: input_rect.x + input_rect.w - 34.0, y: input_rect.y + 2.0, w: 16.0, h: 22.0 };
            let ok_hover = cursor_pos.map(|(px, py)| ok_rect.contains(px, py)).unwrap_or(false);
            let ok_col = if ok_hover { Color::rgba(34, 197, 94, 1.0 * opacity) } else { Color::rgba(34, 197, 94, 0.75 * opacity) };
            let ok_cx = ok_rect.x + 8.0;
            let ok_cy = ok_rect.y + 11.0;
            for i in 0..5 {
                let t = i as f32 / 4.0;
                draw_circle(buf, ok_cx - 3.5 + t * 2.0, ok_cy + t * 2.0, 0.9, ok_col);
            }
            for i in 0..7 {
                let t = i as f32 / 6.0;
                draw_circle(buf, ok_cx - 1.5 + t * 5.0, ok_cy + 2.0 - t * 6.0, 0.9, ok_col);
            }
            self.click_targets.push((ok_rect, ClickAction::CommitAddTask));

            // Cancel button (✕ vector cross)
            let cancel_rect = Rect { x: input_rect.x + input_rect.w - 18.0, y: input_rect.y + 2.0, w: 16.0, h: 22.0 };
            let cancel_hover = cursor_pos.map(|(px, py)| cancel_rect.contains(px, py)).unwrap_or(false);
            let cancel_col = if cancel_hover { Color::rgba(239, 68, 68, 1.0 * opacity) } else { Color::rgba(156, 163, 175, 0.75 * opacity) };
            let cancel_cx = cancel_rect.x + 8.0;
            let cancel_cy = cancel_rect.y + 11.0;
            for i in 0..8 {
                let t = (i as f32 / 7.0) * 6.0 - 3.0;
                draw_circle(buf, cancel_cx + t, cancel_cy + t, 0.9, cancel_col);
                draw_circle(buf, cancel_cx + t, cancel_cy - t, 0.9, cancel_col);
            }
            self.click_targets.push((cancel_rect, ClickAction::CancelAddTask));
        } else {
            // Buttons: + Add Task & ✎ Clear positioned cleanly at y = 96.0
            let add_btn_rect = Rect { x: left_x, y: 96.0, w: 72.0, h: 22.0 };
            let add_hover = cursor_pos.map(|(px, py)| add_btn_rect.contains(px, py)).unwrap_or(false);
            let add_bg = if add_hover { Color::rgba(255, 255, 255, 0.25 * opacity) } else { Color::rgba(255, 255, 255, 0.12 * opacity) };
            draw_rounded_rect(buf, add_btn_rect.x, add_btn_rect.y, add_btn_rect.w, add_btn_rect.h, 6.0, add_bg);
            text_renderer.draw_text(buf, "+ Add Task", add_btn_rect.x + 8.0, add_btn_rect.y + 15.0, 9.5, Color::rgba(255, 255, 255, 0.95 * opacity), true);
            self.click_targets.push((add_btn_rect, ClickAction::PromptAddTask));

            let clear_btn_rect = Rect { x: left_x + 78.0, y: 96.0, w: 56.0, h: 22.0 };
            let clear_hover = cursor_pos.map(|(px, py)| clear_btn_rect.contains(px, py)).unwrap_or(false);
            let clear_bg = if clear_hover { Color::rgba(255, 255, 255, 0.25 * opacity) } else { Color::rgba(255, 255, 255, 0.12 * opacity) };
            draw_rounded_rect(buf, clear_btn_rect.x, clear_btn_rect.y, clear_btn_rect.w, clear_btn_rect.h, 6.0, clear_bg);
            text_renderer.draw_text(buf, "✎ Clear", clear_btn_rect.x + 8.0, clear_btn_rect.y + 15.0, 9.5, Color::rgba(255, 255, 255, 0.95 * opacity), false);
            self.click_targets.push((clear_btn_rect, ClickAction::ClearDoneTodo));
        }

        // Right Column: Scrollable Task List Items
        let right_x = x + 192.0;
        let right_w = w - 212.0;
        let item_h = 24.0;
        let item_gap = 4.0;

        let visible_items = state.todo.items.iter().skip(state.todo.scroll_offset).take(3);

        for (i, item) in visible_items.enumerate() {
            let item_y = 36.0 + (i as f32) * (item_h + item_gap);
            let item_rect = Rect {
                x: right_x,
                y: item_y,
                w: right_w,
                h: item_h,
            };
            let is_hovered = cursor_pos.map(|(px, py)| item_rect.contains(px, py)).unwrap_or(false);

            let item_bg = if item.done {
                Color::rgba(20, 83, 45, 0.35 * opacity)
            } else if is_hovered {
                Color::rgba(255, 255, 255, 0.16 * opacity)
            } else {
                Color::rgba(255, 255, 255, 0.08 * opacity)
            };
            draw_rounded_rect(buf, item_rect.x, item_rect.y, item_rect.w, item_rect.h, 7.0, item_bg);

            // Checkbox with proper breathing room and full anti-aliased border
            let check_cx = item_rect.x + 14.0;
            let check_cy = item_rect.y + item_h * 0.5;
            draw_todo_checkbox(buf, check_cx, check_cy, 6.5, item.done, is_hovered, opacity);

            // Task text
            let text_color = if item.done {
                Color::rgba(187, 247, 208, 0.85 * opacity)
            } else {
                Color::rgba(255, 255, 255, 0.95 * opacity)
            };
            text_renderer.draw_text_truncated(
                buf,
                &item.text,
                item_rect.x + 28.0,
                item_rect.y + 16.0,
                10.5,
                item_rect.w - 36.0,
                text_color,
                item.done,
            );

            self.click_targets.push((item_rect, ClickAction::ToggleTodo(item.id)));
        }

        // Scrollbar Indicator (on right edge of task box)
        if state.todo.items.len() > 3 {
            let sb_x = right_x + right_w + 5.0;
            let sb_y = 38.0;
            let sb_h = 76.0;
            let thumb_h = (sb_h * (3.0 / state.todo.items.len() as f32)).max(12.0);
            let thumb_y = sb_y + (state.todo.scroll_offset as f32 / (state.todo.items.len() - 3) as f32) * (sb_h - thumb_h);
            draw_rounded_rect(buf, sb_x, thumb_y, 3.0, thumb_h, 1.5, Color::rgba(255, 255, 255, 0.35 * opacity));
        }

        // Bottom Page Indicator Dots (• • — • •)
        let indicator_y = 136.0;
        draw_page_indicators(buf, x + w * 0.5, indicator_y, 2, 5, opacity);
        self.register_page_dot_targets(x + w * 0.5, indicator_y, 5);
    }

    /// Mode 7: Focus Timer & Stopwatch (Page 3)
    fn render_timer(
        &mut self,
        buf: &mut PixelBuffer,
        text_renderer: &TextRenderer,
        x: f32,
        w: f32,
        _h: f32,
        state: &IslandState,
        opacity: f32,
        cursor_pos: Option<(f32, f32)>,
    ) {
        let center_x = x + w * 0.5;

        // Top Header Row (y: 18.0) - Consistent with Shelf and TodoList
        let cy = 18.0;
        text_renderer.draw_text(buf, "Focus", x + 16.0, cy + 5.0, 13.0, Color::rgba(251, 191, 36, 0.95 * opacity), true);

        // Center Time & Battery
        draw_battery_circle_ring(buf, center_x - 72.0, cy, 6.0, state.battery_percent, state.is_charging, state.is_energy_saver, opacity);
        let header_time = get_formatted_header_time();
        text_renderer.draw_text(buf, &header_time, center_x - 58.0, cy + 4.0, 11.5, Color::rgba(255, 255, 255, 0.95 * opacity), true);

        // Right side: Live sound wave if media is playing
        if state.media_info.is_playing {
            draw_monochrome_sound_wave(buf, x + w - 38.0, cy - 6.0, 22.0, 12.0, true, state.anim_time, opacity);
        }

        // Nav Arrows: < to ToDo, > to Tools
        let prev_btn_rect = Rect { x: x + 10.0, y: 56.0, w: 22.0, h: 24.0 };
        let prev_hover = cursor_pos.map(|(px, py)| prev_btn_rect.contains(px, py)).unwrap_or(false);
        draw_nav_arrow_button(buf, x + 20.0, 68.0, 10.5, false, prev_hover, opacity);
        self.click_targets.push((prev_btn_rect, ClickAction::PrevPage));

        let next_btn_rect = Rect { x: x + w - 32.0, y: 56.0, w: 22.0, h: 24.0 };
        let next_hover = cursor_pos.map(|(px, py)| next_btn_rect.contains(px, py)).unwrap_or(false);
        draw_nav_arrow_button(buf, x + w - 21.0, 68.0, 10.5, true, next_hover, opacity);
        self.click_targets.push((next_btn_rect, ClickAction::NextPage));

        // Mode Selectors Pills (y: 38.0)
        let modes = [
            ("Focus", crate::system::timer::TimerMode::Focus, 52.0),
            ("Break", crate::system::timer::TimerMode::ShortBreak, 50.0),
            ("Stopwatch", crate::system::timer::TimerMode::Stopwatch, 74.0),
        ];
        let mut pill_x = x + 30.0;
        let pill_y = 38.0;
        let pill_h = 22.0;

        for (label, mode, pill_w) in modes {
            let is_selected = state.focus_timer.mode == mode;
            let p_rect = Rect { x: pill_x, y: pill_y, w: pill_w, h: pill_h };
            let p_hover = cursor_pos.map(|(px, py)| p_rect.contains(px, py)).unwrap_or(false);

            let bg = if is_selected {
                Color::rgba(255, 255, 255, 0.24 * opacity)
            } else if p_hover {
                Color::rgba(255, 255, 255, 0.12 * opacity)
            } else {
                Color::rgba(255, 255, 255, 0.05 * opacity)
            };
            draw_rounded_rect(buf, pill_x, pill_y, pill_w, pill_h, 11.0, bg);

            let text_color = if is_selected {
                Color::rgba(255, 255, 255, 0.98 * opacity)
            } else {
                Color::rgba(148, 163, 184, 0.85 * opacity)
            };
            let (tw, _) = text_renderer.measure_text(label, 11.0, is_selected);
            text_renderer.draw_text(buf, label, pill_x + (pill_w - tw) * 0.5, pill_y + 15.0, 11.0, text_color, is_selected);

            self.click_targets.push((p_rect, ClickAction::SetTimerMode(mode)));
            pill_x += pill_w + 6.0;
        }

        // Center Area: Large Time Display (y: 68.0 to 100.0)
        let (time_str, sub_time) = state.focus_timer.formatted_time();
        text_renderer.draw_text(buf, &time_str, x + 32.0, 96.0, 32.0, Color::rgba(255, 255, 255, 0.98 * opacity), true);
        if !sub_time.is_empty() {
            let (tw, _) = text_renderer.measure_text(&time_str, 32.0, true);
            text_renderer.draw_text(buf, &sub_time, x + 34.0 + tw, 96.0, 16.0, Color::rgba(148, 163, 184, 0.90 * opacity), true);
        }

        // Subtitle / Status text
        let status_text = if state.focus_timer.is_running {
            if state.focus_timer.mode == crate::system::timer::TimerMode::Stopwatch {
                "Stopwatch recording elapsed time".to_string()
            } else {
                format!("Session #{} in progress", state.focus_timer.sessions_completed + 1)
            }
        } else {
            if state.focus_timer.mode == crate::system::timer::TimerMode::Stopwatch {
                "Ready to track time".to_string()
            } else {
                format!("{} • Ready to focus", state.focus_timer.mode.display_name())
            }
        };
        text_renderer.draw_text(buf, &status_text, x + 34.0, 110.0, 10.5, Color::rgba(148, 163, 184, 0.85 * opacity), false);

        // Circular Progress Ring Meter on Right (center: x + w - 68.0, y: 88.0, radius: 24.0)
        let ring_cx = x + w - 68.0;
        let ring_cy = 88.0;
        let ring_r = 22.0;
        let prog = state.focus_timer.progress();

        // Background ring
        let r_min_x = (ring_cx - ring_r - 2.0).floor().max(0.0) as usize;
        let r_max_x = (ring_cx + ring_r + 2.0).ceil().min(buf.width as f32 - 1.0) as usize;
        let r_min_y = (ring_cy - ring_r - 2.0).floor().max(0.0) as usize;
        let r_max_y = (ring_cy + ring_r + 2.0).ceil().min(buf.height as f32 - 1.0) as usize;
        for py in r_min_y..=r_max_y {
            for px in r_min_x..=r_max_x {
                let d = ((px as f32 + 0.5 - ring_cx).hypot(py as f32 + 0.5 - ring_cy) - ring_r).abs();
                if d < 1.2 {
                    buf.blend_pixel(px, py, 255, 255, 255, (1.2 - d) * 0.16 * opacity);
                }
            }
        }
        // Active progress arc
        if prog > 0.005 {
            let steps = (prog * 60.0).ceil() as usize;
            let arc_color = Color::rgba(255, 255, 255, 0.95 * opacity);
            for i in 0..=steps {
                let t = i as f32 / 60.0;
                let angle = -0.5 * std::f32::consts::PI + t * 2.0 * std::f32::consts::PI;
                let px = ring_cx + angle.cos() * ring_r;
                let py = ring_cy + angle.sin() * ring_r;
                draw_circle(buf, px, py, 1.4, arc_color);
            }
        }
        // Center icon
        draw_timer_icon(buf, ring_cx, ring_cy, 15.0, Color::rgba(255, 255, 255, 0.92 * opacity));

        // Bottom Controls Row (y: 124.0)
        let ctrl_y = 124.0;

        // [-5m] and [+5m]
        if state.focus_timer.mode != crate::system::timer::TimerMode::Stopwatch {
            let m5_rect = Rect { x: x + 30.0, y: ctrl_y - 10.0, w: 32.0, h: 20.0 };
            let m5_hover = cursor_pos.map(|(px, py)| m5_rect.contains(px, py)).unwrap_or(false);
            let m5_bg = if m5_hover { Color::rgba(255, 255, 255, 0.18 * opacity) } else { Color::rgba(255, 255, 255, 0.08 * opacity) };
            draw_rounded_rect(buf, m5_rect.x, m5_rect.y, m5_rect.w, m5_rect.h, 10.0, m5_bg);
            text_renderer.draw_text(buf, "-5m", m5_rect.x + 5.0, ctrl_y + 4.0, 10.5, Color::rgba(203, 213, 225, 0.9 * opacity), false);
            self.click_targets.push((m5_rect, ClickAction::AdjustTimerMins(-5)));

            let p5_rect = Rect { x: x + 68.0, y: ctrl_y - 10.0, w: 32.0, h: 20.0 };
            let p5_hover = cursor_pos.map(|(px, py)| p5_rect.contains(px, py)).unwrap_or(false);
            let p5_bg = if p5_hover { Color::rgba(255, 255, 255, 0.18 * opacity) } else { Color::rgba(255, 255, 255, 0.08 * opacity) };
            draw_rounded_rect(buf, p5_rect.x, p5_rect.y, p5_rect.w, p5_rect.h, 10.0, p5_bg);
            text_renderer.draw_text(buf, "+5m", p5_rect.x + 5.0, ctrl_y + 4.0, 10.5, Color::rgba(203, 213, 225, 0.9 * opacity), false);
            self.click_targets.push((p5_rect, ClickAction::AdjustTimerMins(5)));
        }

        // Center: Start / Pause Action Button
        let play_w = 78.0;
        let play_h = 22.0;
        let play_x = x + 110.0;
        let play_rect = Rect { x: play_x, y: ctrl_y - 11.0, w: play_w, h: play_h };
        let play_hover = cursor_pos.map(|(px, py)| play_rect.contains(px, py)).unwrap_or(false);
        let play_bg = if state.focus_timer.is_running {
            if play_hover { Color::rgba(255, 255, 255, 0.22 * opacity) } else { Color::rgba(255, 255, 255, 0.14 * opacity) }
        } else {
            if play_hover { Color::rgba(255, 255, 255, 0.35 * opacity) } else { Color::rgba(255, 255, 255, 0.24 * opacity) }
        };
        draw_rounded_rect(buf, play_rect.x, play_rect.y, play_w, play_h, 11.0, play_bg);
        let play_text = if state.focus_timer.is_running { "Pause" } else { "Start" };
        let (pt_w, _) = text_renderer.measure_text(play_text, 11.5, true);
        text_renderer.draw_text(buf, play_text, play_x + (play_w - pt_w) * 0.5, ctrl_y + 4.0, 11.5, Color::rgba(255, 255, 255, 0.98 * opacity), true);
        self.click_targets.push((play_rect, ClickAction::ToggleTimer));

        // Reset Button
        let rst_w = 54.0;
        let rst_x = play_x + play_w + 8.0;
        let rst_rect = Rect { x: rst_x, y: ctrl_y - 11.0, w: rst_w, h: play_h };
        let rst_hover = cursor_pos.map(|(px, py)| rst_rect.contains(px, py)).unwrap_or(false);
        let rst_bg = if rst_hover { Color::rgba(255, 255, 255, 0.18 * opacity) } else { Color::rgba(255, 255, 255, 0.08 * opacity) };
        draw_rounded_rect(buf, rst_rect.x, rst_rect.y, rst_w, play_h, 11.0, rst_bg);
        text_renderer.draw_text(buf, "Reset", rst_x + 12.0, ctrl_y + 4.0, 11.0, Color::rgba(203, 213, 225, 0.90 * opacity), false);
        self.click_targets.push((rst_rect, ClickAction::ResetTimer));

        // Bottom Page Indicator Dots (• • • — •)
        let indicator_y = 138.0;
        draw_page_indicators(buf, center_x, indicator_y, 3, 5, opacity);
        self.register_page_dot_targets(center_x, indicator_y, 5);
    }

    /// Mode 8: Tools & Utilities (Page 4 / 5th Page) - Apple Control Center cards for Eyedropper, Screen Snip, and Mic Mute
    fn render_tools(
        &mut self,
        buf: &mut PixelBuffer,
        text_renderer: &TextRenderer,
        x: f32,
        w: f32,
        _h: f32,
        state: &IslandState,
        opacity: f32,
        cursor_pos: Option<(f32, f32)>,
    ) {
        let center_x = x + w * 0.5;

        // Top Header Row (y: 18.0) - Consistent with Music, Shelf, Todo, and Focus Timer
        let cy = 18.0;
        text_renderer.draw_text(
            buf,
            "Tools",
            x + 16.0,
            cy + 5.0,
            13.0,
            Color::rgba(255, 255, 255, 0.95 * opacity),
            true,
        );

        // Center Time & Battery
        draw_battery_circle_ring(
            buf,
            center_x - 72.0,
            cy,
            6.0,
            state.battery_percent,
            state.is_charging,
            state.is_energy_saver,
            opacity,
        );
        let header_time = get_formatted_header_time();
        text_renderer.draw_text(
            buf,
            &header_time,
            center_x - 58.0,
            cy + 4.0,
            11.5,
            Color::rgba(255, 255, 255, 0.95 * opacity),
            true,
        );

        // Right side: Live sound wave if media is playing
        if state.media_info.is_playing {
            draw_monochrome_sound_wave(
                buf,
                x + w - 38.0,
                cy - 6.0,
                22.0,
                12.0,
                true,
                state.anim_time,
                opacity,
            );
        }

        // Nav Arrow (<) back to Focus Timer
        let prev_btn_rect = Rect { x: x + 10.0, y: 56.0, w: 22.0, h: 24.0 };
        let prev_hover = cursor_pos.map(|(px, py)| prev_btn_rect.contains(px, py)).unwrap_or(false);
        draw_nav_arrow_button(buf, x + 21.0, 68.0, 10.5, false, prev_hover, opacity);
        self.click_targets.push((prev_btn_rect, ClickAction::PrevPage));

        // 3 Apple Control Center Utility Cards
        // Layout: 3 cards centered horizontally
        let card_w = 110.0;
        let card_h = 74.0;
        let gap = 10.0;
        let start_x = x + 38.0;
        let card_y = 38.0;

        // Card 1: Eyedropper (Color Picker)
        {
            let c_x = start_x;
            let c_rect = Rect { x: c_x, y: card_y, w: card_w, h: card_h };
            let is_hover = cursor_pos.map(|(px, py)| c_rect.contains(px, py)).unwrap_or(false);

            let bg = if is_hover {
                Color::rgba(255, 255, 255, 0.12 * opacity)
            } else {
                Color::rgba(255, 255, 255, 0.055 * opacity)
            };
            let stroke = if is_hover {
                Color::rgba(255, 255, 255, 0.22 * opacity)
            } else {
                Color::rgba(255, 255, 255, 0.09 * opacity)
            };
            draw_rounded_rect(buf, c_x, card_y, card_w, card_h, 12.0, bg);
            draw_rounded_rect_stroke(buf, c_x, card_y, card_w, card_h, 12.0, 1.0, stroke);

            // Icon Badge
            let badge_cx = c_x + 22.0;
            let badge_cy = card_y + 22.0;
            let badge_bg = if is_hover {
                Color::rgba(139, 92, 246, 0.35 * opacity)
            } else {
                Color::rgba(139, 92, 246, 0.22 * opacity)
            };
            draw_circle(buf, badge_cx, badge_cy, 12.0, badge_bg);
            draw_eyedropper_icon(buf, badge_cx, badge_cy, 13.0, Color::rgba(255, 255, 255, 0.95 * opacity));

            // Labels
            text_renderer.draw_text(
                buf,
                "Eyedropper",
                c_x + 12.0,
                card_y + 51.0,
                11.0,
                Color::rgba(255, 255, 255, 0.98 * opacity),
                true,
            );
            text_renderer.draw_text(
                buf,
                "Pick HEX Color",
                c_x + 12.0,
                card_y + 64.0,
                9.0,
                Color::rgba(148, 163, 184, 0.85 * opacity),
                false,
            );

            self.click_targets.push((c_rect, ClickAction::StartColorPicker));
        }

        // Card 2: Screen Snip
        {
            let c_x = start_x + card_w + gap;
            let c_rect = Rect { x: c_x, y: card_y, w: card_w, h: card_h };
            let is_hover = cursor_pos.map(|(px, py)| c_rect.contains(px, py)).unwrap_or(false);

            let bg = if is_hover {
                Color::rgba(255, 255, 255, 0.12 * opacity)
            } else {
                Color::rgba(255, 255, 255, 0.055 * opacity)
            };
            let stroke = if is_hover {
                Color::rgba(255, 255, 255, 0.22 * opacity)
            } else {
                Color::rgba(255, 255, 255, 0.09 * opacity)
            };
            draw_rounded_rect(buf, c_x, card_y, card_w, card_h, 12.0, bg);
            draw_rounded_rect_stroke(buf, c_x, card_y, card_w, card_h, 12.0, 1.0, stroke);

            // Icon Badge
            let badge_cx = c_x + 22.0;
            let badge_cy = card_y + 22.0;
            let badge_bg = if is_hover {
                Color::rgba(56, 189, 248, 0.35 * opacity)
            } else {
                Color::rgba(56, 189, 248, 0.22 * opacity)
            };
            draw_circle(buf, badge_cx, badge_cy, 12.0, badge_bg);
            draw_snip_icon(buf, badge_cx, badge_cy, 12.5, Color::rgba(255, 255, 255, 0.95 * opacity));

            // Labels
            text_renderer.draw_text(
                buf,
                "Screen Snip",
                c_x + 12.0,
                card_y + 51.0,
                11.0,
                Color::rgba(255, 255, 255, 0.98 * opacity),
                true,
            );
            text_renderer.draw_text(
                buf,
                "Capture Area",
                c_x + 12.0,
                card_y + 64.0,
                9.0,
                Color::rgba(148, 163, 184, 0.85 * opacity),
                false,
            );

            self.click_targets.push((c_rect, ClickAction::TriggerSnip));
        }

        // Card 3: Mic Mute
        {
            let c_x = start_x + (card_w + gap) * 2.0;
            let c_rect = Rect { x: c_x, y: card_y, w: card_w, h: card_h };
            let is_hover = cursor_pos.map(|(px, py)| c_rect.contains(px, py)).unwrap_or(false);

            let bg = if is_hover {
                Color::rgba(255, 255, 255, 0.12 * opacity)
            } else {
                Color::rgba(255, 255, 255, 0.055 * opacity)
            };
            let stroke = if is_hover {
                Color::rgba(255, 255, 255, 0.22 * opacity)
            } else {
                Color::rgba(255, 255, 255, 0.09 * opacity)
            };
            draw_rounded_rect(buf, c_x, card_y, card_w, card_h, 12.0, bg);
            draw_rounded_rect_stroke(buf, c_x, card_y, card_w, card_h, 12.0, 1.0, stroke);

            // Icon Badge
            let badge_cx = c_x + 22.0;
            let badge_cy = card_y + 22.0;
            let is_muted = state.privacy.mic_muted;
            let badge_bg = if is_muted {
                if is_hover { Color::rgba(239, 68, 68, 0.40 * opacity) } else { Color::rgba(239, 68, 68, 0.25 * opacity) }
            } else {
                if is_hover { Color::rgba(34, 197, 94, 0.35 * opacity) } else { Color::rgba(34, 197, 94, 0.20 * opacity) }
            };
            draw_circle(buf, badge_cx, badge_cy, 12.0, badge_bg);
            draw_microphone_icon(buf, badge_cx, badge_cy, 12.5, is_muted, Color::rgba(255, 255, 255, 0.95 * opacity));

            // Labels
            text_renderer.draw_text(
                buf,
                "Microphone",
                c_x + 12.0,
                card_y + 51.0,
                11.0,
                Color::rgba(255, 255, 255, 0.98 * opacity),
                true,
            );
            let (sub_text, sub_color) = if is_muted {
                ("Muted", Color::rgba(248, 113, 113, 0.95 * opacity))
            } else {
                ("Active / Live", Color::rgba(74, 222, 128, 0.95 * opacity))
            };
            text_renderer.draw_text(
                buf,
                sub_text,
                c_x + 12.0,
                card_y + 64.0,
                9.0,
                sub_color,
                false,
            );

            self.click_targets.push((c_rect, ClickAction::ToggleMicMute));
        }

        // Bottom Page Indicator Dots (• • • • —)
        let indicator_y = 134.0;
        draw_page_indicators(buf, center_x, indicator_y, 4, 5, opacity);
        self.register_page_dot_targets(center_x, indicator_y, 5);
    }
    fn render_charging(
        &self,
        buf: &mut PixelBuffer,
        text_renderer: &TextRenderer,
        x: f32,
        w: f32,
        h: f32,
        percent: u32,
        is_charging: bool,
        opacity: f32,
    ) {
        let cy = h * 0.5;

        let bat_cx = x + 24.0;
        draw_battery_circle_ring(buf, bat_cx, cy, 10.0, percent, is_charging, false, opacity);

        let title = if is_charging { "Charging" } else { "Battery" };
        let subtitle = if is_charging { "Charger connected" } else { "Charger disconnected" };

        text_renderer.draw_text(
            buf,
            title,
            x + 46.0,
            cy - 1.0,
            13.5,
            Color::rgba(255, 255, 255, 0.98 * opacity),
            true,
        );
        text_renderer.draw_text(
            buf,
            subtitle,
            x + 46.0,
            cy + 13.0,
            10.5,
            Color::rgba(148, 163, 184, 0.85 * opacity),
            false,
        );

        let pct_str = format!("{}%", percent);
        text_renderer.draw_text(
            buf,
            &pct_str,
            x + w - 58.0,
            cy + 7.0,
            17.0,
            Color::rgba(255, 255, 255, 0.98 * opacity),
            true,
        );
    }

    /// Notification 5: Display Brightness
    fn render_brightness(
        &self,
        buf: &mut PixelBuffer,
        text_renderer: &TextRenderer,
        x: f32,
        w: f32,
        h: f32,
        level: u32,
        opacity: f32,
    ) {
        let cy = h * 0.5;

        // Left Sun Icon
        draw_sun_icon(buf, x + 24.0, cy, 8.5, Color::rgba(255, 255, 255, 0.95 * opacity));

        // Center: Progress Bar
        let bar_x = x + 44.0;
        let bar_w = w - 100.0;
        let progress = (level as f32 / 100.0).clamp(0.0, 1.0);
        draw_progress_bar(
            buf,
            bar_x,
            cy - 3.5,
            bar_w,
            7.0,
            progress,
            Color::rgba(255, 255, 255, 0.16 * opacity),
            Color::rgba(255, 255, 255, 0.95 * opacity), // Apple monochrome white
        );

        // Right: Percentage
        let pct_str = format!("{}%", level);
        text_renderer.draw_text(
            buf,
            &pct_str,
            bar_x + bar_w + 10.0,
            cy + 5.0,
            12.0,
            Color::rgba(255, 255, 255, 0.95 * opacity),
            true,
        );
    }

    /// Notification 6: Windows Toast Notification Banner
    fn render_banner(
        &self,
        buf: &mut PixelBuffer,
        text_renderer: &TextRenderer,
        x: f32,
        w: f32,
        h: f32,
        app_name: &str,
        title: &str,
        message: &str,
        opacity: f32,
    ) {
        let cy = h * 0.5;

        // Left Bell Icon in pill badge
        draw_rounded_rect(buf, x + 14.0, cy - 14.0, 28.0, 28.0, 8.0, Color::rgba(255, 255, 255, 0.12 * opacity));
        draw_bell_icon(buf, x + 28.0, cy, 14.0, Color::rgba(255, 255, 255, 0.95 * opacity));

        // App Tag + Title
        let content_x = x + 50.0;
        let max_content_w = w - 62.0;

        // Top line: App Name pill / label + Bold Title
        let app_label = if app_name.is_empty() { "Notification" } else { app_name };
        text_renderer.draw_text_truncated(
            buf,
            app_label,
            content_x,
            cy - 3.0,
            10.5,
            75.0,
            Color::rgba(56, 189, 248, 0.98 * opacity), // Sky blue app tag
            true,
        );

        let title_x = content_x + (app_label.len() as f32 * 6.5).min(75.0) + 8.0;
        let title_max_w = (max_content_w - (title_x - content_x)).max(40.0);
        text_renderer.draw_text_truncated(
            buf,
            title,
            title_x,
            cy - 2.0,
            12.0,
            title_max_w,
            Color::rgba(255, 255, 255, 0.98 * opacity),
            true,
        );

        // Bottom line: Message preview
        text_renderer.draw_text_truncated(
            buf,
            message,
            content_x,
            cy + 13.0,
            10.5,
            max_content_w,
            Color::rgba(203, 213, 225, 0.85 * opacity),
            false,
        );
    }

    /// Notification 2: Headphones Connected / Disconnected
    fn render_headphones(
        &self,
        buf: &mut PixelBuffer,
        text_renderer: &TextRenderer,
        x: f32,
        w: f32,
        h: f32,
        name: &str,
        battery: u32,
        connected: bool,
        opacity: f32,
    ) {
        let cy = h * 0.5;

        let bt_cx = x + 24.0;
        let outer_circle_color = if connected {
            Color::rgba(30, 41, 59, 0.8 * opacity)
        } else {
            Color::rgba(45, 25, 25, 0.85 * opacity)
        };
        let dot_color = if connected {
            Color::rgba(56, 189, 248, 0.95 * opacity) // Sky blue for connected
        } else {
            Color::rgba(248, 113, 113, 0.95 * opacity) // Coral red for disconnected
        };

        super::super::render::draw_circle(buf, bt_cx, cy, 10.0, outer_circle_color);
        super::super::render::draw_circle(buf, bt_cx, cy, 3.5, dot_color);

        let disp_name = if name.is_empty() {
            "Headphones"
        } else {
            // Strip technical driver suffixes like " (Realtek High Definition Audio)" if redundant
            if name.starts_with("Headphones (") {
                "Headphones"
            } else if name.starts_with("Speakers/Headphones (") {
                "Wired Headphones"
            } else {
                name
            }
        };

        let sub_str = if connected {
            if battery > 0 {
                format!("Connected · {}%", battery)
            } else {
                "Connected".to_string()
            }
        } else {
            "Disconnected".to_string()
        };

        let sub_color = if connected {
            Color::rgba(148, 163, 184, 0.85 * opacity)
        } else {
            Color::rgba(248, 113, 113, 0.90 * opacity)
        };

        text_renderer.draw_text(
            buf,
            disp_name,
            x + 46.0,
            cy - 1.0,
            13.0,
            Color::rgba(255, 255, 255, 0.98 * opacity),
            true,
        );
        text_renderer.draw_text(
            buf,
            &sub_str,
            x + 46.0,
            cy + 13.0,
            10.5,
            sub_color,
            false,
        );

        draw_headphones_badge(buf, x + w - 28.0, cy, 14.0, opacity);
    }

    /// Notification 3: Volume
    fn render_volume(
        &self,
        buf: &mut PixelBuffer,
        text_renderer: &TextRenderer,
        x: f32,
        w: f32,
        h: f32,
        level: u32,
        muted: bool,
        opacity: f32,
    ) {
        let cy = h * 0.5;
        let spk_color = if muted {
            Color::rgba(239, 68, 68, 0.95 * opacity)
        } else {
            Color::rgba(255, 255, 255, 0.95 * opacity)
        };
        draw_speaker_icon(buf, x + 16.0, cy - 8.0, 16.0, muted, spk_color);

        let bar_x = x + 44.0;
        let bar_w = w - 100.0;
        let prog = if muted { 0.0 } else { level as f32 / 100.0 };
        draw_progress_bar(
            buf,
            bar_x,
            cy - 3.5,
            bar_w,
            7.0,
            prog,
            Color::rgba(255, 255, 255, 0.16 * opacity),
            Color::rgba(255, 255, 255, 0.95 * opacity),
        );

        let text = if muted { "Mute".to_string() } else { format!("{}%", level) };
        text_renderer.draw_text(buf, &text, bar_x + bar_w + 10.0, cy + 5.0, 12.0, Color::rgba(255, 255, 255, 0.95 * opacity), true);
    }

    /// Notification 4: Caps Lock
    fn render_caps_lock(
        &self,
        buf: &mut PixelBuffer,
        text_renderer: &TextRenderer,
        x: f32,
        _w: f32,
        h: f32,
        enabled: bool,
        opacity: f32,
    ) {
        let cy = h * 0.5;
        let badge_bg = if enabled { Color::rgba(34, 197, 94, 0.3 * opacity) } else { Color::rgba(255, 255, 255, 0.12 * opacity) };
        draw_rounded_rect(buf, x + 16.0, cy - 11.0, 22.0, 22.0, 6.0, badge_bg);
        text_renderer.draw_text(buf, "A", x + 23.0, cy + 6.0, 13.0, Color::rgba(255, 255, 255, 0.98 * opacity), true);

        let status = if enabled { "Caps Lock ON" } else { "Caps Lock OFF" };
        let color = if enabled { Color::rgba(34, 197, 94, 0.98 * opacity) } else { Color::rgba(255, 255, 255, 0.85 * opacity) };
        text_renderer.draw_text(buf, status, x + 48.0, cy + 5.0, 12.5, color, true);
    }

    /// Notification 5: Color Picked Eyedropper Toast
    fn render_color_picked(
        &self,
        buf: &mut PixelBuffer,
        text_renderer: &TextRenderer,
        x: f32,
        w: f32,
        h: f32,
        hex: &str,
        rgb: &str,
        opacity: f32,
    ) {
        let cy = h * 0.5;
        let color = if hex.starts_with('#') && hex.len() == 7 {
            let r = u8::from_str_radix(&hex[1..3], 16).unwrap_or(255);
            let g = u8::from_str_radix(&hex[3..5], 16).unwrap_or(255);
            let b = u8::from_str_radix(&hex[5..7], 16).unwrap_or(255);
            Color::rgba(r, g, b, 0.98 * opacity)
        } else {
            Color::rgba(255, 255, 255, 0.98 * opacity)
        };

        // Left Color Swatch Circle with border
        draw_circle(buf, x + 24.0, cy, 9.0, color);
        draw_circle(buf, x + 24.0, cy, 10.0, Color::rgba(255, 255, 255, 0.4 * opacity));

        // Center: Hex code
        text_renderer.draw_text(
            buf,
            hex,
            x + 44.0,
            cy - 1.0,
            13.5,
            Color::rgba(255, 255, 255, 0.98 * opacity),
            true,
        );

        // Subtitle: Copied to clipboard
        text_renderer.draw_text(
            buf,
            "Copied to clipboard",
            x + 44.0,
            cy + 13.0,
            10.0,
            Color::rgba(148, 163, 184, 0.90 * opacity),
            false,
        );

        // Right: RGB values
        let (rgb_w, _) = text_renderer.measure_text(rgb, 10.5, false);
        text_renderer.draw_text(
            buf,
            rgb,
            x + w - 16.0 - rgb_w,
            cy + 4.5,
            10.5,
            Color::rgba(203, 213, 225, 0.85 * opacity),
            false,
        );
    }

    /// Notification 6: Privacy Alert (Camera / Mic In-Use)
    fn render_privacy_alert(
        &mut self,
        buf: &mut PixelBuffer,
        text_renderer: &TextRenderer,
        x: f32,
        w: f32,
        h: f32,
        is_camera: bool,
        app_name: &str,
        opacity: f32,
    ) {
        let cy = h * 0.5;
        if is_camera {
            draw_camera_icon(buf, x + 24.0, cy, 18.0, Color::rgba(34, 197, 94, 0.98 * opacity));
            text_renderer.draw_text(buf, "Camera in use", x + 44.0, cy - 1.0, 12.5, Color::rgba(255, 255, 255, 0.98 * opacity), true);
            text_renderer.draw_text(buf, app_name, x + 44.0, cy + 13.0, 10.5, Color::rgba(148, 163, 184, 0.90 * opacity), false);
        } else {
            draw_microphone_icon(buf, x + 24.0, cy, 18.0, false, Color::rgba(249, 115, 22, 0.98 * opacity));
            text_renderer.draw_text(buf, "Microphone in use", x + 44.0, cy - 1.0, 12.5, Color::rgba(255, 255, 255, 0.98 * opacity), true);
            text_renderer.draw_text(buf, app_name, x + 44.0, cy + 13.0, 10.5, Color::rgba(148, 163, 184, 0.90 * opacity), false);

            // Right Quick Mute Pill
            let mute_w = 56.0;
            let mute_h = 24.0;
            let mute_x = x + w - 16.0 - mute_w;
            let mute_rect = Rect { x: mute_x, y: cy - mute_h * 0.5, w: mute_w, h: mute_h };
            draw_rounded_rect(buf, mute_x, cy - mute_h * 0.5, mute_w, mute_h, 12.0, Color::rgba(239, 68, 68, 0.28 * opacity));
            text_renderer.draw_text(buf, "Mute", mute_x + 14.0, cy + 4.5, 11.0, Color::rgba(255, 255, 255, 0.98 * opacity), true);
            self.click_targets.push((mute_rect, ClickAction::ToggleMicMute));
        }
    }

    /// Notification 7: Timer Completed
    fn render_timer_completed(
        &self,
        buf: &mut PixelBuffer,
        text_renderer: &TextRenderer,
        x: f32,
        _w: f32,
        h: f32,
        message: &str,
        opacity: f32,
    ) {
        let cy = h * 0.5;
        draw_timer_icon(buf, x + 24.0, cy, 18.0, Color::rgba(251, 191, 36, 0.98 * opacity));
        text_renderer.draw_text(buf, "Focus Session Complete", x + 44.0, cy - 1.0, 12.5, Color::rgba(255, 255, 255, 0.98 * opacity), true);
        text_renderer.draw_text(buf, message, x + 44.0, cy + 13.0, 10.5, Color::rgba(148, 163, 184, 0.90 * opacity), false);
    }

    fn render_header_bar(
        &self,
        buf: &mut PixelBuffer,
        text_renderer: &TextRenderer,
        x: f32,
        w: f32,
        state: &IslandState,
        opacity: f32,
    ) {
        let cy = 18.0;
        let bat_cx = x + w * 0.5 - 72.0;
        draw_battery_circle_ring(buf, bat_cx, cy, 6.0, state.battery_percent, state.is_charging, state.is_energy_saver, opacity);

        let header_time = get_formatted_header_time();
        text_renderer.draw_text(
            buf,
            &header_time,
            bat_cx + 14.0,
            cy + 4.0,
            11.5,
            Color::rgba(255, 255, 255, 0.95 * opacity),
            true,
        );

        let wave_x = x + w - 38.0;
        draw_monochrome_sound_wave(buf, wave_x, cy - 6.0, 22.0, 12.0, state.media_info.is_playing, state.anim_time, opacity);
    }

    fn register_page_dot_targets(&mut self, cx: f32, cy: f32, total: usize) {
        let dot_spacing = 14.0;
        let start_x = cx - (total as f32 * dot_spacing) * 0.5;
        for i in 0..total {
            let px = start_x + (i as f32) * dot_spacing;
            let rect = Rect { x: px - 8.0, y: cy - 8.0, w: 16.0, h: 16.0 };
            let page = match i {
                0 => ExpandedPage::Music,
                1 => ExpandedPage::Shelf,
                2 => ExpandedPage::ToDo,
                3 => ExpandedPage::Timer,
                _ => ExpandedPage::Tools,
            };
            self.click_targets.push((rect, ClickAction::SelectPage(page)));
        }
    }

    pub fn handle_click(&self, px: f32, py: f32) -> Option<ClickAction> {
        for (rect, action) in self.click_targets.iter().rev() {
            if rect.contains(px, py) {
                return Some(action.clone());
            }
        }
        None
    }
}

fn format_media_display(raw_title: &str, raw_artist: &str) -> (String, String) {
    let mut title = raw_title.trim();
    let artist = raw_artist.trim();

    let mut is_streaming_service = false;
    let suffixes = [
        " - Netflix", " | Netflix", " - Watch on Netflix", " | Watch on Netflix",
        " - Disney+", " | Disney+", " - Prime Video", " | Prime Video",
        " - HBO Max", " | HBO Max", " - Apple TV", " | Apple TV",
    ];
    for suf in &suffixes {
        if let Some(idx) = title.to_lowercase().rfind(&suf.to_lowercase()) {
            title = title[..idx].trim();
            is_streaming_service = true;
        }
    }

    let artist_str = if !artist.is_empty() {
        artist.to_string()
    } else if is_streaming_service {
        "Netflix".to_string()
    } else {
        "Media Player".to_string()
    };

    (title.to_string(), artist_str)
}

fn render_album_art_tile(
    buf: &mut PixelBuffer,
    x: f32,
    y: f32,
    size: f32,
    artwork: Option<&[u32]>,
    art_w: usize,
    art_h: usize,
    opacity: f32,
) {
    let r = 8.0;
    if let Some(pixels) = artwork {
        if art_w > 0 && art_h > 0 && pixels.len() >= art_w * art_h {
            let min_x = x.floor().max(0.0) as usize;
            let max_x = (x + size).ceil().min(buf.width as f32) as usize;
            let min_y = y.floor().max(0.0) as usize;
            let max_y = (y + size).ceil().min(buf.height as f32) as usize;

            for py in min_y..max_y {
                let p_yf = py as f32 + 0.5;
                let v = ((p_yf - y) / size).clamp(0.0, 1.0);
                let src_y = ((v * (art_h - 1) as f32).round() as usize).min(art_h - 1);

                for px in min_x..max_x {
                    let p_xf = px as f32 + 0.5;
                    let u = ((p_xf - x) / size).clamp(0.0, 1.0);
                    let src_x = ((u * (art_w - 1) as f32).round() as usize).min(art_w - 1);

                    let dx = if p_xf < x + r {
                        x + r - p_xf
                    } else if p_xf > x + size - r {
                        p_xf - (x + size - r)
                    } else {
                        0.0
                    };
                    let dy = if p_yf < y + r {
                        y + r - p_yf
                    } else if p_yf > y + size - r {
                        p_yf - (y + size - r)
                    } else {
                        0.0
                    };
                    let dist = (dx * dx + dy * dy).sqrt();
                    let coverage = if dx > 0.0 && dy > 0.0 {
                        (r + 0.5 - dist).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };

                    if coverage > 0.01 {
                        let src_px = pixels[src_y * art_w + src_x];
                        let sa = ((src_px >> 24) & 0xFF) as f32 / 255.0 * opacity * coverage;
                        let sr = ((src_px >> 16) & 0xFF) as u8;
                        let sg = ((src_px >> 8) & 0xFF) as u8;
                        let sb = (src_px & 0xFF) as u8;
                        buf.blend_pixel(px, py, sr, sg, sb, sa);
                    }
                }
            }
            draw_rounded_rect_stroke(buf, x, y, size, size, r, 1.0, Color::rgba(255, 255, 255, 0.20 * opacity));
            return;
        }
    }

    draw_rounded_rect(buf, x, y, size, size, 10.0, Color::rgba(30, 41, 59, 0.85 * opacity));
    draw_rounded_rect_stroke(buf, x, y, size, size, 10.0, 1.0, Color::rgba(255, 255, 255, 0.15 * opacity));

    let cx = x + size * 0.5;
    let cy = y + size * 0.5;
    draw_circle(buf, cx, cy, 14.0, Color::rgba(56, 189, 248, 0.9 * opacity));
    draw_circle(buf, cx, cy, 5.0, Color::rgba(15, 23, 42, 0.95 * opacity));
}

fn format_seconds(secs: u32) -> String {
    let m = secs / 60;
    let s = secs % 60;
    format!("{:02}:{:02}", m, s)
}

fn get_formatted_date_parts() -> (String, String) {
    unsafe {
        let st = windows::Win32::System::SystemInformation::GetLocalTime();
        let day_str = match st.wDayOfWeek {
            0 => "Sun",
            1 => "Mon",
            2 => "Tue",
            3 => "Wed",
            4 => "Thu",
            5 => "Fri",
            _ => "Sat",
        };
        let mon_str = match st.wMonth {
            1 => "Jan",
            2 => "Feb",
            3 => "Mar",
            4 => "Apr",
            5 => "May",
            6 => "Jun",
            7 => "Jul",
            8 => "Aug",
            9 => "Sep",
            10 => "Oct",
            11 => "Nov",
            _ => "Dec",
        };
        let date_part = format!("{} {} {} {}", day_str, st.wDay, mon_str, st.wYear);
        let time_part = format!("{:02}:{:02}", st.wHour, st.wMinute);
        (date_part, time_part)
    }
}

#[allow(dead_code)]
fn get_formatted_time_short() -> String {
    unsafe {
        let st = windows::Win32::System::SystemInformation::GetLocalTime();
        format!("{:02}:{:02}", st.wHour, st.wMinute)
    }
}

#[allow(dead_code)]
fn get_formatted_header_time() -> String {
    unsafe {
        let st = windows::Win32::System::SystemInformation::GetLocalTime();
        let day_str = match st.wDayOfWeek {
            0 => "Sun",
            1 => "Mon",
            2 => "Tue",
            3 => "Wed",
            4 => "Thu",
            5 => "Fri",
            _ => "Sat",
        };
        let mon_str = match st.wMonth {
            1 => "Jan",
            2 => "Feb",
            3 => "Mar",
            4 => "Apr",
            5 => "May",
            6 => "Jun",
            7 => "Jul",
            8 => "Aug",
            9 => "Sep",
            10 => "Oct",
            11 => "Nov",
            _ => "Dec",
        };
        format!("{} {} {} {} {:02}:{:02}", day_str, st.wDay, mon_str, st.wYear, st.wHour, st.wMinute)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_geometry() {
        let lm = LayoutManager::new();
        let mut state = IslandState::new();

        state.mode = IslandMode::Lyrics;
        let (_, _, w_lyr, h_lyr, r_lyr) = lm.compute_target_geometry(820.0, &state);
        assert_eq!((w_lyr, h_lyr, r_lyr), (600.0, 150.0, 24.0));
    }

    #[test]
    fn test_seek_relative_targets_registered() {
        let mut lm = LayoutManager::new();
        let tr = TextRenderer::new();
        let mut buf = PixelBuffer::new(820, 340);
        let mut state = IslandState::new();

        // 1. Music mode
        state.mode = IslandMode::Expanded(ExpandedPage::Music);
        let (x, _, w, h, _) = lm.compute_target_geometry(820.0, &state);
        lm.click_targets.clear();
        lm.render_music(&mut buf, &tr, x, w, h, &state, 1.0, None);
        assert!(lm.click_targets.iter().any(|(_, a)| *a == ClickAction::SeekRelative(-5)));
        assert!(lm.click_targets.iter().any(|(_, a)| *a == ClickAction::SeekRelative(5)));

        // 2. Lyrics mode
        state.mode = IslandMode::Lyrics;
        let (x, _, w, h, _) = lm.compute_target_geometry(820.0, &state);
        lm.click_targets.clear();
        let lyrics = SyncedLyrics::default();
        lm.render_lyrics(&mut buf, &tr, x, w, h, &state, &lyrics, 1.0, None);
        assert!(lm.click_targets.iter().any(|(_, a)| *a == ClickAction::SeekRelative(-5)));
        assert!(lm.click_targets.iter().any(|(_, a)| *a == ClickAction::SeekRelative(5)));
    }

    #[test]
    fn test_render_all_modes_visual() {
        use std::io::Write;

        let save_bmp = |buf: &PixelBuffer, path: &str| {
            let w = buf.width as u32;
            let h = buf.height as u32;
            let row_padded = ((w * 3 + 3) / 4) * 4;
            let file_size = 54 + row_padded * h;
            let mut file = std::fs::File::create(path).unwrap();
            let mut header = vec![0u8; 54];
            header[0] = b'B'; header[1] = b'M';
            header[2..6].copy_from_slice(&file_size.to_le_bytes());
            header[10..14].copy_from_slice(&54u32.to_le_bytes());
            header[14..18].copy_from_slice(&40u32.to_le_bytes());
            header[18..22].copy_from_slice(&(w as i32).to_le_bytes());
            header[22..26].copy_from_slice(&(h as i32).to_le_bytes());
            header[26..28].copy_from_slice(&1u16.to_le_bytes());
            header[28..30].copy_from_slice(&24u16.to_le_bytes());
            header[34..38].copy_from_slice(&(row_padded * h).to_le_bytes());
            file.write_all(&header).unwrap();

            let mut row = vec![0u8; row_padded as usize];
            for y in 0..h {
                let py = (h - 1 - y) as usize;
                for x in 0..w {
                    let px = x as usize;
                    let p = buf.data[py * buf.width + px];
                    let a = ((p >> 24) & 0xFF) as f32 / 255.0;
                    let r = (((p >> 16) & 0xFF) as f32).min(255.0);
                    let g = (((p >> 8) & 0xFF) as f32).min(255.0);
                    let b = ((p & 0xFF) as f32).min(255.0);

                    // Composite over dark-gray background so transparent regions look clear
                    let bg = 45.0f32;
                    let out_r = (r + bg * (1.0 - a)).clamp(0.0, 255.0) as u8;
                    let out_g = (g + bg * (1.0 - a)).clamp(0.0, 255.0) as u8;
                    let out_b = (b + bg * (1.0 - a)).clamp(0.0, 255.0) as u8;

                    row[px * 3] = out_b;
                    row[px * 3 + 1] = out_g;
                    row[px * 3 + 2] = out_r;
                }
                file.write_all(&row).unwrap();
            }
        };

        let mut lm = LayoutManager::new();
        let tr = TextRenderer::new();
        let scratch_dir = r"C:\Users\localAccount\.gemini\antigravity\brain\bfa7860f-bfe6-4d7e-a360-648c1094a9b7\scratch";

        // 1. Closed Mode (Resting notch with inverted corners)
        {
            let mut buf = PixelBuffer::new(820, 200);
            let mut state = IslandState::new();
            state.mode = IslandMode::Closed;
            let (tx, ty, tw, th, tr_val) = lm.compute_target_geometry(820.0, &state);
            let mut springs = SpringRect::new(tx, ty, tw, th, tr_val);
            springs.x.current = tx;
            springs.y.current = ty;
            springs.width.current = tw;
            springs.height.current = th;
            springs.corner_radius.current = tr_val;
            springs.opacity.current = 1.0;
            let lyrics = SyncedLyrics::default();
            lm.render(&mut buf, &tr, &springs, &state, &lyrics, None);
            save_bmp(&buf, &format!(r"{}\render_closed.bmp", scratch_dir));
        }

        // 1b. Music Mode
        {
            let mut buf = PixelBuffer::new(820, 200);
            let mut state = IslandState::new();
            state.mode = IslandMode::Expanded(ExpandedPage::Music);
            state.media_info.title = "NMIXX - DASH".to_string();
            state.media_info.artist = "NMIXX".to_string();
            state.media_info.duration_secs = 180;
            state.media_info.position_secs = 45;
            state.media_info.is_playing = true;
            state.media_info.has_media = true;
            state.lyrics_scroll_y = 28.0;
            let (tx, ty, tw, th, tr_val) = lm.compute_target_geometry(820.0, &state);
            let mut springs = SpringRect::new(tx, ty, tw, th, tr_val);
            springs.x.current = tx;
            springs.y.current = ty;
            springs.width.current = tw;
            springs.height.current = th;
            springs.corner_radius.current = tr_val;
            springs.opacity.current = 1.0;
            let lyrics = SyncedLyrics::default();
            lm.render(&mut buf, &tr, &springs, &state, &lyrics, None);
            save_bmp(&buf, &format!(r"{}\render_music.bmp", scratch_dir));
        }

        // 2. Lyrics Mode (No divider line, seek buttons)
        {
            let mut buf = PixelBuffer::new(820, 200);
            let mut state = IslandState::new();
            state.mode = IslandMode::Lyrics;
            state.media_info.title = "NMIXX - DASH".to_string();
            state.media_info.artist = "NMIXX".to_string();
            state.media_info.duration_secs = 180;
            state.media_info.position_secs = 45;
            state.media_info.is_playing = true;
            state.media_info.has_media = true;
            let mut lyrics = SyncedLyrics::default();
            lyrics.lines = vec![
                (35.0, "Inside someone like you now".to_string()),
                (45.0, "But had I known or I'd been shown back when how long It'd take me".to_string()),
                (55.0, "To break the charms that brought me harm that all but would erase me".to_string()),
                (65.0, "I never would no thought I could".to_string()),
            ];
            let layouts = LayoutManager::compute_lyric_layouts(&tr, &lyrics, 304.0);
            state.lyrics_scroll_y = LayoutManager::get_lyric_target_y(&layouts, 1);
            let (tx, ty, tw, th, tr_val) = lm.compute_target_geometry(820.0, &state);
            let mut springs = SpringRect::new(tx, ty, tw, th, tr_val);
            springs.x.current = tx;
            springs.y.current = ty;
            springs.width.current = tw;
            springs.height.current = th;
            springs.corner_radius.current = tr_val;
            springs.opacity.current = 1.0;
            lm.render(&mut buf, &tr, &springs, &state, &lyrics, None);
            save_bmp(&buf, &format!(r"{}\render_lyrics.bmp", scratch_dir));
        }

        // 3. To-Do Mode with active input & long text
        {
            let mut buf = PixelBuffer::new(820, 200);
            let mut state = IslandState::new();
            state.mode = IslandMode::Expanded(ExpandedPage::ToDo);
            state.todo.is_input_active = true;
            state.todo.input_buffer = "Beli buah segar di pasar pagi dan sayur bayam".to_string();
            state.anim_time = 0.1; // Cursor visible (0.1 * 2.5 = 0.25 < 0.5)
            let (tx, ty, tw, th, tr_val) = lm.compute_target_geometry(820.0, &state);
            let mut springs = SpringRect::new(tx, ty, tw, th, tr_val);
            springs.x.current = tx;
            springs.y.current = ty;
            springs.width.current = tw;
            springs.height.current = th;
            springs.corner_radius.current = tr_val;
            springs.opacity.current = 1.0;
            let lyrics = SyncedLyrics::default();
            lm.render(&mut buf, &tr, &springs, &state, &lyrics, None);
            save_bmp(&buf, &format!(r"{}\render_todo.bmp", scratch_dir));
        }

        // 4. Brightness Notification Mode (Apple monochrome white progress bar)
        {
            let mut buf = PixelBuffer::new(820, 200);
            let mut state = IslandState::new();
            state.mode = IslandMode::Notification(NotificationKind::Brightness { level: 65 });
            let (tx, ty, tw, th, tr_val) = lm.compute_target_geometry(820.0, &state);
            let mut springs = SpringRect::new(tx, ty, tw, th, tr_val);
            springs.x.current = tx;
            springs.y.current = ty;
            springs.width.current = tw;
            springs.height.current = th;
            springs.corner_radius.current = tr_val;
            springs.opacity.current = 1.0;
            let lyrics = SyncedLyrics::default();
            lm.render(&mut buf, &tr, &springs, &state, &lyrics, None);
            save_bmp(&buf, &format!(r"{}\render_brightness.bmp", scratch_dir));
        }

        // 5. Volume Notification Mode (Apple pure white progress bar)
        {
            let mut buf = PixelBuffer::new(820, 200);
            let mut state = IslandState::new();
            state.mode = IslandMode::Notification(NotificationKind::Volume { level: 80, muted: false });
            let (tx, ty, tw, th, tr_val) = lm.compute_target_geometry(820.0, &state);
            let mut springs = SpringRect::new(tx, ty, tw, th, tr_val);
            springs.x.current = tx;
            springs.y.current = ty;
            springs.width.current = tw;
            springs.height.current = th;
            springs.corner_radius.current = tr_val;
            springs.opacity.current = 1.0;
            let lyrics = SyncedLyrics::default();
            lm.render(&mut buf, &tr, &springs, &state, &lyrics, None);
            save_bmp(&buf, &format!(r"{}\render_volume.bmp", scratch_dir));
        }

        // 6. Focus Timer Mode
        {
            let mut buf = PixelBuffer::new(820, 200);
            let mut state = IslandState::new();
            state.mode = IslandMode::Expanded(ExpandedPage::Timer);
            state.focus_timer.remaining_seconds = (23 * 60 + 45) as f32;
            state.focus_timer.is_running = true;
            let (tx, ty, tw, th, tr_val) = lm.compute_target_geometry(820.0, &state);
            let mut springs = SpringRect::new(tx, ty, tw, th, tr_val);
            springs.x.current = tx;
            springs.y.current = ty;
            springs.width.current = tw;
            springs.height.current = th;
            springs.corner_radius.current = tr_val;
            springs.opacity.current = 1.0;
            let lyrics = SyncedLyrics::default();
            lm.render(&mut buf, &tr, &springs, &state, &lyrics, None);
            save_bmp(&buf, &format!(r"{}\render_timer.bmp", scratch_dir));
        }

        // 7. Resting Notch with Active Privacy Indicators + Network Speed
        {
            let mut buf = PixelBuffer::new(820, 200);
            let mut state = IslandState::new();
            state.mode = IslandMode::Resting;
            state.privacy.camera_active = true;
            state.privacy.mic_active = true;
            state.network_speed.download_bytes_sec = 2_840_000; // 2.8 MB/s
            let (tx, ty, tw, th, tr_val) = lm.compute_target_geometry(820.0, &state);
            let mut springs = SpringRect::new(tx, ty, tw, th, tr_val);
            springs.x.current = tx;
            springs.y.current = ty;
            springs.width.current = tw;
            springs.height.current = th;
            springs.corner_radius.current = tr_val;
            springs.opacity.current = 1.0;
            let lyrics = SyncedLyrics::default();
            lm.render(&mut buf, &tr, &springs, &state, &lyrics, None);
            save_bmp(&buf, &format!(r"{}\render_resting_privacy.bmp", scratch_dir));
        }

        // 8. Color Picked Notification Toast
        {
            let mut buf = PixelBuffer::new(820, 200);
            let mut state = IslandState::new();
            state.mode = IslandMode::Notification(NotificationKind::ColorPicked {
                hex: "#0A84FF".to_string(),
                rgb: "10, 132, 255".to_string(),
            });
            let (tx, ty, tw, th, tr_val) = lm.compute_target_geometry(820.0, &state);
            let mut springs = SpringRect::new(tx, ty, tw, th, tr_val);
            springs.x.current = tx;
            springs.y.current = ty;
            springs.width.current = tw;
            springs.height.current = th;
            springs.corner_radius.current = tr_val;
            springs.opacity.current = 1.0;
            let lyrics = SyncedLyrics::default();
            lm.render(&mut buf, &tr, &springs, &state, &lyrics, None);
            save_bmp(&buf, &format!(r"{}\render_color_picked.bmp", scratch_dir));
        }

        // 9. Tools & Control Center Page (5th Page)
        {
            let mut buf = PixelBuffer::new(820, 200);
            let mut state = IslandState::new();
            state.mode = IslandMode::Expanded(ExpandedPage::Tools);
            state.privacy.mic_muted = false;
            let (tx, ty, tw, th, tr_val) = lm.compute_target_geometry(820.0, &state);
            let mut springs = SpringRect::new(tx, ty, tw, th, tr_val);
            springs.x.current = tx;
            springs.y.current = ty;
            springs.width.current = tw;
            springs.height.current = th;
            springs.corner_radius.current = tr_val;
            springs.opacity.current = 1.0;
            let lyrics = SyncedLyrics::default();
            lm.render(&mut buf, &tr, &springs, &state, &lyrics, None);
            save_bmp(&buf, &format!(r"{}\render_tools.bmp", scratch_dir));

            // Verify click targets registered for all 3 cards
            assert!(lm.click_targets.iter().any(|(_, a)| *a == ClickAction::StartColorPicker));
            assert!(lm.click_targets.iter().any(|(_, a)| *a == ClickAction::TriggerSnip));
            assert!(lm.click_targets.iter().any(|(_, a)| *a == ClickAction::ToggleMicMute));
            assert!(lm.click_targets.iter().any(|(_, a)| *a == ClickAction::PrevPage));
            assert_eq!(lm.click_targets.iter().filter(|(_, a)| matches!(a, ClickAction::SelectPage(_))).count(), 5);
        }
    }
}
