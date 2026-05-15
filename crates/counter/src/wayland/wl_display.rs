use app::event::Event;

use crate::wayland::{SharedConnection, WaylandRawEvent};
use crate::wire::MessageReader;

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
}

impl WlDisplay {
    pub const OBJECT_ID: u32 = 1;

    pub fn new(conn: SharedConnection) -> Self {
        Self { conn }
    }

    // opcode 0: sync(callback: new_id)
    pub fn sync(&self, callback_id: u32) {
        self.conn
            .borrow_mut()
            .message_builder(Self::OBJECT_ID, 0)
            .write_u32(callback_id)
            .build();
    }

    // opcode 1: get_registry(registry: new_id)
    pub fn get_registry(&self, registry_id: u32) {
        self.conn
            .borrow_mut()
            .message_builder(Self::OBJECT_ID, 1)
            .write_u32(registry_id)
            .build();
    }

    pub fn process(&mut self, ev: &WaylandRawEvent) -> Option<DisplayEvent> {
        if ev.sender_id != Self::OBJECT_ID {
            return None;
        }
        let mut fds = vec![];
        let mut r = MessageReader::new(&ev.body, &mut fds);
        let event = match ev.opcode {
            0 => {
                let object_id = r.read_u32().unwrap_or(0);
                let code = r.read_u32().unwrap_or(0);
                let message = r.read_string().unwrap_or("unknown").to_string();
                DisplayEvent::Error {
                    object_id,
                    code,
                    message,
                }
            }
            1 => {
                let id = r.read_u32().unwrap_or(0);
                DisplayEvent::DeleteId { id }
            }
            _ => return None,
        };
        println!("[wl_display] {:?}", event);
        Some(event)
    }

    pub fn handle_event(&mut self, sender_id: u32, opcode: u16, body: &[u8]) {
        if sender_id != Self::OBJECT_ID {
            return;
        }
        let mut fds = vec![];
        let mut r = MessageReader::new(body, &mut fds);
        match opcode {
            0 => {
                let _obj = r.read_u32();
                let code = r.read_u32().unwrap_or(0);
                let msg = r.read_string().unwrap_or("unknown");
                eprintln!("[wl_display] error code={} msg={}", code, msg);
            }
            1 => {
                let id = r.read_u32().unwrap_or(0);
                println!("[wl_display] delete_id {}", id);
            }
            _ => {}
        }
    }
}

#[macro_export]
macro_rules! register_wl_display {
    () => {
        app::module::Module::<crate::wayland::WlDisplay>::new().processor(
            |d: &mut crate::wayland::WlDisplay, ev: &crate::wayland::WaylandRawEvent| d.process(ev),
        )
    };
}
