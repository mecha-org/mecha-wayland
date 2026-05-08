use io_runtime::channel::{Receiver, Sender, unbounded};

use crate::{
    object::{WlObjectHandle, WlObjectProxy},
    wl_callback::WlCallbackProxy,
};

pub mod wl_display_proto {
    pub const INTERFACE: &str = "wl_display";
    pub const VERSION: u32 = 1;
    pub mod request {
        pub const SYNC: u16 = 0;
        pub const GET_REGISTRY: u16 = 1;
    }
    pub mod event {
        pub const ERROR: u16 = 0;
        pub const DELETE_ID: u16 = 1;
    }
}

#[derive(Debug)]
pub enum WlDisplayEvent {
    Error {
        object_id: u32,
        code: u32,
        message: String,
    },
    DeleteId {
        id: u32,
    },
}

pub struct WlDisplay {
    object_id: u32,
    tx: Sender<WlDisplayEvent>,
}

impl WlObjectHandle for WlDisplay {
    fn dispatch(&self, opcode: u16, body: &[u8]) -> std::io::Result<()> {
        match opcode {
            wl_display_proto::event::ERROR => {
                tracing::trace!(opcode = wl_display_proto::event::ERROR, "wl_display.error");
                let object_id = crate::wire::read_u32(body, 0);
                let code = crate::wire::read_u32(body, 4);
                let (message, _off2) = crate::wire::read_string(body, 8);
                self.tx
                    .send(WlDisplayEvent::Error {
                        object_id,
                        code,
                        message,
                    })
                    .unwrap();
            }
            wl_display_proto::event::DELETE_ID => {
                tracing::trace!(
                    opcode = wl_display_proto::event::DELETE_ID,
                    "wl_display.delete_id"
                );
                let id = crate::wire::read_u32(body, 0);
                self.tx.send(WlDisplayEvent::DeleteId { id }).unwrap();
            }
            _ => {}
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::object::WlObject for WlDisplay {
    type Proxy = WlDisplayProxy;

    fn object_id(&self) -> u32 {
        self.object_id
    }

    fn spawn(object_id: u32) -> (Self, Self::Proxy)
    where
        Self: Sized,
    {
        let (tx, rx) = unbounded::<WlDisplayEvent>();
        let display = WlDisplay { object_id, tx };
        let proxy = WlDisplayProxy { object_id, rx };
        (display, proxy)
    }
}

impl WlDisplay {}

pub struct WlDisplayProxy {
    object_id: u32,
    rx: Receiver<WlDisplayEvent>,
}

impl WlObjectProxy for WlDisplayProxy {
    fn object_id(&self) -> u32 {
        self.object_id
    }
}

impl WlDisplayProxy {
    fn on_error(&mut self) -> Option<(u32, u32, String)> {
        match self.rx.try_recv().ok() {
            Some(WlDisplayEvent::Error {
                object_id,
                code,
                message,
            }) => Some((object_id, code, message)),
            _ => None,
        }
    }

    fn on_delete_id(&mut self) -> Option<u32> {
        match self.rx.try_recv().ok() {
            Some(WlDisplayEvent::DeleteId { id }) => Some(id),
            _ => None,
        }
    }

    pub fn sync(
        &self,
        conn: &mut crate::connection::Connection,
        io: &mut io_runtime::ring::Ring,
        callback: &WlCallbackProxy,
    ) -> std::io::Result<()> {
        tracing::debug!(
            object_id = self.object_id(),
            opcode = wl_display_proto::request::SYNC,
            "wl_display.sync"
        );
        let mut args = Vec::new();
        crate::wire::write_u32(&mut args, callback.object_id());
        conn.send(io, self.object_id(), wl_display_proto::request::SYNC, &args)
    }

    pub fn get_registry(
        &self,
        conn: &mut crate::connection::Connection,
        io: &mut io_runtime::ring::Ring,
        registry_object_id: u32,
    ) -> std::io::Result<()> {
        tracing::debug!(
            object_id = self.object_id(),
            opcode = wl_display_proto::request::GET_REGISTRY,
            "wl_display.get_registry"
        );
        let mut args = Vec::new();
        crate::wire::write_u32(&mut args, registry_object_id);
        conn.send(
            io,
            self.object_id(),
            wl_display_proto::request::GET_REGISTRY,
            &args,
        )
    }
}
