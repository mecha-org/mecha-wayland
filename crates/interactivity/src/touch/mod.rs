mod event;

pub use event::{DragState, SwipeDirection, TouchEvent};

use std::collections::HashMap;
use wayland::TouchEvent as WlTouchEvent;

const TAP_MAX_DISTANCE: f64 = 15.0;
const TAP_MAX_DURATION_MS: u32 = 300;
const SWIPE_MIN_DISTANCE: f64 = 40.0;
const SWIPE_MAX_DURATION_MS: u32 = 500;

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

    /// Process a raw Wayland touch event and return semantic TouchEvents.
    pub fn process(&mut self, ev: &WlTouchEvent) -> Vec<TouchEvent> {
        let mut events = Vec::new();

        match ev {
            WlTouchEvent::Down { id, x, y, time, .. } => {
                let active = ActiveTouch {
                    start_x: *x,
                    start_y: *y,
                    last_x: *x,
                    last_y: *y,
                    start_time: *time,
                    last_time: *time,
                };
                self.active_touches.insert(*id, active);
                events.push(TouchEvent::Down {
                    id: *id,
                    x: *x,
                    y: *y,
                    time: *time,
                });
                events.push(TouchEvent::Drag {
                    id: *id,
                    state: DragState::Start,
                    start_x: *x,
                    start_y: *y,
                    x: *x,
                    y: *y,
                    delta_x: 0.0,
                    delta_y: 0.0,
                    total_dx: 0.0,
                    total_dy: 0.0,
                });
            }

            WlTouchEvent::Motion { id, x, y, time } => {
                if let Some(active) = self.active_touches.get_mut(id) {
                    let dx = x - active.last_x;
                    let dy = y - active.last_y;
                    let total_dx = x - active.start_x;
                    let total_dy = y - active.start_y;
                    let start_x = active.start_x;
                    let start_y = active.start_y;

                    active.last_x = *x;
                    active.last_y = *y;
                    active.last_time = *time;

                    events.push(TouchEvent::Motion {
                        id: *id,
                        x: *x,
                        y: *y,
                        dx,
                        dy,
                        time: *time,
                    });
                    events.push(TouchEvent::Drag {
                        id: *id,
                        state: DragState::Move,
                        start_x,
                        start_y,
                        x: *x,
                        y: *y,
                        delta_x: dx,
                        delta_y: dy,
                        total_dx,
                        total_dy,
                    });
                }
            }

            WlTouchEvent::Up { id, time, .. } => {
                if let Some(active) = self.active_touches.remove(id) {
                    let x = active.last_x;
                    let y = active.last_y;
                    events.push(TouchEvent::Up {
                        id: *id,
                        x,
                        y,
                        time: *time,
                    });

                    let total_dx = x - active.start_x;
                    let total_dy = y - active.start_y;

                    events.push(TouchEvent::Drag {
                        id: *id,
                        state: DragState::End,
                        start_x: active.start_x,
                        start_y: active.start_y,
                        x,
                        y,
                        delta_x: 0.0,
                        delta_y: 0.0,
                        total_dx,
                        total_dy,
                    });

                    let dx = x - active.start_x;
                    let dy = y - active.start_y;
                    let distance = (dx * dx + dy * dy).sqrt();
                    let duration_ms = time.saturating_sub(active.start_time);

                    // Check for Tap: small movement and short duration
                    if distance < TAP_MAX_DISTANCE && duration_ms < TAP_MAX_DURATION_MS {
                        events.push(TouchEvent::Tap { id: *id, x, y });
                    } else if distance >= SWIPE_MIN_DISTANCE && duration_ms <= SWIPE_MAX_DURATION_MS
                    {
                        // Check for Swipe: larger movement (>=40px) and fast motion (<=500ms)
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

                        events.push(TouchEvent::Swipe {
                            direction,
                            start_x: active.start_x,
                            start_y: active.start_y,
                            end_x: x,
                            end_y: y,
                            start_time: active.start_time,
                            end_time: *time,
                            duration_ms,
                            velocity,
                        });
                    }
                }
            }

            WlTouchEvent::Cancel => {
                for (id, active) in &self.active_touches {
                    events.push(TouchEvent::Drag {
                        id: *id,
                        state: DragState::Cancel,
                        start_x: active.start_x,
                        start_y: active.start_y,
                        x: active.last_x,
                        y: active.last_y,
                        delta_x: 0.0,
                        delta_y: 0.0,
                        total_dx: active.last_x - active.start_x,
                        total_dy: active.last_y - active.start_y,
                    });
                }
                self.active_touches.clear();
                events.push(TouchEvent::Cancel);
            }

            WlTouchEvent::Frame => {
                events.push(TouchEvent::Frame);
            }

            _ => {}
        }

        events
    }
}
