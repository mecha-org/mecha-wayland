const SWIPE_MIN_DISTANCE: f64 = 40.0;
const SWIPE_MAX_DURATION_MS: u32 = 500;

#[derive(Clone, Debug)]
pub struct GestureSwipeData {
    pub direction: SwipeDirection,
    pub start_x: f64,
    pub start_y: f64,
    pub end_x: f64,
    pub end_y: f64,
    pub start_time: u32,
    pub end_time: u32,
    pub duration_ms: u32,
    pub velocity: f64, // pixels per millisecond
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
pub struct GestureDragData {
    pub state: DragState,
    pub start_x: f64,
    pub start_y: f64,
    pub x: f64,
    pub y: f64,
    pub delta_x: f64,
    pub delta_y: f64,
    pub total_dx: f64,
    pub total_dy: f64,
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
    start_x: f64,
    start_y: f64,
    last_x: f64,
    last_y: f64,
    start_time: u32,
    last_time: u32,
    pub drag_data: Option<GestureDragData>,
    pub swipe_data: Option<GestureSwipeData>,
}

impl GestureSingle {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn clear(&mut self) {
        self.swipe_data = None;
    }

    pub(crate) fn on_source_down(&mut self, x: f64, y: f64, time: u32) {
        self.clear();
        self.start_x = x;
        self.start_y = y;
        self.last_x = x;
        self.last_y = y;
        self.start_time = time;
        self.last_time = time;
        self.drag_data = Some(GestureDragData {
            state: DragState::Start,
            start_x: x,
            start_y: y,
            x,
            y,
            delta_x: 0.0,
            delta_y: 0.0,
            total_dx: 0.0,
            total_dy: 0.0,
        });
    }

    pub(crate) fn on_source_update(&mut self, x: f64, y: f64, time: u32) {
        self.clear();
        let dx = x - self.last_x;
        let dy = y - self.last_y;
        let total_dx = x - self.start_x;
        let total_dy = y - self.start_y;
        let start_x = self.start_x;
        let start_y = self.start_y;

        self.last_x = x;
        self.last_y = y;
        self.last_time = time;
        self.drag_data = Some(GestureDragData {
            state: DragState::Move,
            start_x,
            start_y,
            x,
            y,
            delta_x: dx,
            delta_y: dy,
            total_dx,
            total_dy,
        });
    }

    pub(crate) fn on_source_up(&mut self, time: u32) {
        self.clear();
        let x = self.last_x;
        let y = self.last_y;

        let total_dx = x - self.start_x;
        let total_dy = y - self.start_y;

        self.drag_data = Some(GestureDragData {
            state: DragState::End,
            start_x: self.start_x,
            start_y: self.start_y,
            x: x,
            y: y,
            delta_x: 0.0,
            delta_y: 0.0,
            total_dx: total_dx,
            total_dy: total_dy,
        });

        let dx = x - self.start_x;
        let dy = y - self.start_y;
        let distance = (dx * dx + dy * dy).sqrt();
        let duration_ms = time.saturating_sub(self.start_time);

        if distance >= SWIPE_MIN_DISTANCE && duration_ms <= SWIPE_MAX_DURATION_MS {
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
            self.swipe_data = Some(GestureSwipeData {
                direction,
                start_x: self.start_x,
                start_y: self.start_y,
                end_x: x,
                end_y: y,
                start_time: self.start_time,
                end_time: time,
                duration_ms,
                velocity,
            });
        }
    }

    pub(crate) fn on_source_cancel(&mut self) {
        self.drag_data = Some(GestureDragData {
            state: DragState::Cancel,
            start_x: self.start_x,
            start_y: self.start_y,
            x: self.last_x,
            y: self.last_y,
            delta_x: 0.0,
            delta_y: 0.0,
            total_dx: self.last_x - self.start_x,
            total_dy: self.last_y - self.start_y,
        });
    }
}
