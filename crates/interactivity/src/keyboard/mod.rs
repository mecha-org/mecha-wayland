mod event;

pub use event::{KeyEvent, Modifiers};

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use wayland::{KeyState, KeyboardEvent as WlKeyboardEvent};

/// Tracks keyboard focus, modifier flags, held keys, and repeat configuration.
#[derive(Debug, Default)]
pub struct KeyboardState {
    /// Currently active modifier flags.
    pub modifiers: Modifiers,
    /// Physical evdev scancodes that are currently pressed down.
    pub held_keys: HashSet<u32>,
    /// Key repeats per second configured by the compositor.
    /// `0` means the user has disabled key repeat.
    /// `-1` means the compositor hasn't sent `repeat_info` yet.
    pub repeat_rate: i32,
    /// Milliseconds before the first key repeat fires after initial press.
    /// `-1` means the compositor hasn't sent `repeat_info` yet.
    pub repeat_delay: i32,
    /// Tracks active repeating keys (key code -> (next repeat time, repeat interval)).
    pub(crate) repeating_keys: HashMap<u32, (Instant, Duration)>,
}

impl KeyboardState {
    pub fn new() -> Self {
        Self {
            repeat_rate: -1,
            repeat_delay: -1,
            ..Self::default()
        }
    }

    /// Returns `true` if the given physical evdev scancode is currently held.
    pub fn is_held(&self, key: u32) -> bool {
        self.held_keys.contains(&key)
    }

    /// Translate one raw Wayland [`WlKeyboardEvent`] into a semantic [`KeyEvent`].
    pub(crate) fn process(&mut self, ev: &WlKeyboardEvent) -> Option<KeyEvent> {
        match ev {
            WlKeyboardEvent::Key {
                key, state, time, ..
            } => match state {
                KeyState::Pressed => {
                    self.held_keys.insert(*key);
                    if self.repeat_rate > 0 && self.repeat_delay >= 0 {
                        let delay = Duration::from_millis(self.repeat_delay as u64);
                        let interval =
                            Duration::from_millis((1000.0 / self.repeat_rate as f64) as u64);
                        let next_repeat = Instant::now() + delay;
                        self.repeating_keys.insert(*key, (next_repeat, interval));
                    }
                    Some(KeyEvent::Press {
                        key: *key,
                        modifiers: self.modifiers,
                        time: *time,
                    })
                }
                KeyState::Released => {
                    self.held_keys.remove(key);
                    self.repeating_keys.remove(key);
                    Some(KeyEvent::Release {
                        key: *key,
                        modifiers: self.modifiers,
                        time: *time,
                    })
                }
                _ => None,
            },

            WlKeyboardEvent::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                ..
            } => {
                let combined = mods_depressed | mods_latched | mods_locked;
                self.modifiers = Modifiers::from_xkb(combined);
                Some(KeyEvent::ModifiersChanged {
                    modifiers: self.modifiers,
                })
            }

            WlKeyboardEvent::Enter { surface, keys, .. } => {
                let held: Vec<u32> = keys.clone();
                for &k in &held {
                    self.held_keys.insert(k);
                }
                Some(KeyEvent::FocusEnter {
                    surface: *surface,
                    held_keys: held,
                })
            }

            WlKeyboardEvent::Leave { surface, .. } => {
                self.held_keys.clear();
                self.repeating_keys.clear();
                Some(KeyEvent::FocusLeave { surface: *surface })
            }

            WlKeyboardEvent::RepeatInfo { rate, delay } => {
                self.repeat_rate = *rate;
                self.repeat_delay = *delay;
                Some(KeyEvent::RepeatInfo {
                    rate: *rate,
                    delay: *delay,
                })
            }

            WlKeyboardEvent::Keymap { format, fd, size } => Some(KeyEvent::Keymap {
                format: *format,
                fd: *fd,
                size: *size,
            }),
        }
    }

    /// Check repeating keys and return any Hold events.
    /// Catches up if multiple intervals have elapsed since last tick.
    pub(crate) fn tick(&mut self) -> Vec<KeyEvent> {
        let now = Instant::now();
        let mut events = Vec::new();

        for (&key, (next_repeat, interval)) in &mut self.repeating_keys {
            while now >= *next_repeat {
                events.push(KeyEvent::Hold {
                    key,
                    modifiers: self.modifiers,
                });
                *next_repeat += *interval;
            }
        }
        events
    }
}
