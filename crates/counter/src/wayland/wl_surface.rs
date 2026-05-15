use std::collections::HashMap;

use app::event::Event;

use crate::wayland::{SharedConnection, WaylandRawEvent};
use crate::wire::MessageReader;

#[derive(Debug)]
pub enum SurfaceEvent {
    Enter { id: u32, output: u32 },
    Leave { id: u32, output: u32 },
}

impl Event for SurfaceEvent {}

pub struct SurfaceState;

pub struct WlSurface {
    conn: SharedConnection,
    surfaces: HashMap<u32, SurfaceState>,
}

impl WlSurface {
    pub fn new(conn: SharedConnection) -> Self {
        Self {
            conn,
            surfaces: HashMap::new(),
        }
    }

    pub fn register(&mut self, id: u32) {
        self.surfaces.insert(id, SurfaceState);
    }

    // opcode 1: attach(buffer: object, x: int, y: int)
    pub fn attach(&self, id: u32, buffer_id: u32, x: i32, y: i32) {
        self.conn
            .borrow_mut()
            .message_builder(id, 1)
            .write_u32(buffer_id)
            .write_i32(x)
            .write_i32(y)
            .build();
    }

    // opcode 2: damage(x: int, y: int, width: int, height: int)
    pub fn damage(&self, id: u32, x: i32, y: i32, width: i32, height: i32) {
        self.conn
            .borrow_mut()
            .message_builder(id, 2)
            .write_i32(x)
            .write_i32(y)
            .write_i32(width)
            .write_i32(height)
            .build();
    }

    // opcode 6: commit()
    pub fn commit(&self, id: u32) {
        self.conn.borrow_mut().message_builder(id, 6).build();
    }

    pub fn process(&mut self, ev: &WaylandRawEvent) -> Option<SurfaceEvent> {
        if !self.surfaces.contains_key(&ev.sender_id) {
            return None;
        }
        let id = ev.sender_id;
        let mut fds = vec![];
        let mut r = MessageReader::new(&ev.body, &mut fds);
        let event = match ev.opcode {
            0 => {
                let output = r.read_u32().unwrap_or(0);
                SurfaceEvent::Enter { id, output }
            }
            1 => {
                let output = r.read_u32().unwrap_or(0);
                SurfaceEvent::Leave { id, output }
            }
            _ => return None,
        };
        println!("[wl_surface] {:?}", event);
        Some(event)
    }
}

#[macro_export]
macro_rules! register_wl_surface {
    () => {
        app::module::Module::<crate::wayland::WlSurface>::new().processor(
            |s: &mut crate::wayland::WlSurface, ev: &crate::wayland::WaylandRawEvent| s.process(ev),
        )
    };
}
