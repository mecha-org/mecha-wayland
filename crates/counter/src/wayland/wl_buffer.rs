use std::collections::HashSet;

use app::event::Event;

use crate::wayland::{SharedConnection, WaylandRawEvent};

#[derive(Debug)]
pub enum WlBufferEvent {
    Release { id: u32 },
}

impl Event for WlBufferEvent {}

pub struct WlBuffer {
    _conn: SharedConnection,
    ids: HashSet<u32>,
}

impl WlBuffer {
    pub fn new(conn: SharedConnection) -> Self {
        Self { _conn: conn, ids: HashSet::new() }
    }

    pub fn register(&mut self, id: u32) {
        self.ids.insert(id);
    }

    pub fn process(&mut self, ev: &WaylandRawEvent) -> Option<WlBufferEvent> {
        if !self.ids.contains(&ev.sender_id) || ev.opcode != 0 {
            return None;
        }
        Some(WlBufferEvent::Release { id: ev.sender_id })
    }
}

#[macro_export]
macro_rules! register_wl_buffer {
    () => {
        app::module::Module::<crate::wayland::WlBuffer>::new().processor(
            |b: &mut crate::wayland::WlBuffer, ev: &crate::wayland::WaylandRawEvent| b.process(ev),
        )
    };
}
