use std::collections::HashSet;

use app::event::Event;

use crate::proto::Handle;
use crate::proto::wl_callback as proto;
use crate::{SharedConnection, WaylandRawEvent, parse};

#[derive(Debug)]
pub enum WlCallbackEvent {
    Done { id: u32, callback_data: u32 },
}

impl Event for WlCallbackEvent {}

pub struct WlCallback {
    _conn: SharedConnection,
    pub handle: Handle<proto::WlCallback>,
    done: bool,
    frame_ids: HashSet<u32>,
}

impl WlCallback {
    pub fn new(conn: SharedConnection) -> Self {
        Self {
            _conn: conn,
            handle: Handle::new(0),
            done: false,
            frame_ids: HashSet::new(),
        }
    }

    pub fn set_id(&mut self, id: u32) {
        self.handle = Handle::new(id);
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn register_frame(&mut self, id: u32) {
        self.frame_ids.insert(id);
    }

    pub fn process(&mut self, raw: &WaylandRawEvent) -> Option<WlCallbackEvent> {
        if !self.frame_ids.remove(&raw.sender_id) {
            return None;
        }
        let e = parse::<proto::event::Done>(raw)?;
        Some(WlCallbackEvent::Done {
            id: raw.sender_id,
            callback_data: e.callback_data,
        })
    }

    pub fn handle_event_sync(&mut self, sender_id: u32, opcode: u16, body: &[u8]) {
        if sender_id == self.handle.id {
            let raw = WaylandRawEvent {
                sender_id,
                opcode,
                body: body.to_vec(),
            };
            if parse::<proto::event::Done>(&raw).is_some() {
                self.done = true;
            }
        }
    }
}

#[macro_export]
macro_rules! register_wl_callback {
    () => {
        app::module::Module::<$crate::WlCallback>::new()
            .processor(|c: &mut $crate::WlCallback, ev: &$crate::WaylandRawEvent| c.process(ev))
    };
}
