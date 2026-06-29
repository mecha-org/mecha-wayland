pub mod hit;
pub mod keyboard;
pub mod pointer;
pub mod touch;

pub use keyboard::{KeyEvent, KeyboardState, Modifiers};
pub use pointer::{PointerEvent, PointerState};
pub use touch::{DragState, SwipeDirection, TouchEvent, TouchState};

#[derive(Debug, Default)]
pub struct InteractivityState {
    pub pointer: PointerState,
    pub keyboard: KeyboardState,
    pub touch: TouchState,
}

impl InteractivityState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_clicked(&self, bounds: utils::Rect) -> bool {
        self.pointer.is_clicked(bounds)
    }
}
