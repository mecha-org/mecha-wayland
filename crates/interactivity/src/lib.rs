pub mod hit;
pub mod keyboard;
pub mod pointer;

pub use keyboard::{KeyEvent, KeyboardState, Modifiers};
pub use pointer::{PointerEvent, PointerState};

#[derive(Debug, Default)]
pub struct InteractivityState {
    pub pointer: PointerState,
    pub keyboard: KeyboardState,
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
        .on(|s: &mut InteractivityState, ev: &wayland::PointerEvent| s.pointer.process(ev))
        .on(|s: &mut InteractivityState, ev: &wayland::KeyboardEvent| s.keyboard.process(ev))
}
