use wayland::PointerAxis;

/// High-level pointer event emitted by the interactivity module.
#[derive(Clone, Debug)]
pub enum PointerEvent {
    /// Pointer entered the surface.
    Enter { surface: u32, x: f64, y: f64 },

    /// Pointer left the surface.
    Leave { surface: u32, x: f64, y: f64 },

    /// Pointer moved over the surface.
    Move {
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
        time: u32,
    },

    /// A mouse button was pressed.
    ButtonPress {
        button: u32,
        x: f64,
        y: f64,
        time: u32,
    },

    /// A mouse button was released.
    ButtonRelease {
        button: u32,
        x: f64,
        y: f64,
        time: u32,
    },

    /// Scroll-wheel or continuous-axis (trackpad) event.
    Scroll {
        axis: PointerAxis,
        delta: f64,
        time: u32,
    },

    /// End of a logical group of simultaneous pointer updates.
    Frame,
}

impl app::Event for PointerEvent {}
