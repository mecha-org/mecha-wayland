use crate::object::Object;
use crate::{XdgWmBase, XdgWmBaseHandler, XdgWmBasePingEvent};

pub struct WmBase {
    pub inner: XdgWmBase,
    pub pending_pong: Option<u32>,
}

impl WmBase {
    pub fn new(inner: XdgWmBase) -> Self {
        WmBase {
            inner,
            pending_pong: None,
        }
    }
}

impl Object for WmBase {
    fn object_id(&self) -> u32 {
        self.inner.object_id()
    }
}

impl XdgWmBaseHandler for WmBase {
    fn on_ping(&mut self, event: XdgWmBasePingEvent) {
        tracing::debug!(serial = event.serial, "xdg_wm_base::ping");
        self.pending_pong = Some(event.serial);
    }
}
