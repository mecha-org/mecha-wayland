use app::event::Event;

use crate::wayland::{SharedConnection, WaylandRawEvent};
use crate::wire::MessageReader;

#[derive(Debug)]
pub enum CallbackEvent {
    Done { callback_data: u32 },
}

impl Event for CallbackEvent {}

pub struct WlCallback {
    _conn: SharedConnection,
    id: u32,
    done: bool,
}

impl WlCallback {
    pub fn new(conn: SharedConnection) -> Self {
        Self { _conn: conn, id: 0, done: false }
    }

    pub fn set_id(&mut self, id: u32) {
        self.id = id;
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn process(&mut self, ev: &WaylandRawEvent) -> Option<CallbackEvent> {
        if ev.sender_id != self.id || ev.opcode != 0 {
            return None;
        }
        let mut fds = vec![];
        let mut r = MessageReader::new(&ev.body, &mut fds);
        let callback_data = r.read_u32().unwrap_or(0);
        let event = CallbackEvent::Done { callback_data };
        println!("[wl_callback] {:?}", event);
        Some(event)
    }

    pub fn handle_event(&mut self, sender_id: u32, opcode: u16, _body: &[u8]) {
        if sender_id != self.id {
            return;
        }
        if opcode == 0 {
            self.done = true;
        }
    }
}

#[macro_export]
macro_rules! register_wl_callback {
    () => {
        app::module::Module::<crate::wayland::WlCallback>::new()
            .processor(|c: &mut crate::wayland::WlCallback, ev: &crate::wayland::WaylandRawEvent| {
                c.process(ev)
            })
    };
}
