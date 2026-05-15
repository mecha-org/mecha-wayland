use std::collections::HashMap;

use app::event::Event;

use crate::wayland::{SharedConnection, WaylandRawEvent};
use crate::wire::MessageReader;

#[derive(Debug)]
pub enum RegistryEvent {
    Global { name: u32, interface: String, version: u32 },
    GlobalRemove { name: u32 },
}

impl Event for RegistryEvent {}

pub struct WlRegistry {
    conn: SharedConnection,
    id: u32,
    globals: HashMap<u32, (String, u32)>,
}

impl WlRegistry {
    pub fn new(conn: SharedConnection) -> Self {
        Self { conn, id: 0, globals: HashMap::new() }
    }

    pub fn set_id(&mut self, id: u32) {
        self.id = id;
    }

    pub fn find(&self, interface: &str) -> Option<(u32, u32)> {
        self.globals
            .iter()
            .find(|(_, (i, _))| i == interface)
            .map(|(name, (_, ver))| (*name, *ver))
    }

    // opcode 0: bind(name: uint, interface: string, version: uint, id: new_id)
    pub fn bind(&self, name: u32, interface: &str, version: u32, new_id: u32) {
        self.conn
            .borrow_mut()
            .message_builder(self.id, 0)
            .write_u32(name)
            .write_string(interface)
            .write_u32(version)
            .write_u32(new_id)
            .build();
    }

    pub fn process(&mut self, ev: &WaylandRawEvent) -> Option<RegistryEvent> {
        if ev.sender_id != self.id {
            return None;
        }
        let mut fds = vec![];
        let mut r = MessageReader::new(&ev.body, &mut fds);
        let event = match ev.opcode {
            0 => {
                let name = r.read_u32().unwrap_or(0);
                let interface = r.read_string().unwrap_or("").to_string();
                let version = r.read_u32().unwrap_or(0);
                self.globals.insert(name, (interface.clone(), version));
                RegistryEvent::Global { name, interface, version }
            }
            1 => {
                let name = r.read_u32().unwrap_or(0);
                self.globals.remove(&name);
                RegistryEvent::GlobalRemove { name }
            }
            _ => return None,
        };
        println!("[wl_registry] {:?}", event);
        Some(event)
    }

    pub fn handle_event(&mut self, sender_id: u32, opcode: u16, body: &[u8]) {
        if sender_id != self.id {
            return;
        }
        let mut fds = vec![];
        let mut r = MessageReader::new(body, &mut fds);
        match opcode {
            0 => {
                let name = r.read_u32().unwrap_or(0);
                let interface = r.read_string().unwrap_or("").to_string();
                let version = r.read_u32().unwrap_or(0);
                println!("[wl_registry] global {} {} v{}", name, interface, version);
                self.globals.insert(name, (interface, version));
            }
            1 => {
                let name = r.read_u32().unwrap_or(0);
                self.globals.remove(&name);
            }
            _ => {}
        }
    }
}

#[macro_export]
macro_rules! register_wl_registry {
    () => {
        app::module::Module::<crate::wayland::WlRegistry>::new()
            .processor(|r: &mut crate::wayland::WlRegistry, ev: &crate::wayland::WaylandRawEvent| {
                r.process(ev)
            })
    };
}
