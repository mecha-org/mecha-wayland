use std::time::Duration;
use utils::Point;

const SWIPE_MIN_DISTANCE: f32 = 40.0;
const SWIPE_MAX_DURATION: Duration = Duration::from_millis(500);

#[derive(Clone, Debug)]
pub struct SwipeData {
    pub direction: SwipeDirection,
    pub start_position: Point,
    pub end_position: Point,
    pub start_time: Duration,
    pub end_time: Duration,
    pub duration: Duration,
    pub velocity: f32, // pixels per millisecond
}

/// Phase of a drag gesture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragState {
    Start,
    Move,
    End,
    Cancel,
}

#[derive(Clone, Debug)]
pub struct DragData {
    pub state: DragState,
    pub start_position: Point,
    pub current_position: Point,
    pub delta: Point,
    pub total: Point,
}

/// Swipe direction for swipe gestures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwipeDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Default)]
pub struct GestureSingle {
    start_position: Point,
    last_position: Point,
    start_time: Duration,
    last_time: Duration,
    drag_data: Option<DragData>,
    swipe_data: Option<SwipeData>,
}

impl GestureSingle {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.swipe_data = None;
        if let Some(drag_data) = &self.drag_data {
            if drag_data.state == DragState::Cancel || drag_data.state == DragState::End {
                self.drag_data = None;
            }
        }
    }

    pub fn drag_data(&self) -> Option<&DragData> {
        self.drag_data.as_ref()
    }

    pub fn swipe_data(&self) -> Option<&SwipeData> {
        self.swipe_data.as_ref()
    }

    pub(crate) fn on_source_down(&mut self, position: Point, time: Duration) {
        self.start_position = position;
        self.last_position = position;
        self.start_time = time;
        self.last_time = time;
        self.drag_data = Some(DragData {
            state: DragState::Start,
            start_position: position,
            current_position: position,
            delta: Point::ZERO,
            total: Point::ZERO,
        });
    }

    pub(crate) fn on_source_update(&mut self, position: Point, time: Duration) {
        let delta = position - self.last_position;
        let total = position - self.start_position;

        self.last_position = position;
        self.last_time = time;
        self.drag_data = Some(DragData {
            state: DragState::Move,
            start_position: self.start_position,
            current_position: position,
            delta,
            total,
        });
    }

    pub(crate) fn on_source_up(&mut self, time: Duration) {
        let position = self.last_position;
        let total = position - self.start_position;

        self.drag_data = Some(DragData {
            state: DragState::End,
            start_position: self.start_position,
            current_position: position,
            delta: Point::ZERO,
            total,
        });

        let distance = total.length();
        let duration = time.saturating_sub(self.start_time);

        if distance >= SWIPE_MIN_DISTANCE && duration <= SWIPE_MAX_DURATION {
            let direction = if total.x().abs() > total.y().abs() {
                if total.x() > 0.0 {
                    SwipeDirection::Right
                } else {
                    SwipeDirection::Left
                }
            } else {
                if total.y() > 0.0 {
                    SwipeDirection::Down
                } else {
                    SwipeDirection::Up
                }
            };
            let duration_ms = duration.as_secs_f32() * 1000.0;
            let velocity = if duration_ms > 0.0 {
                distance / duration_ms
            } else {
                distance
            };
            self.swipe_data = Some(SwipeData {
                direction,
                start_position: self.start_position,
                end_position: position,
                start_time: self.start_time,
                end_time: time,
                duration,
                velocity,
            });
        }
    }

    pub(crate) fn on_source_cancel(&mut self) {
        let total = self.last_position - self.start_position;
        self.drag_data = Some(DragData {
            state: DragState::Cancel,
            start_position: self.start_position,
            current_position: self.last_position,
            delta: Point::ZERO,
            total,
        });
        self.clear();
    }
}
