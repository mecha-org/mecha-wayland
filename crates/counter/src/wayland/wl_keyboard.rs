use app::event::Event;

use crate::wayland::{SharedConnection, WaylandRawEvent};
use crate::wire::MessageReader;

#[derive(Debug, Clone)]
pub enum KeyboardEvent {
    Keymap {
        format: u32,
        fd: i32,
        size: u32,
    },
    Enter {
        serial: u32,
        surface: u32,
        keys: Vec<u32>,
    },
    Leave {
        serial: u32,
        surface: u32,
    },
    Key {
        serial: u32,
        time: u32,
        key: u32,
        state: u32,
    },
    Modifiers {
        serial: u32,
        mods_depressed: u32,
        mods_latched: u32,
        mods_locked: u32,
        group: u32,
    },
    RepeatInfo {
        rate: i32,
        delay: i32,
    },
}

impl Event for KeyboardEvent {}

pub struct WlKeyboard {
    conn: SharedConnection,
    pub id: u32,
}

impl WlKeyboard {
    pub fn new(conn: SharedConnection) -> Self {
        Self { conn, id: 0 }
    }

    pub fn set_id(&mut self, id: u32) {
        self.id = id;
    }

    pub fn release(&self) {
        self.conn.borrow_mut().message_builder(self.id, 0).build();
    }

    pub fn process(&mut self, ev: &WaylandRawEvent) -> Option<KeyboardEvent> {
        if ev.sender_id != self.id {
            return None;
        }

        let mut fds = vec![];
        let mut r = MessageReader::new(&ev.body, &mut fds);

        let event = match ev.opcode {
            0 => KeyboardEvent::Keymap {
                format: r.read_u32()?,
                fd: r.read_fd()?,
                size: r.read_u32()?,
            },
            1 => {
                let serial = r.read_u32()?;
                let surface = r.read_u32()?;
                let keys_data = r.read_array()?;
                let keys = keys_data
                    .chunks_exact(4)
                    .map(|c| u32::from_ne_bytes(c.try_into().unwrap()))
                    .collect();
                KeyboardEvent::Enter {
                    serial,
                    surface,
                    keys,
                }
            }
            2 => KeyboardEvent::Leave {
                serial: r.read_u32()?,
                surface: r.read_u32()?,
            },
            3 => KeyboardEvent::Key {
                serial: r.read_u32()?,
                time: r.read_u32()?,
                key: r.read_u32()?,
                state: r.read_u32()?,
            },
            4 => KeyboardEvent::Modifiers {
                serial: r.read_u32()?,
                mods_depressed: r.read_u32()?,
                mods_latched: r.read_u32()?,
                mods_locked: r.read_u32()?,
                group: r.read_u32()?,
            },
            5 => KeyboardEvent::RepeatInfo {
                rate: r.read_i32()?,
                delay: r.read_i32()?,
            },
            _ => return None,
        };

        println!("[wl_keyboard] {:?}", event);
        Some(event)
    }
}

#[macro_export]
macro_rules! register_wl_keyboard {
    () => {
        app::module::Module::<crate::wayland::WlKeyboard>::new().processor(
            |s: &mut crate::wayland::WlKeyboard, ev: &crate::wayland::WaylandRawEvent| {
                s.process(ev)
            },
        )
    };
}
