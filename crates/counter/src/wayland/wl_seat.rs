use app::event::Event;

use crate::wayland::{SharedConnection, WaylandRawEvent};
use crate::wire::MessageReader;

pub const CAP_POINTER: u32 = 1;
pub const CAP_KEYBOARD: u32 = 2;
pub const CAP_TOUCH: u32 = 4;

#[derive(Debug, Clone)]
pub enum SeatEvent {
    Capabilities { capabilities: u32 },
    Name { name: String },
}

impl Event for SeatEvent {}

pub struct WlSeat {
    conn: SharedConnection,
    pub id: u32,
    pub capabilities: u32,
}

impl WlSeat {
    pub fn new(conn: SharedConnection) -> Self {
        Self {
            conn,
            id: 0,
            capabilities: 0,
        }
    }

    pub fn set_id(&mut self, id: u32) {
        self.id = id;
    }

    pub fn get_pointer(&self) -> u32 {
        let mut conn = self.conn.borrow_mut();
        let pointer_id = conn.alloc_id();
        conn.message_builder(self.id, 0)
            .write_u32(pointer_id)
            .build();
        pointer_id
    }

    pub fn get_keyboard(&self) -> u32 {
        let mut conn = self.conn.borrow_mut();
        let keyboard_id = conn.alloc_id();
        conn.message_builder(self.id, 1)
            .write_u32(keyboard_id)
            .build();
        keyboard_id
    }

    pub fn get_touch(&self) -> u32 {
        let mut conn = self.conn.borrow_mut();
        let touch_id = conn.alloc_id();
        conn.message_builder(self.id, 2).write_u32(touch_id).build();
        touch_id
    }

    pub fn release(&self) {
        self.conn.borrow_mut().message_builder(self.id, 3).build();
    }

    pub fn process(&mut self, ev: &WaylandRawEvent) -> Option<SeatEvent> {
        if ev.sender_id != self.id {
            return None;
        }

        let mut fds = vec![];
        let mut r = MessageReader::new(&ev.body, &mut fds);

        let event = match ev.opcode {
            0 => {
                let capabilities = r.read_u32().unwrap_or(0);
                self.capabilities = capabilities;
                SeatEvent::Capabilities { capabilities }
            }
            1 => {
                let name = r.read_string().unwrap_or_default().to_string();
                SeatEvent::Name { name }
            }
            _ => return None,
        };

        println!("[wl_seat] {:?}", event);
        Some(event)
    }
}

#[macro_export]
macro_rules! register_wl_seat {
    () => {
        app::module::Module::<crate::wayland::WlSeat>::new().processor(
            |s: &mut crate::wayland::WlSeat, ev: &crate::wayland::WaylandRawEvent| s.process(ev),
        )
    };
}
