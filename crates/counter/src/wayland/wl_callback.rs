use std::collections::HashSet;

use app::event::Event;

use crate::wayland::{SharedConnection, WaylandRawEvent};
use crate::wire::MessageReader;

#[derive(Debug)]
pub enum WlCallbackEvent {
    Done { id: u32, callback_data: u32 },
}

impl Event for WlCallbackEvent {}

pub struct WlCallback {
    _conn: SharedConnection,
    pub id: u32,
    done: bool,
    frame_ids: HashSet<u32>,
}

impl WlCallback {
    pub fn new(conn: SharedConnection) -> Self {
        Self { _conn: conn, id: 0, done: false, frame_ids: HashSet::new() }
    }

    pub fn set_id(&mut self, id: u32) {
        self.id = id;
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Register a wl_callback object id to listen for frame-done events.
    /// Each id fires once and is removed from the set automatically.
    pub fn register_frame(&mut self, id: u32) {
        self.frame_ids.insert(id);
    }

    pub fn process(&mut self, ev: &WaylandRawEvent) -> Option<WlCallbackEvent> {
        if ev.opcode != 0 || !self.frame_ids.remove(&ev.sender_id) {
            return None;
        }
        let mut fds = vec![];
        let mut r = MessageReader::new(&ev.body, &mut fds);
        let callback_data = r.read_u32().unwrap_or(0);
        Some(WlCallbackEvent::Done { id: ev.sender_id, callback_data })
    }

    /// Blocking-path event handler used only during init sync roundtrip.
    pub fn handle_event(&mut self, sender_id: u32, opcode: u16, _body: &[u8]) {
        if sender_id == self.id && opcode == 0 {
            self.done = true;
        }
    }
}

#[macro_export]
macro_rules! register_wl_callback {
    () => {
        app::module::Module::<crate::wayland::WlCallback>::new().processor(
            |c: &mut crate::wayland::WlCallback, ev: &crate::wayland::WaylandRawEvent| {
                c.process(ev)
            },
        )
    };
}
