use std::collections::HashMap;

use app::event::Event;

use crate::proto::Handle;
use crate::proto::wl_registry as proto;
use crate::{SharedConnection, WaylandRawEvent, parse, send};

#[derive(Debug)]
pub enum RegistryEvent {
    Global {
        name: u32,
        interface: String,
        version: u32,
    },
    GlobalRemove {
        name: u32,
    },
}

impl Event for RegistryEvent {}

pub struct WlRegistry {
    conn: SharedConnection,
    pub handle: Handle<proto::WlRegistry>,
    globals: HashMap<u32, (String, u32)>,
}

impl WlRegistry {
    pub fn new(conn: SharedConnection) -> Self {
        Self {
            conn,
            handle: Handle::new(0),
            globals: HashMap::new(),
        }
    }

    pub fn set_id(&mut self, id: u32) {
        self.handle = Handle::new(id);
    }

    pub fn find(&self, interface: &str) -> Option<(u32, u32)> {
        self.globals
            .iter()
            .find(|(_, (i, _))| i == interface)
            .map(|(name, (_, ver))| (*name, *ver))
    }

    pub fn bind(&self, name: u32, interface: &str, version: u32, new_id: u32) {
        send(
            &self.conn,
            &self.handle,
            &proto::request::Bind {
                name,
                interface: interface.to_string(),
                version,
                id: new_id,
            },
        );
    }

    pub fn process(&mut self, raw: &WaylandRawEvent) -> Option<RegistryEvent> {
        if raw.sender_id != self.handle.id {
            return None;
        }
        let ev = if let Some(e) = parse::<proto::event::Global>(raw) {
            self.globals
                .insert(e.name, (e.interface.clone(), e.version));
            RegistryEvent::Global {
                name: e.name,
                interface: e.interface,
                version: e.version,
            }
        } else if let Some(e) = parse::<proto::event::GlobalRemove>(raw) {
            self.globals.remove(&e.name);
            RegistryEvent::GlobalRemove { name: e.name }
        } else {
            return None;
        };
        println!("[wl_registry] {:?}", ev);
        Some(ev)
    }

    pub fn handle_event_sync(&mut self, sender_id: u32, opcode: u16, body: &[u8]) {
        if sender_id != self.handle.id {
            return;
        }
        let raw = WaylandRawEvent {
            sender_id,
            opcode,
            body: body.to_vec(),
        };
        if let Some(e) = parse::<proto::event::Global>(&raw) {
            println!(
                "[wl_registry] global {} {} v{}",
                e.name, e.interface, e.version
            );
            self.globals.insert(e.name, (e.interface, e.version));
        } else if let Some(e) = parse::<proto::event::GlobalRemove>(&raw) {
            self.globals.remove(&e.name);
        }
    }
}

#[macro_export]
macro_rules! register_wl_registry {
    () => {
        app::module::Module::<$crate::WlRegistry>::new()
            .processor(|r: &mut $crate::WlRegistry, ev: &$crate::WaylandRawEvent| r.process(ev))
    };
}
