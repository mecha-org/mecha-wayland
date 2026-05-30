use app::Event;

use crate::proto::Handle;
use crate::proto::wl_keyboard as proto;
use crate::{SharedConnection, WaylandRawEvent, parse};

pub type KeymapFormat = crate::proto::wl_keyboard::WlKeyboardKeymapFormat;
pub type KeyState = crate::proto::wl_keyboard::WlKeyboardKeyState;

#[derive(Debug, Clone)]
pub enum KeyboardEvent {
    Keymap {
        format: KeymapFormat,
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
        state: KeyState,
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
    _conn: SharedConnection,
    handle: Handle<proto::WlKeyboard>,
}

impl WlKeyboard {
    pub fn new(conn: SharedConnection) -> Self {
        Self {
            _conn: conn,
            handle: Handle::new(0),
        }
    }

    pub fn id(&self) -> u32 {
        self.handle.id
    }

    pub fn set_id(&mut self, id: u32) {
        self.handle = Handle::new(id);
    }

    pub fn process(&mut self, raw: &WaylandRawEvent) -> Option<KeyboardEvent> {
        if raw.sender_id != self.handle.id {
            return None;
        }
        let ev = if let Some(e) = parse::<proto::event::Keymap>(raw) {
            KeyboardEvent::Keymap {
                format: e.format,
                fd: e.fd,
                size: e.size,
            }
        } else if let Some(e) = parse::<proto::event::Enter>(raw) {
            KeyboardEvent::Enter {
                serial: e.serial,
                surface: e.surface,
                keys: e
                    .keys
                    .chunks_exact(4)
                    .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                    .collect(),
            }
        } else if let Some(e) = parse::<proto::event::Leave>(raw) {
            KeyboardEvent::Leave {
                serial: e.serial,
                surface: e.surface,
            }
        } else if let Some(e) = parse::<proto::event::Key>(raw) {
            KeyboardEvent::Key {
                serial: e.serial,
                time: e.time,
                key: e.key,
                state: e.state,
            }
        } else if let Some(e) = parse::<proto::event::Modifiers>(raw) {
            KeyboardEvent::Modifiers {
                serial: e.serial,
                mods_depressed: e.mods_depressed,
                mods_latched: e.mods_latched,
                mods_locked: e.mods_locked,
                group: e.group,
            }
        } else if let Some(e) = parse::<proto::event::RepeatInfo>(raw) {
            KeyboardEvent::RepeatInfo {
                rate: e.rate,
                delay: e.delay,
            }
        } else {
            return None;
        };
        println!("[wl_keyboard] {:?}", ev);
        Some(ev)
    }
}

pub fn module<AppState>() -> impl app::RegisteredModule<WlKeyboard, AppState> {
    app::Module::<WlKeyboard, _, _>::new()
        .on(|s: &mut WlKeyboard, ev: &crate::WaylandRawEvent| s.process(ev))
}
