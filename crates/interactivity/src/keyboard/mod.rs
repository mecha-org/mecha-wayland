use std::collections::HashSet;

use wayland::{WlKeyboardEvent, WlKeyboardKeyState};

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

#[derive(Debug, Default)]
pub struct KeyboardState {
    pub modifiers: Modifiers,
    pub held_keys: HashSet<u32>,
    pub repeat_rate: i32,
    pub repeat_delay: i32,
}

impl KeyboardState {
    pub fn new() -> Self {
        Self {
            repeat_rate: -1,
            repeat_delay: -1,
            ..Self::default()
        }
    }

    pub fn is_held(&self, key: u32) -> bool {
        self.held_keys.contains(&key)
    }

    pub fn process(&mut self, ev: &WlKeyboardEvent) {
        match ev {
            WlKeyboardEvent::Key { key, state, .. } => match state {
                WlKeyboardKeyState::Pressed => {
                    self.held_keys.insert(*key);
                }
                WlKeyboardKeyState::Released => {
                    self.held_keys.remove(key);
                }
                _ => (),
            },

            WlKeyboardEvent::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                ..
            } => {
                let combined = mods_depressed | mods_latched | mods_locked;
                self.modifiers = Modifiers::from_xkb(combined);
            }

            WlKeyboardEvent::Enter { keys, .. } => {
                let held: Vec<u32> = keys
                    .chunks_exact(4)
                    .map(|c| u32::from_ne_bytes(c.try_into().unwrap()))
                    .collect();
                for &k in &held {
                    self.held_keys.insert(k);
                }
            }

            WlKeyboardEvent::Leave { .. } => {
                self.held_keys.clear();
            }

            WlKeyboardEvent::RepeatInfo { rate, delay, .. } => {
                self.repeat_rate = *rate;
                self.repeat_delay = *delay;
            }

            WlKeyboardEvent::Keymap { .. } => (),
        }
    }
}
