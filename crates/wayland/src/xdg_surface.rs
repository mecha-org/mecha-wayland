use crate::object::Object;
use crate::{XdgSurface, XdgSurfaceConfigureEvent, XdgSurfaceHandler};

pub struct XdgSurf {
    pub inner: XdgSurface,
    pub pending_ack: Option<u32>,
    pub configured: bool,
}

impl XdgSurf {
    pub fn new(inner: XdgSurface) -> Self {
        XdgSurf {
            inner,
            pending_ack: None,
            configured: false,
        }
    }
}

impl Object for XdgSurf {
    fn object_id(&self) -> u32 {
        self.inner.object_id()
    }
}

impl XdgSurfaceHandler for XdgSurf {
    fn on_configure(&mut self, event: XdgSurfaceConfigureEvent) {
        tracing::debug!(serial = event.serial, "xdg_surface::configure");
        self.pending_ack = Some(event.serial);
        self.configured = true;
    }
}
