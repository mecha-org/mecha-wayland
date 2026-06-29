use wayland::{ObjectId, WlPointerAxis};

#[derive(Clone, Debug)]
pub enum PointerEvent {
    Enter { surface: ObjectId, x: f64, y: f64 },
    Leave { surface: ObjectId, x: f64, y: f64 },
    Move { x: f64, y: f64, dx: f64, dy: f64, time: u32 },
    ButtonPress { button: u32, x: f64, y: f64, time: u32 },
    ButtonRelease { button: u32, x: f64, y: f64, time: u32 },
    Scroll { axis: WlPointerAxis, delta: f64, time: u32 },
    Frame,
}

impl app::Event for PointerEvent {}
