use wayland::{ObjectId, WlKeyboardKeymapFormat};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub logo: bool,
    pub caps_lock: bool,
    pub num_lock: bool,
    pub scroll_lock: bool,
}

impl Modifiers {
    pub(super) fn from_xkb(combined: u32) -> Self {
        Self {
            shift: combined & 0x01 != 0,
            caps_lock: combined & 0x02 != 0,
            ctrl: combined & 0x04 != 0,
            alt: combined & 0x08 != 0,
            num_lock: combined & 0x10 != 0,
            scroll_lock: combined & 0x20 != 0,
            logo: combined & 0x40 != 0,
        }
    }

    pub fn is_empty(self) -> bool {
        !self.shift && !self.ctrl && !self.alt && !self.logo && !self.caps_lock && !self.num_lock
    }
}

#[derive(Clone, Debug)]
pub enum KeyEvent {
    Press { key: u32, modifiers: Modifiers, time: u32 },
    Release { key: u32, modifiers: Modifiers, time: u32 },
    Hold { key: u32, modifiers: Modifiers },
    ModifiersChanged { modifiers: Modifiers },
    FocusEnter { surface: ObjectId, held_keys: Vec<u32> },
    FocusLeave { surface: ObjectId },
    RepeatInfo { rate: i32, delay: i32 },
    Keymap { format: WlKeyboardKeymapFormat, fd: i32, size: u32 },
}

impl app::Event for KeyEvent {}
