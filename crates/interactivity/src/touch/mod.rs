use crate::gesture::GestureSingle;
use std::collections::HashMap;
use utils::Rect;
use wayland::WlTouchEvent;

const TAP_MAX_DISTANCE: f64 = 15.0;
const TAP_MAX_DURATION_MS: u32 = 300;

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
    x: f64,
    y: f64,
    active_touches: HashMap<i32, ActiveTouch>,
    pointer_touch_id: Option<i32>,
    pub gesture_single: GestureSingle,
    just_tapped: bool,
    held: bool,
    just_hold_released: bool,
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
                // If no pointer touch id is set, set the first touch id as the pointer touch id
                if self.pointer_touch_id.is_none() {
                    if self.active_touches.is_empty() {
                        self.pointer_touch_id = Some(*id);
                        self.x = x;
                        self.y = y;
                        self.gesture_single.on_source_down(x, y, *time);
                    }
                }
                self.active_touches.insert(*id, active);
            }

            WlTouchEvent::Motion { id, x, y, time, .. } => {
                let x = *x as f64 / 256.0;
                let y = *y as f64 / 256.0;
                if let Some(active) = self.active_touches.get_mut(id) {
                    active.last_x = x;
                    active.last_y = y;
                    active.last_time = *time;
                }

                if self.pointer_touch_id == Some(*id) {
                    self.x = x;
                    self.y = y;
                    if let Some(active) = self.active_touches.get(id) {
                        let dx = x - active.start_x;
                        let dy = y - active.start_y;
                        let distance = (dx * dx + dy * dy).sqrt();
                        let duration_ms = time.saturating_sub(active.start_time);

                        if distance < TAP_MAX_DISTANCE && duration_ms > TAP_MAX_DURATION_MS {
                            self.held = true;
                        }
                    }
                    self.gesture_single.on_source_update(x, y, *time);
                }
            }

            WlTouchEvent::Up { id, time, .. } => {
                if let Some(active) = self.active_touches.remove(id)
                    && self.pointer_touch_id == Some(*id)
                {
                    let x = active.last_x;
                    let y = active.last_y;
                    let dx = x - active.start_x;
                    let dy = y - active.start_y;
                    let distance = (dx * dx + dy * dy).sqrt();
                    let duration_ms = time.saturating_sub(active.start_time);

                    if distance < TAP_MAX_DISTANCE {
                        if duration_ms < TAP_MAX_DURATION_MS {
                            // tap
                            self.just_tapped = true;
                            self.held = false;
                        } else {
                            // TODO What about hold and drag like text select?
                            self.held = false;
                            self.just_hold_released = true;
                        }
                    } else {
                        // swipe or drag
                        self.gesture_single.on_source_up(*time);
                    }
                }

                // New pointer touch id candidate among active touches
                if self.pointer_touch_id == Some(*id) {
                    self.pointer_touch_id = self.get_earliest_touch();
                }
            }

            WlTouchEvent::Cancel { .. } => {
                self.active_touches.clear();
                self.gesture_single.on_source_cancel();
            }

            WlTouchEvent::Frame { .. } => (),

            _ => (),
        }
    }

    pub fn clear(&mut self) {
        self.just_tapped = false;
        self.just_hold_released = false;
    }

    /// Returns the current primary touch position.
    pub fn position(&self) -> (f64, f64) {
        (self.x, self.y)
    }

    /// Returns true if the primary touch was tapped within the given bounds in this frame.
    pub fn tapped(&self, bounds: Rect) -> bool {
        self.just_tapped && bounds.contains(self.x, self.y)
    }

    /// Returns true if the primary touch was held down within the given bounds.
    pub fn held(&self, bounds: Rect) -> bool {
        self.held && bounds.contains(self.x, self.y)
    }

    /// Returns true if the primary touch was released after being held down within the given bounds in this frame.
    pub fn hold_released(&self, bounds: Rect) -> bool {
        self.just_hold_released && bounds.contains(self.x, self.y)
    }

    /// Get the touch with the earliest start time.
    /// We can consider changing HashMap to BTreeMap sorted by time if called frequently.
    fn get_earliest_touch(&self) -> Option<i32> {
        if self.active_touches.is_empty() {
            return None;
        }

        let mut earliest_id = 0;
        let mut earliest_time = u32::MAX;
        for (id, active) in &self.active_touches {
            if active.start_time < earliest_time {
                earliest_time = active.start_time;
                earliest_id = *id;
            }
        }

        Some(earliest_id)
    }
}
