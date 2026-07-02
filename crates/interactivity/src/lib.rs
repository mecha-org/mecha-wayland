pub mod gesture;
pub mod hit;
pub mod keyboard;
pub mod pointer;
pub mod touch;

pub use gesture::{DragState, GestureSingle, SwipeDirection};
pub use keyboard::{KeyCode, KeyboardState, Modifiers};
pub use pointer::PointerState;
pub use touch::TouchState;

use wayland::{WlKeyboardEvent, WlPointerEvent, WlTouchEvent};

#[derive(Debug, Default)]
pub struct InteractivityState {
    pub pointer: PointerState,
    pub keyboard: KeyboardState,
    pub touch: TouchState,
    pub gesture: GestureSingle,
}

impl InteractivityState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process_pointer(&mut self, ev: &WlPointerEvent) {
        self.pointer.process(ev, &mut self.gesture);
    }

    pub fn clear_pointer(&mut self) {
        self.pointer.clear(&mut self.gesture);
    }

    pub fn process_touch(&mut self, ev: &WlTouchEvent) {
        self.touch.process(ev, &mut self.gesture);
    }

    pub fn clear_touch(&mut self) {
        self.touch.clear(&mut self.gesture);
    }

    pub fn process_keyboard(&mut self, ev: &WlKeyboardEvent) {
        self.keyboard.process(ev);
    }

    pub fn clear_keyboard(&mut self) {
        self.keyboard.clear();
    }

    pub fn is_clicked(&self, bounds: utils::Rect) -> bool {
        self.pointer
            .just_pressed_buttons()
            .values()
            .any(|point| bounds.contains_point(*point))
            || self.touch.tapped(bounds)
    }
}
