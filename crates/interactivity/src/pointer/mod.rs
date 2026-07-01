use std::collections::HashMap;

use utils::Rect;
use wayland::{WlPointerButtonState, WlPointerEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Side,    // BTN_SIDE
    Extra,   // BTN_EXTRA
    Forward, // BTN_FORWARD
    Back,    // BTN_BACK
    Task,    // BTN_TASK

    // TODO              Verify extras
    Numbered(u8),    // BTN_0..BTN_9   -> 0..=9
    ExtraButton(u8), // BTN_TRIGGER_HAPPY1..40 -> 0..=39
    Unknown(u32),    // anything else — don't drop the event, just pass the raw code through
}

impl From<u32> for MouseButton {
    fn from(code: u32) -> Self {
        match code {
            0x110 => MouseButton::Left,
            0x111 => MouseButton::Right,
            0x112 => MouseButton::Middle,
            0x113 => MouseButton::Side,
            0x114 => MouseButton::Extra,
            0x115 => MouseButton::Forward,
            0x116 => MouseButton::Back,
            0x117 => MouseButton::Task,
            0x100..=0x109 => MouseButton::Numbered((code - 0x100) as u8),
            0x2c0..=0x2e7 => MouseButton::ExtraButton((code - 0x2c0) as u8),
            other => MouseButton::Unknown(other),
        }
    }
}

#[derive(Debug, Default)]
pub struct PointerState {
    pub x: f64,
    pub y: f64,
    pub press: Option<(f64, f64)>,
    pub pressed_buttons: HashMap<MouseButton, (f64, f64)>,
    pub just_pressed_buttons: HashMap<MouseButton, (f64, f64)>,
    pub just_released_buttons: HashMap<MouseButton, (f64, f64)>,
}

impl PointerState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_clicked(&self, bounds: Rect) -> bool {
        self.press.map_or(false, |(x, y)| bounds.contains(x, y))
    }

    pub fn clear_press(&mut self) {
        self.press = None;
        self.just_pressed_buttons.clear();
        self.just_released_buttons.clear();
    }

    pub fn just_pressed(&self, button: MouseButton) -> bool {
        self.just_pressed_buttons.contains_key(&button)
    }

    pub fn just_released(&self, button: MouseButton) -> bool {
        self.just_released_buttons.contains_key(&button)
    }

    pub fn pressed(&self, button: MouseButton) -> bool {
        self.pressed_buttons.contains_key(&button)
    }

    pub fn process(&mut self, ev: &WlPointerEvent) {
        match ev {
            WlPointerEvent::Enter {
                surface_x,
                surface_y,
                ..
            } => {
                self.x = *surface_x as f64 / 256.0;
                self.y = *surface_y as f64 / 256.0;
            }

            WlPointerEvent::Leave { .. } => (),

            WlPointerEvent::Motion {
                surface_x,
                surface_y,
                ..
            } => {
                let x = *surface_x as f64 / 256.0;
                let y = *surface_y as f64 / 256.0;
                let dx = x - self.x;
                let dy = y - self.y;
                self.x = x;
                self.y = y;
            }

            WlPointerEvent::Button { state, button, .. } => {
                let button = MouseButton::from(*button);
                match state {
                    WlPointerButtonState::Pressed => {
                        self.press = Some((self.x, self.y));
                        self.pressed_buttons.insert(button, (self.x, self.y));
                        self.just_pressed_buttons.insert(button, (self.x, self.y));
                    }
                    WlPointerButtonState::Released => {
                        self.pressed_buttons.remove(&button);
                        self.just_released_buttons.insert(button, (self.x, self.y));
                    }
                    _ => (),
                }
            }

            WlPointerEvent::Axis { .. } => (),

            WlPointerEvent::Frame { .. } => (),

            _ => (),
        }
    }
}
