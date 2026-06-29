mod event;

pub use event::PointerEvent;

use utils::Rect;
use wayland::{WlPointerButtonState, WlPointerEvent};

#[derive(Debug, Default)]
pub struct PointerState {
    pub x: f64,
    pub y: f64,
    pub press: Option<(f64, f64)>,
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
    }

    pub fn process(&mut self, ev: &WlPointerEvent) -> Option<PointerEvent> {
        match ev {
            WlPointerEvent::Enter { surface, surface_x, surface_y, .. } => {
                self.x = *surface_x as f64 / 256.0;
                self.y = *surface_y as f64 / 256.0;
                Some(PointerEvent::Enter {
                    surface: surface.object_id()?,
                    x: self.x,
                    y: self.y,
                })
            }

            WlPointerEvent::Leave { surface, .. } => Some(PointerEvent::Leave {
                surface: surface.object_id()?,
                x: self.x,
                y: self.y,
            }),

            WlPointerEvent::Motion { time, surface_x, surface_y, .. } => {
                let x = *surface_x as f64 / 256.0;
                let y = *surface_y as f64 / 256.0;
                let dx = x - self.x;
                let dy = y - self.y;
                self.x = x;
                self.y = y;
                Some(PointerEvent::Move { x: self.x, y: self.y, dx, dy, time: *time })
            }

            WlPointerEvent::Button { button, state, time, .. } => match state {
                WlPointerButtonState::Pressed => {
                    self.press = Some((self.x, self.y));
                    Some(PointerEvent::ButtonPress {
                        button: *button,
                        x: self.x,
                        y: self.y,
                        time: *time,
                    })
                }
                WlPointerButtonState::Released => Some(PointerEvent::ButtonRelease {
                    button: *button,
                    x: self.x,
                    y: self.y,
                    time: *time,
                }),
                _ => None,
            },

            WlPointerEvent::Axis { time, axis, value, .. } => Some(PointerEvent::Scroll {
                axis: *axis,
                delta: *value as f64 / 256.0,
                time: *time,
            }),

            WlPointerEvent::Frame { .. } => Some(PointerEvent::Frame),

            _ => None,
        }
    }
}
