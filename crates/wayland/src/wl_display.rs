use crate::object::Object;
use crate::{WlDisplay, WlDisplayDeleteIdEvent, WlDisplayErrorEvent, WlDisplayHandler};

pub struct Display {
    pub inner: WlDisplay,
}

impl Display {
    pub fn new(object_id: u32) -> Self {
        Display {
            inner: WlDisplay::new(object_id),
        }
    }
}

impl Object for Display {
    fn object_id(&self) -> u32 {
        self.inner.object_id()
    }
}

impl WlDisplayHandler for Display {
    fn on_error(&mut self, event: WlDisplayErrorEvent) {
        tracing::error!(
            object_id = event.object_id,
            code = event.code,
            message = %event.message,
            "wl_display error"
        );
    }

    fn on_delete_id(&mut self, event: WlDisplayDeleteIdEvent) {
        tracing::debug!(id = event.id, "wl_display::delete_id");
    }
}
