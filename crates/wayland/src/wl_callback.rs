use crate::object::Object;
use crate::{WlCallback, WlCallbackDoneEvent, WlCallbackHandler};

pub struct SyncCallback {
    inner: WlCallback,
    pub done: bool,
}

impl SyncCallback {
    pub fn new(object_id: u32) -> Self {
        SyncCallback {
            inner: WlCallback::new(object_id),
            done: false,
        }
    }
}

impl crate::object::Object for SyncCallback {
    fn object_id(&self) -> u32 {
        self.inner.object_id()
    }
}

impl WlCallbackHandler for SyncCallback {
    fn on_done(&mut self, _event: WlCallbackDoneEvent) {
        tracing::debug!(object_id = self.object_id(), "wl_callback::done");
        self.done = true;
    }
}
