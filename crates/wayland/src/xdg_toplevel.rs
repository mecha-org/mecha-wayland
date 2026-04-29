use crate::object::Object;
use crate::{
    XdgToplevel, XdgToplevelConfigureBoundsEvent, XdgToplevelConfigureEvent, XdgToplevelHandler,
    XdgToplevelWmCapabilitiesEvent,
};

pub struct Toplevel {
    pub inner: XdgToplevel,
    pub closed: bool,
    pub width: i32,
    pub height: i32,
}

impl Toplevel {
    pub fn new(inner: XdgToplevel) -> Self {
        Toplevel {
            inner,
            closed: false,
            width: 0,
            height: 0,
        }
    }
}

impl Object for Toplevel {
    fn object_id(&self) -> u32 {
        self.inner.object_id()
    }
}

impl XdgToplevelHandler for Toplevel {
    fn on_configure(&mut self, event: XdgToplevelConfigureEvent) {
        tracing::debug!(
            width = event.width,
            height = event.height,
            "xdg_toplevel::configure"
        );
        if event.width != 0 {
            self.width = event.width;
        }
        if event.height != 0 {
            self.height = event.height;
        }
    }

    fn on_close(&mut self) {
        tracing::info!("xdg_toplevel::close");
        self.closed = true;
    }

    fn on_configure_bounds(&mut self, _event: XdgToplevelConfigureBoundsEvent) {}

    fn on_wm_capabilities(&mut self, _event: XdgToplevelWmCapabilitiesEvent) {}
}
