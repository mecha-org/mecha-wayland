use std::collections::HashMap;
use wayland::WlTouchEvent;

const TAP_MAX_DISTANCE: f64 = 15.0;
const TAP_MAX_DURATION_MS: u32 = 300;
const SWIPE_MIN_DISTANCE: f64 = 40.0;
const SWIPE_MAX_DURATION_MS: u32 = 500;

/// Swipe direction for swipe gestures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwipeDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Phase of a drag gesture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragState {
    Start,
    Move,
    End,
    Cancel,
}

#[derive(Debug, Clone)]
struct ActiveTouch {
    start_x: f64,
    start_y: f64,
    last_x: f64,
    last_y: f64,
    start_time: u32,
    last_time: u32,
}

#[derive(Debug, Default)]
pub struct TouchState {
    active_touches: HashMap<i32, ActiveTouch>,
}

impl TouchState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process(&mut self, ev: &WlTouchEvent) {
        match ev {
            WlTouchEvent::Down { id, x, y, time, .. } => {
                let x = *x as f64 / 256.0;
                let y = *y as f64 / 256.0;
                let active = ActiveTouch {
                    start_x: x,
                    start_y: y,
                    last_x: x,
                    last_y: y,
                    start_time: *time,
                    last_time: *time,
                };
                self.active_touches.insert(*id, active);
            }

            WlTouchEvent::Motion { id, x, y, time, .. } => {
                let x = *x as f64 / 256.0;
                let y = *y as f64 / 256.0;
                if let Some(active) = self.active_touches.get_mut(id) {
                    let dx = x - active.last_x;
                    let dy = y - active.last_y;
                    let total_dx = x - active.start_x;
                    let total_dy = y - active.start_y;
                    let start_x = active.start_x;
                    let start_y = active.start_y;

                    active.last_x = x;
                    active.last_y = y;
                    active.last_time = *time;
                }
            }

            WlTouchEvent::Up { id, time, .. } => {
                if let Some(active) = self.active_touches.remove(id) {
                    let x = active.last_x;
                    let y = active.last_y;

                    let total_dx = x - active.start_x;
                    let total_dy = y - active.start_y;

                    let dx = x - active.start_x;
                    let dy = y - active.start_y;
                    let distance = (dx * dx + dy * dy).sqrt();
                    let duration_ms = time.saturating_sub(active.start_time);

                    if distance < TAP_MAX_DISTANCE && duration_ms < TAP_MAX_DURATION_MS {
                        // tap
                    } else if distance >= SWIPE_MIN_DISTANCE && duration_ms <= SWIPE_MAX_DURATION_MS
                    {
                        let direction = if dx.abs() > dy.abs() {
                            if dx > 0.0 {
                                SwipeDirection::Right
                            } else {
                                SwipeDirection::Left
                            }
                        } else {
                            if dy > 0.0 {
                                SwipeDirection::Down
                            } else {
                                SwipeDirection::Up
                            }
                        };
                        let velocity = if duration_ms > 0 {
                            distance / duration_ms as f64
                        } else {
                            distance
                        };
                    }
                }
            }

            WlTouchEvent::Cancel { .. } => {
                for (id, active) in &self.active_touches {
                    // drag cancel
                }
                self.active_touches.clear();
            }

            WlTouchEvent::Frame { .. } => (),

            _ => (),
        }
    }
}
