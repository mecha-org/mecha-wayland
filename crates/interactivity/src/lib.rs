pub mod gesture;
pub mod hit;
pub mod keyboard;
pub mod pointer;
pub mod touch;

pub use gesture::{DragState, SwipeDirection};
pub use keyboard::{KeyboardState, Modifiers};
pub use pointer::PointerState;
pub use touch::TouchState;

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
        self.pointer
            .just_pressed_buttons()
            .values()
            .any(|&(x, y)| bounds.contains(x, y))
    }
}
