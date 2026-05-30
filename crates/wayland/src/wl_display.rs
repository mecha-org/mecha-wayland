use app::Event;

use crate::proto::wl_display as proto;
use crate::proto::{Handle, WaylandParse};
use crate::{SharedConnection, WaylandRawEvent, parse, send};

#[derive(Debug)]
pub enum DisplayEvent {
    Error {
        object_id: u32,
        code: u32,
        message: String,
    },
    DeleteId {
        id: u32,
    },
}

impl Event for DisplayEvent {}

pub struct WlDisplay {
    conn: SharedConnection,
    handle: Handle<proto::WlDisplay>,
}

impl WlDisplay {
    pub fn new(conn: SharedConnection) -> Self {
        Self {
            conn,
            handle: Handle::new(1),
        }
    }

    pub fn sync(&self, callback_id: u32) {
        send(
            &self.conn,
            &self.handle,
            &proto::request::Sync {
                callback: callback_id,
            },
        );
    }

    pub fn get_registry(&self, registry_id: u32) {
        send(
            &self.conn,
            &self.handle,
            &proto::request::GetRegistry {
                registry: registry_id,
            },
        );
    }

    pub fn process(&mut self, raw: &WaylandRawEvent) -> Option<DisplayEvent> {
        if raw.sender_id != self.handle.id {
            return None;
        }
        let ev = if let Some(e) = parse::<proto::event::Error>(raw) {
            DisplayEvent::Error {
                object_id: e.object_id,
                code: e.code,
                message: e.message,
            }
        } else if let Some(e) = parse::<proto::event::DeleteId>(raw) {
            DisplayEvent::DeleteId { id: e.id }
        } else {
            return None;
        };
        println!("[wl_display] {:?}", ev);
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
        if let Some(e) = parse::<proto::event::Error>(&raw) {
            eprintln!("[wl_display] error code={} msg={}", e.code, e.message);
        } else if let Some(e) = parse::<proto::event::DeleteId>(&raw) {
            println!("[wl_display] delete_id {}", e.id);
        }
    }
}

pub fn module<AppState>() -> impl app::RegisteredModule<WlDisplay, AppState> {
    app::Module::<WlDisplay, _, _>::new()
        .on(|d: &mut WlDisplay, ev: &crate::WaylandRawEvent| d.process(ev))
}
