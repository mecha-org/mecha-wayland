use crate::object::{WlObject, WlObjectHandle, WlObjectProxy};
use io_runtime::channel::{Receiver, Sender, unbounded};

pub mod wl_callback_proto {
    pub const INTERFACE: &str = "wl_callback";
    pub const VERSION: u32 = 1;
    pub mod event {
        pub const DONE: u16 = 0;
    }
}

#[derive(Debug)]
pub enum WlCallbackEvent {
    Done { callback_data: u32 },
}

pub struct WlCallback {
    object_id: u32,
    tx: Sender<WlCallbackEvent>,
}

impl WlObjectHandle for WlCallback {
    fn dispatch(&self, opcode: u16, body: &[u8]) -> std::io::Result<()> {
        match opcode {
            wl_callback_proto::event::DONE => {
                tracing::trace!(opcode = wl_callback_proto::event::DONE, "wl_callback.done");
                let callback_data = crate::wire::read_u32(body, 0);
                self.tx
                    .send(WlCallbackEvent::Done { callback_data })
                    .unwrap();
            }
            _ => {}
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl WlObject for WlCallback {
    type Proxy = WlCallbackProxy;

    fn object_id(&self) -> u32 {
        self.object_id
    }

    fn spawn(object_id: u32) -> (Self, Self::Proxy)
    where
        Self: Sized,
    {
        let (tx, rx) = unbounded::<WlCallbackEvent>();
        let callback = WlCallback { object_id, tx };
        let proxy = WlCallbackProxy { object_id, rx };
        (callback, proxy)
    }
}

pub struct WlCallbackProxy {
    object_id: u32,
    rx: Receiver<WlCallbackEvent>,
}

impl WlCallbackProxy {
    pub fn on_done(&self) -> Option<u32> {
        match self.rx.try_recv().ok() {
            Some(WlCallbackEvent::Done { callback_data }) => Some(callback_data),
            _ => None,
        }
    }
}

impl WlObjectProxy for WlCallbackProxy {
    fn object_id(&self) -> u32 {
        self.object_id
    }
}
