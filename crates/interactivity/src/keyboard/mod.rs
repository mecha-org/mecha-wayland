mod event;

pub use event::{KeyEvent, Modifiers};

use std::collections::HashSet;
use std::os::fd::AsRawFd;

use wayland::{WlKeyboardEvent, WlKeyboardKeyState};

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

    pub fn process(&mut self, ev: &WlKeyboardEvent) -> Option<KeyEvent> {
        match ev {
            WlKeyboardEvent::Key { key, state, time, .. } => match state {
                WlKeyboardKeyState::Pressed => {
                    self.held_keys.insert(*key);
                    Some(KeyEvent::Press { key: *key, modifiers: self.modifiers, time: *time })
                }
                WlKeyboardKeyState::Released => {
                    self.held_keys.remove(key);
                    Some(KeyEvent::Release { key: *key, modifiers: self.modifiers, time: *time })
                }
                _ => None,
            },

            WlKeyboardEvent::Modifiers { mods_depressed, mods_latched, mods_locked, .. } => {
                let combined = mods_depressed | mods_latched | mods_locked;
                self.modifiers = Modifiers::from_xkb(combined);
                Some(KeyEvent::ModifiersChanged { modifiers: self.modifiers })
            }

            WlKeyboardEvent::Enter { surface, keys, .. } => {
                let held: Vec<u32> = keys
                    .chunks_exact(4)
                    .map(|c| u32::from_ne_bytes(c.try_into().unwrap()))
                    .collect();
                for &k in &held {
                    self.held_keys.insert(k);
                }
                Some(KeyEvent::FocusEnter { surface: surface.object_id()?, held_keys: held })
            }

            WlKeyboardEvent::Leave { surface, .. } => {
                self.held_keys.clear();
                Some(KeyEvent::FocusLeave { surface: surface.object_id()? })
            }

            WlKeyboardEvent::RepeatInfo { rate, delay, .. } => {
                self.repeat_rate = *rate;
                self.repeat_delay = *delay;
                Some(KeyEvent::RepeatInfo { rate: *rate, delay: *delay })
            }

            WlKeyboardEvent::Keymap { format, fd, size, .. } => Some(KeyEvent::Keymap {
                format: *format,
                fd: fd.as_raw_fd(),
                size: *size,
            }),
        }
    }
}
