//! Focus Timer & Stopwatch (Pomodoro & Productivity)
//! Supports Focus sessions (25m), Short breaks (5m), Long breaks (15m), and Stopwatch.
//! Highly responsive, zero CPU when paused.

use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimerMode {
    Focus,
    ShortBreak,
    #[allow(dead_code)]
    LongBreak,
    Stopwatch,
}

impl TimerMode {
    pub fn default_duration_secs(&self) -> u32 {
        match self {
            Self::Focus => 25 * 60,
            Self::ShortBreak => 5 * 60,
            Self::LongBreak => 15 * 60,
            Self::Stopwatch => 0,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Focus => "Focus",
            Self::ShortBreak => "Short Break",
            Self::LongBreak => "Long Break",
            Self::Stopwatch => "Stopwatch",
        }
    }
}

#[derive(Clone, Debug)]
pub struct FocusTimer {
    pub mode: TimerMode,
    pub is_running: bool,
    pub total_seconds: u32,
    pub remaining_seconds: f32,
    pub elapsed_stopwatch: f32,
    pub sessions_completed: u32,
    pub last_tick: Option<Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimerEvent {
    Completed(TimerMode),
}

impl FocusTimer {
    pub fn new() -> Self {
        let mode = TimerMode::Focus;
        let total = mode.default_duration_secs();
        Self {
            mode,
            is_running: false,
            total_seconds: total,
            remaining_seconds: total as f32,
            elapsed_stopwatch: 0.0,
            sessions_completed: 0,
            last_tick: None,
        }
    }

    pub fn set_mode(&mut self, mode: TimerMode) {
        self.mode = mode;
        self.is_running = false;
        self.total_seconds = mode.default_duration_secs();
        self.remaining_seconds = self.total_seconds as f32;
        self.elapsed_stopwatch = 0.0;
        self.last_tick = None;
    }

    pub fn set_duration_mins(&mut self, mins: u32) {
        self.total_seconds = mins * 60;
        self.remaining_seconds = (mins * 60) as f32;
    }

    pub fn toggle(&mut self) {
        if self.is_running {
            self.pause();
        } else {
            self.start();
        }
    }

    pub fn start(&mut self) {
        self.is_running = true;
        self.last_tick = Some(Instant::now());
    }

    pub fn pause(&mut self) {
        self.is_running = false;
        self.last_tick = None;
    }

    pub fn reset(&mut self) {
        self.is_running = false;
        self.last_tick = None;
        if self.mode == TimerMode::Stopwatch {
            self.elapsed_stopwatch = 0.0;
        } else {
            self.remaining_seconds = self.total_seconds as f32;
        }
    }

    pub fn tick(&mut self, now: Instant) -> Option<TimerEvent> {
        if !self.is_running {
            return None;
        }

        let last = self.last_tick.unwrap_or(now);
        let dt = now.duration_since(last).as_secs_f32();
        self.last_tick = Some(now);

        if self.mode == TimerMode::Stopwatch {
            self.elapsed_stopwatch += dt;
            None
        } else {
            self.remaining_seconds -= dt;
            if self.remaining_seconds <= 0.0 {
                self.remaining_seconds = 0.0;
                self.is_running = false;
                self.last_tick = None;
                if self.mode == TimerMode::Focus {
                    self.sessions_completed += 1;
                }
                Some(TimerEvent::Completed(self.mode))
            } else {
                None
            }
        }
    }

    pub fn progress(&self) -> f32 {
        if self.mode == TimerMode::Stopwatch {
            ((self.elapsed_stopwatch % 60.0) / 60.0).clamp(0.0, 1.0)
        } else if self.total_seconds > 0 {
            (1.0 - (self.remaining_seconds / self.total_seconds as f32)).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub fn formatted_time(&self) -> (String, String) {
        if self.mode == TimerMode::Stopwatch {
            let total_secs = self.elapsed_stopwatch as u32;
            let mins = total_secs / 60;
            let secs = total_secs % 60;
            let centis = ((self.elapsed_stopwatch - total_secs as f32) * 100.0) as u32;
            (format!("{:02}:{:02}", mins, secs), format!(".{:02}", centis))
        } else {
            let total_secs = self.remaining_seconds.ceil() as u32;
            let mins = total_secs / 60;
            let secs = total_secs % 60;
            (format!("{:02}:{:02}", mins, secs), String::new())
        }
    }
}
