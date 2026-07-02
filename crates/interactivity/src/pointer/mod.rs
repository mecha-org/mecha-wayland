use std::collections::HashMap;
use wayland::{WlPointerAxis, WlPointerButtonState, WlPointerEvent};

use crate::gesture::GestureSingle;

/// Linux mouse button codes (`BTN_*`).
///
/// See:
/// <https://github.com/torvalds/linux/blob/master/include/uapi/linux/input-event-codes.h>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Side,
    Extra,
    Forward,
    Back,
    Task,
    /// Linux `BTN_0..BTN_9`.
    Numbered(u8),
    /// Linux `BTN_TRIGGER_HAPPY1..40`.
    ExtraButton(u8),
    /// Anything else — don't drop the event, just pass the raw code through.
    Unknown(u32),
}

#[derive(Debug, Default)]
pub struct ScrollData {
    /// Horizontal scroll delta, positive = right.
    pub dx: f64,
    /// Vertical scroll delta, positive = down.
    pub dy: f64,
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
            0x2c0..=0x2e7 => MouseButton::ExtraButton((code - 0x2c0 + 1) as u8),
            other => MouseButton::Unknown(other),
        }
    }
}

#[derive(Debug, Default)]
pub struct PointerState {
    x: f64,
    y: f64,
    last_press_position: Option<(f64, f64)>,
    pressed_buttons: HashMap<MouseButton, (f64, f64)>,
    just_pressed_buttons: HashMap<MouseButton, (f64, f64)>,
    just_released_buttons: HashMap<MouseButton, (f64, f64)>,
    just_scrolled: Option<ScrollData>,
    pub gesture_single: GestureSingle,
}

impl PointerState {
    pub fn new() -> Self {
        Self::default()
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

            WlPointerEvent::Leave { .. } => {
                self.gesture_single.on_source_cancel();
            }

            WlPointerEvent::Motion {
                surface_x,
                surface_y,
                time,
                ..
            } => {
                self.x = *surface_x as f64 / 256.0;
                self.y = *surface_y as f64 / 256.0;
                if self.pressed(MouseButton::Left) {
                    self.gesture_single.on_source_update(self.x, self.y, *time);
                } else {
                    self.gesture_single.clear();
                }
            }

            WlPointerEvent::Button {
                state,
                button,
                time,
                ..
            } => {
                let button = MouseButton::from(*button);
                match state {
                    WlPointerButtonState::Pressed => {
                        self.last_press_position = Some((self.x, self.y));
                        self.pressed_buttons.insert(button, (self.x, self.y));
                        self.just_pressed_buttons.insert(button, (self.x, self.y));
                        if button == MouseButton::Left {
                            self.gesture_single.on_source_down(self.x, self.y, *time);
                        }
                    }
                    WlPointerButtonState::Released => {
                        self.pressed_buttons.remove(&button);
                        self.just_released_buttons.insert(button, (self.x, self.y));
                        if button == MouseButton::Left {
                            self.gesture_single.on_source_up(*time);
                        }
                    }
                }
            }

            WlPointerEvent::Axis { axis, value, .. } => {
                let data = self.just_scrolled.get_or_insert_with(ScrollData::default);
                let delta = *value as f64 / 256.0;
                match axis {
                    WlPointerAxis::VerticalScroll => data.dy += delta,
                    WlPointerAxis::HorizontalScroll => data.dx += delta,
                }
            }

            WlPointerEvent::Frame { .. } => {}

            _ => (),
        }
    }

    pub fn clear(&mut self) {
        self.last_press_position = None;
        self.just_pressed_buttons.clear();
        self.just_released_buttons.clear();
        self.just_scrolled = None;
    }

    /// Returns the current pointer position.
    pub fn position(&self) -> (f64, f64) {
        (self.x, self.y)
    }

    /// Returns the scroll event for this frame, if any.
    pub fn just_scrolled(&self) -> Option<&ScrollData> {
        self.just_scrolled.as_ref()
    }

    // -----------------------------------------------------------------------------
    // Just Pressed
    // -----------------------------------------------------------------------------

    /// Returns all buttons that were pressed this frame.
    pub fn just_pressed_buttons(&self) -> &HashMap<MouseButton, (f64, f64)> {
        &self.just_pressed_buttons
    }

    /// Returns true if `button` was pressed this frame.
    pub fn just_pressed(&self, button: MouseButton) -> bool {
        self.just_pressed_buttons.contains_key(&button)
    }

    /// Returns the position where `button` was pressed this frame.
    pub fn just_pressed_position(&self, button: MouseButton) -> Option<(f64, f64)> {
        self.just_pressed_buttons.get(&button).copied()
    }

    // -----------------------------------------------------------------------------
    // Pressed
    // -----------------------------------------------------------------------------

    /// Returns all buttons that are currently held down.
    pub fn pressed_buttons(&self) -> &HashMap<MouseButton, (f64, f64)> {
        &self.pressed_buttons
    }

    /// Returns true if `button` is currently held down.
    pub fn pressed(&self, button: MouseButton) -> bool {
        self.pressed_buttons.contains_key(&button)
    }

    /// Returns the position where `button` was pressed.
    pub fn pressed_position(&self, button: MouseButton) -> Option<(f64, f64)> {
        self.pressed_buttons.get(&button).copied()
    }

    // -----------------------------------------------------------------------------
    // Just Released
    // -----------------------------------------------------------------------------

    /// Returns all buttons that were released this frame.
    pub fn just_released_buttons(&self) -> &HashMap<MouseButton, (f64, f64)> {
        &self.just_released_buttons
    }

    /// Returns true if `button` was released this frame.
    pub fn just_released(&self, button: MouseButton) -> bool {
        self.just_released_buttons.contains_key(&button)
    }

    /// Returns the position where `button` was released this frame.
    pub fn just_released_position(&self, button: MouseButton) -> Option<(f64, f64)> {
        self.just_released_buttons.get(&button).copied()
    }
}
