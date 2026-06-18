use std::time::{Duration, Instant};

use app::prelude::State;
use io_ring::Ring;
use renderer::{DmaBuf, RenderableSurface, Renderer, TextureId};
use timer::Timer;
use utils::Rect;
use wayland::Wayland;

// Domain Models

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActiveTab {
    Clock,
    Stopwatch,
}

pub struct StopwatchState {
    pub is_running: bool,
    pub start_instant: Option<Instant>,
    pub accumulated_duration: Duration,
    pub laps: Vec<Duration>,
}

impl Default for StopwatchState {
    fn default() -> Self {
        Self {
            is_running: false,
            start_instant: None,
            accumulated_duration: Duration::ZERO,
            laps: Vec::new(),
        }
    }
}

impl StopwatchState {
    pub fn toggle(&mut self) {
        if self.is_running {
            if let Some(start) = self.start_instant.take() {
                self.accumulated_duration += start.elapsed();
            }
            self.is_running = false;
        } else {
            self.start_instant = Some(Instant::now());
            self.is_running = true;
        }
    }

    pub fn lap_or_reset(&mut self) {
        if self.is_running {
            let current = self.accumulated_duration
                + self
                    .start_instant
                    .map(|i| i.elapsed())
                    .unwrap_or(Duration::ZERO);
            self.laps.push(current);
        } else {
            self.is_running = false;
            self.start_instant = None;
            self.accumulated_duration = Duration::ZERO;
            self.laps.clear();
        }
    }
}

// UI Models

use crate::ui::theme::ActiveTheme;

#[derive(Debug, Default, Clone, Copy)]
pub struct HitBoxes {
    pub clock_tab: Rect,
    pub stopwatch_tab: Rect,
    pub start_stop_btn: Rect,
    pub lap_reset_btn: Rect,
    pub settings_btn: Rect,
    pub format_toggle_btn: Rect,
    pub seconds_toggle_btn: Rect,
    pub theme_toggle_btn: Rect,
    pub done_btn: Rect,
}

/// All non-module application state that doesn't need a `Lens` impl.
pub struct UiState {
    pub surface_id: u32,
    pub surface_size: (i32, i32),
    pub dmabuf: [Option<RenderableSurface<DmaBuf>>; 2],
    pub wl_buf_ids: [u32; 2],
    pub buf_in_flight: [bool; 2],
    pub icon_tex: Option<TextureId>,

    pub active_tab: ActiveTab,
    pub stopwatch: StopwatchState,
    pub cursor_x: f64,
    pub cursor_y: f64,
    pub hit_boxes: HitBoxes,
    pub show_settings: bool,
    pub format_24h: bool,
    pub show_seconds: bool,
    pub theme: ActiveTheme,
}

impl UiState {
    fn new() -> Self {
        Self {
            surface_id: 0,
            surface_size: (0, 0),
            dmabuf: [None, None],
            wl_buf_ids: [0, 0],
            buf_in_flight: [false, false],
            icon_tex: None,
            active_tab: ActiveTab::Clock,
            stopwatch: StopwatchState::default(),
            cursor_x: 0.0,
            cursor_y: 0.0,
            hit_boxes: HitBoxes::default(),
            show_settings: false,
            format_24h: false,
            show_seconds: true,
            theme: ActiveTheme::Dark,
        }
    }
}

#[derive(State)]
pub struct AppState {
    pub ring: Ring,
    pub timer: Timer,
    pub wayland: Wayland,
    pub renderer: Renderer,
    pub ui: UiState,
}

impl AppState {
    pub fn new() -> Self {
        let ring = Ring::default();
        let timer = Timer::new(ring.get_proxy());
        let wayland = Wayland::new(ring.get_proxy()).expect("Failed to initialize Wayland");
        let renderer = Renderer::new().expect("Failed to initialize Renderer");

        Self {
            ring,
            timer,
            wayland,
            renderer,
            ui: UiState::new(),
        }
    }
}
