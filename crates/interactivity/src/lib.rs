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
}

pub fn module<AppState>() -> impl app::RegisteredModule<InteractivityState, AppState>
where
    AppState: app::Lens<InteractivityState>,
{
    app::Module::<InteractivityState, _, _>::new()
}
