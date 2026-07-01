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

            WlPointerEvent::Button { state, .. } => match state {
                WlPointerButtonState::Pressed => {
                    self.press = Some((self.x, self.y));
                }
                WlPointerButtonState::Released => (),
                _ => (),
            },

            WlPointerEvent::Axis { .. } => (),

            WlPointerEvent::Frame { .. } => (),

            _ => (),
        }
    }
}
