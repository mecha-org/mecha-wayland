use std::time::Duration;

use animation::{Animated, AnimationConfig, Easing, monotonic_now};

pub const DEFAULT_ANIMATION_DURATION: Duration = Duration::from_millis(300);
pub const DEFAULT_ANIMATION_EASING: Easing = Easing::EaseOut;
pub const DRAG_EDGE_RESISTANCE: f32 = 0.06;
pub const SLOW_SWIPE_FRACTION_THRESHOLD: f32 = 0.35;

#[derive(Debug, Clone)]
pub struct PagerState {
    pub current_page: usize,
    pub page_count: usize,
    pub drag_start: Option<f64>,
    pub drag_offset: f32,
    pub animation_offset: Animated<f32>,
    pub is_dragging: bool,
    pub animation_config: AnimationConfig,
}

impl PagerState {
    pub fn new(page_count: usize) -> Self {
        let config = AnimationConfig::new(DEFAULT_ANIMATION_DURATION, DEFAULT_ANIMATION_EASING);
        Self {
            current_page: 0,
            page_count,
            drag_start: None,
            drag_offset: 0.0,
            animation_offset: Animated::static_value(0.0),
            is_dragging: false,
            animation_config: config,
        }
    }

    pub fn next_page(&mut self) {
        if self.current_page + 1 < self.page_count && !self.is_dragging {
            let now = monotonic_now();
            let start = self.animation_offset.get(now);
            self.current_page += 1;
            self.animation_offset =
                Animated::new(start, self.current_page as f32, self.animation_config, now);
        }
    }

    pub fn previous_page(&mut self) {
        if self.current_page > 0 && !self.is_dragging {
            let now = monotonic_now();
            let start = self.animation_offset.get(now);
            self.current_page -= 1;
            self.animation_offset =
                Animated::new(start, self.current_page as f32, self.animation_config, now);
        }
    }

    pub fn current_visual_offset(&self) -> f32 {
        if self.is_dragging {
            self.current_page as f32
        } else {
            self.animation_offset.get(monotonic_now())
        }
    }

    pub fn handle_drag_start(&mut self, start_pos: f64) {
        self.is_dragging = true;
        self.drag_start = Some(start_pos);
        self.drag_offset = 0.0;
        self.animation_offset = Animated::static_value(self.current_page as f32);
    }

    pub fn handle_drag_move(&mut self, current_pos: f64) {
        if let Some(start) = self.drag_start {
            let delta = (current_pos - start) as f32;

            if (self.current_page == 0 && delta > 0.0)
                || (self.current_page + 1 == self.page_count && delta < 0.0)
            {
                self.drag_offset = delta * DRAG_EDGE_RESISTANCE;
            } else {
                self.drag_offset = delta;
            }
        }
    }

    pub fn handle_drag_end(&mut self, page_size: f32) {
        if !self.is_dragging {
            return;
        }
        self.is_dragging = false;
        self.drag_start = None;

        let drag_frac = self.drag_offset / page_size.max(1.0);
        let mut target_page = self.current_page;

        if drag_frac < -SLOW_SWIPE_FRACTION_THRESHOLD
            && target_page < self.page_count.saturating_sub(1)
        {
            target_page += 1;
        } else if drag_frac > SLOW_SWIPE_FRACTION_THRESHOLD && target_page > 0 {
            target_page -= 1;
        }

        let start_frac = self.current_page as f32 - drag_frac;
        self.current_page = target_page;

        let now = monotonic_now();
        if (start_frac - target_page as f32).abs() < f32::EPSILON {
            self.animation_offset = Animated::static_value(target_page as f32);
        } else {
            self.animation_offset =
                Animated::new(start_frac, target_page as f32, self.animation_config, now);
        }
        self.drag_offset = 0.0;
    }

    pub fn is_animating(&self) -> bool {
        self.animation_offset.is_animating(monotonic_now())
    }
}
