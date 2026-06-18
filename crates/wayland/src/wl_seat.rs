use app::Event;

use crate::proto::Handle;
use crate::proto::wl_seat as proto;
use crate::{SharedConnection, WaylandRawEvent, parse, send};

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
    handle: Handle<proto::WlSeat>,
    pub capabilities: u32,
}

impl WlSeat {
    pub fn new(conn: SharedConnection) -> Self {
        Self {
            conn,
            handle: Handle::new(0),
            capabilities: 0,
        }
    }

    pub fn set_id(&mut self, id: u32) {
        self.handle = Handle::new(id);
    }

    pub fn get_pointer(&self) -> u32 {
        let id = self.conn.borrow_mut().alloc_id();
        send(&self.conn, &self.handle, &proto::request::GetPointer { id });
        id
    }

    pub fn get_keyboard(&self) -> u32 {
        let id = self.conn.borrow_mut().alloc_id();
        send(
            &self.conn,
            &self.handle,
            &proto::request::GetKeyboard { id },
        );
        id
    }

    pub fn get_touch(&self) -> u32 {
        let id = self.conn.borrow_mut().alloc_id();
        send(&self.conn, &self.handle, &proto::request::GetTouch { id });
        id
    }

    pub fn process(&mut self, raw: &WaylandRawEvent) -> Option<SeatEvent> {
        if raw.sender_id != self.handle.id {
            return None;
        }
        let ev = if let Some(e) = parse::<proto::event::Capabilities>(raw) {
            let caps = e.capabilities.bits();
            self.capabilities = caps;
            SeatEvent::Capabilities { capabilities: caps }
        } else if let Some(e) = parse::<proto::event::Name>(raw) {
            SeatEvent::Name { name: e.name }
        } else {
            return None;
        };
        println!("[wl_seat] {:?}", ev);
        Some(ev)
    }
}

pub fn module<AppState>() -> impl app::RegisteredModule<WlSeat, AppState> {
    app::Module::<WlSeat, _, _>::new()
        .on(|s: &mut WlSeat, ev: &crate::WaylandRawEvent| s.process(ev))
}
