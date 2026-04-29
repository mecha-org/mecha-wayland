use crate::object::Object;
use crate::{WlPointer, WlPointerHandler};

pub struct Pointer {
    pub inner: WlPointer,
    pub x: f64,
    pub y: f64,
}

impl Pointer {
    pub fn new(object_id: u32) -> Self {
        Pointer {
            inner: WlPointer::new(object_id),
            x: 0.0,
            y: 0.0,
        }
    }
}

impl Object for Pointer {
    fn object_id(&self) -> u32 {
        self.inner.object_id()
    }
}

impl WlPointerHandler for Pointer {
    fn on_enter(&mut self, event: crate::WlPointerEnterEvent) {
        self.x = event.surface_x as f64 / 256.0;
        self.y = event.surface_y as f64 / 256.0;
        tracing::debug!(x = self.x, y = self.y, "mouse entered window");
    }

    fn on_leave(&mut self, event: crate::WlPointerLeaveEvent) {
        tracing::debug!("mouse left window");
    }

    fn on_motion(&mut self, event: crate::WlPointerMotionEvent) {
        self.x = event.surface_x as f64 / 256.0;
        self.y = event.surface_y as f64 / 256.0;
        tracing::trace!(x = self.x, y = self.y, "mouse motion");
    }

    fn on_button(&mut self, event: crate::WlPointerButtonEvent) {
        tracing::debug!(
            button = event.button,
            state = event.state,
            x = self.x,
            y = self.y,
            "mouse button"
        );
    }

    fn on_axis(&mut self, event: crate::WlPointerAxisEvent) {
        tracing::trace!(axis = event.axis, value = event.value, "mouse wheel scroll");
    }
}
