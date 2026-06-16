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
pub enum DragPhase {
    Start,
    Move,
    End,
    Cancel,
}

/// High-level touch event emitted by the interactivity module.
#[derive(Clone, Debug)]
pub enum TouchEvent {
    /// A touch contact was pressed down.
    Down { id: i32, x: f64, y: f64, time: u32 },

    /// A touch contact was released.
    Up { id: i32, x: f64, y: f64, time: u32 },

    /// A touch contact moved.
    Motion {
        id: i32,
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
        time: u32,
    },

    /// A single tap gesture was detected.
    Tap { id: i32, x: f64, y: f64 },

    /// A swipe gesture was detected.
    Swipe {
        direction: SwipeDirection,
        start_x: f64,
        start_y: f64,
        end_x: f64,
        end_y: f64,
        start_time: u32,
        end_time: u32,
        duration_ms: u32,
        velocity: f64, // pixels per millisecond
    },

    /// A continuous drag gesture.
    Drag {
        id: i32,
        phase: DragPhase,
        start_x: f64,
        start_y: f64,
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
        total_dx: f64,
        total_dy: f64,
    },

    /// The compositor cancelled the active touch points.
    Cancel,

    /// End of a logical group of simultaneous touch updates.
    Frame,
}

impl app::Event for TouchEvent {}
