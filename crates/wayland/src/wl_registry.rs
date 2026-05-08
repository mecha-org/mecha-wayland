use io_runtime::channel::{Receiver, Sender, unbounded};

use crate::object::{WlObject, WlObjectHandle, WlObjectProxy};

pub mod wl_registry_proto {
    pub const INTERFACE: &str = "wl_registry";
    pub const VERSION: u32 = 1;
    pub mod request {
        pub const BIND: u16 = 0;
    }
    pub mod event {
        pub const GLOBAL: u16 = 0;
        pub const GLOBAL_REMOVE: u16 = 1;
    }
}

#[derive(Debug)]
pub enum WlRegistryEvent {
    Global {
        name: u32,
        interface: String,
        version: u32,
    },
    GlobalRemove {
        name: u32,
    },
}

pub struct WlRegistry {
    object_id: u32,
    tx: Sender<WlRegistryEvent>,
}

impl WlRegistry {}

impl WlObjectHandle for WlRegistry {
    fn dispatch(&self, opcode: u16, body: &[u8]) -> std::io::Result<()> {
        match opcode {
            wl_registry_proto::event::GLOBAL => {
                tracing::trace!(
                    opcode = wl_registry_proto::event::GLOBAL,
                    "wl_registry.global"
                );
                let name = crate::wire::read_u32(body, 0);
                let (interface, _off1) = crate::wire::read_string(body, 4);
                let version = crate::wire::read_u32(body, _off1);
                let _off2 = _off1 + 4;

                self.tx
                    .send(WlRegistryEvent::Global {
                        name,
                        interface,
                        version,
                    })
                    .unwrap();
            }
            wl_registry_proto::event::GLOBAL_REMOVE => {
                tracing::trace!(
                    opcode = wl_registry_proto::event::GLOBAL_REMOVE,
                    "wl_registry.global_remove"
                );
                let name = crate::wire::read_u32(body, 0);
                self.tx
                    .send(WlRegistryEvent::GlobalRemove { name })
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

impl crate::object::WlObject for WlRegistry {
    type Proxy = WlRegistryProxy;

    fn object_id(&self) -> u32 {
        self.object_id
    }

    fn spawn(object_id: u32) -> (Self, Self::Proxy)
    where
        Self: Sized,
    {
        let (tx, rx) = unbounded::<WlRegistryEvent>();
        let registry = WlRegistry { object_id, tx };
        let proxy = WlRegistryProxy { object_id, rx };
        (registry, proxy)
    }
}

pub struct WlRegistryProxy {
    object_id: u32,
    rx: Receiver<WlRegistryEvent>,
}

impl WlObjectProxy for WlRegistryProxy {
    fn object_id(&self) -> u32 {
        self.object_id
    }
}

impl WlRegistryProxy {
    fn on_global(&mut self) -> Option<(u32, String, u32)> {
        match self.rx.try_recv().ok() {
            Some(WlRegistryEvent::Global {
                name,
                interface,
                version,
            }) => Some((name, interface, version)),
            _ => None,
        }
    }

    fn on_global_remove(&mut self) -> Option<u32> {
        match self.rx.try_recv().ok() {
            Some(WlRegistryEvent::GlobalRemove { name }) => Some(name),
            _ => None,
        }
    }

    pub fn bind(
        &self,
        conn: &mut crate::connection::Connection,
        io: &mut io_runtime::ring::Ring,
        name: u32,
        interface: &str,
        version: u32,
        id: impl crate::object::WlObjectProxy,
    ) -> std::io::Result<()> {
        tracing::debug!(
            object_id = self.object_id(),
            opcode = wl_registry_proto::request::BIND,
            name,
            interface,
            version,
            "wl_registry.bind",
        );
        let mut args = Vec::new();
        crate::wire::write_u32(&mut args, name);
        crate::wire::write_string(&mut args, interface);
        crate::wire::write_u32(&mut args, version);
        crate::wire::write_u32(&mut args, id.object_id());
        conn.send(
            io,
            self.object_id(),
            wl_registry_proto::request::BIND,
            &args,
        )
    }
}
