use crate::gesture::GestureSingle;
use std::collections::HashMap;
use std::time::Duration;
use utils::{Point, Rect};
use wayland::WlTouchEvent;

const TAP_MAX_DISTANCE: f32 = 15.0;
const TAP_MAX_DURATION: Duration = Duration::from_millis(300);

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
    start_position: Point,
    last_position: Point,
    start_time: Duration,
    last_time: Duration,
}

#[derive(Debug, Default)]
pub struct TouchState {
    position: Point,
    active_touches: HashMap<i32, ActiveTouch>,
    pointer_touch_id: Option<i32>,
    just_tapped: bool,
    held: bool,
    just_hold_released: bool,
}

impl TouchState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process(&mut self, ev: &WlTouchEvent, gesture: &mut GestureSingle) {
        match ev {
            WlTouchEvent::Down { id, x, y, time, .. } => {
                let position = Point::new(*x as f32 / 256.0, *y as f32 / 256.0);
                let time_dur = Duration::from_millis(*time as u64);
                let active = ActiveTouch {
                    start_position: position,
                    last_position: position,
                    start_time: time_dur,
                    last_time: time_dur,
                };
                // If no pointer touch id is set, set the first touch id as the pointer touch id
                if self.pointer_touch_id.is_none() {
                    if self.active_touches.is_empty() {
                        self.pointer_touch_id = Some(*id);
                        self.position = position;
                        gesture.on_source_down(position, time_dur);
                    }
                }
                self.active_touches.insert(*id, active);
            }

            WlTouchEvent::Motion { id, x, y, time, .. } => {
                let position = Point::new(*x as f32 / 256.0, *y as f32 / 256.0);
                let time_dur = Duration::from_millis(*time as u64);
                if let Some(active) = self.active_touches.get_mut(id) {
                    active.last_position = position;
                    active.last_time = time_dur;
                }

                if self.pointer_touch_id == Some(*id) {
                    self.position = position;
                    if let Some(active) = self.active_touches.get(id) {
                        let dx = position.x() - active.start_position.x();
                        let dy = position.y() - active.start_position.y();
                        let distance = (dx * dx + dy * dy).sqrt();
                        let duration = time_dur.saturating_sub(active.start_time);

                        if distance < TAP_MAX_DISTANCE && duration > TAP_MAX_DURATION {
                            self.held = true;
                        }
                    }
                    gesture.on_source_update(position, time_dur);
                }
            }

            WlTouchEvent::Up { id, time, .. } => {
                let time_dur = Duration::from_millis(*time as u64);
                if let Some(active) = self.active_touches.remove(id)
                    && self.pointer_touch_id == Some(*id)
                {
                    let dx = active.last_position.x() - active.start_position.x();
                    let dy = active.last_position.y() - active.start_position.y();
                    let distance = (dx * dx + dy * dy).sqrt();
                    let duration = time_dur.saturating_sub(active.start_time);

                    if distance < TAP_MAX_DISTANCE {
                        if duration < TAP_MAX_DURATION {
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
                        gesture.on_source_up(time_dur);
                    }
                }

                // New pointer touch id candidate among active touches
                if self.pointer_touch_id == Some(*id) {
                    self.pointer_touch_id = self.get_earliest_touch();
                }
            }

            WlTouchEvent::Cancel { .. } => {
                self.active_touches.clear();
                gesture.on_source_cancel();
            }

            WlTouchEvent::Frame { .. } => (),

            _ => (),
        }
    }

    pub fn clear(&mut self, gesture: &mut GestureSingle) {
        self.just_tapped = false;
        self.just_hold_released = false;
        gesture.clear();
    }

    /// Returns the current primary touch position.
    pub fn position(&self) -> Point {
        self.position
    }

    /// Returns true if the primary touch was tapped within the given bounds in this frame.
    pub fn tapped(&self, bounds: Rect) -> bool {
        self.just_tapped && bounds.contains_point(self.position)
    }

    /// Returns true if the primary touch was held down within the given bounds.
    pub fn held(&self, bounds: Rect) -> bool {
        self.held && bounds.contains_point(self.position)
    }

    /// Returns true if the primary touch was released after being held down within the given bounds in this frame.
    pub fn hold_released(&self, bounds: Rect) -> bool {
        self.just_hold_released && bounds.contains_point(self.position)
    }

    /// Get the touch with the earliest start time.
    /// We can consider changing HashMap to BTreeMap sorted by time if called frequently.
    fn get_earliest_touch(&self) -> Option<i32> {
        if self.active_touches.is_empty() {
            return None;
        }

        let mut earliest_id = 0;
        let mut earliest_time = Duration::MAX;
        for (id, active) in &self.active_touches {
            if active.start_time < earliest_time {
                earliest_time = active.start_time;
                earliest_id = *id;
            }
        }

        Some(earliest_id)
    }
}
