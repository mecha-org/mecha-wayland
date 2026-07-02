use std::collections::HashSet;

use wayland::{WlKeyboardEvent, WlKeyboardKeyState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyCode(pub u32);

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
    pub(super) fn from_wayland(combined: u32) -> Self {
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

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Default)]
pub struct KeyboardState {
    modifiers: Modifiers,
    pressed_keys: HashSet<KeyCode>,
    just_pressed_keys: HashSet<KeyCode>,
    just_released_keys: HashSet<KeyCode>,
    just_repeated_keys: HashSet<KeyCode>,
    repeat_rate: i32,
    repeat_delay: i32,
}

impl KeyboardState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process(&mut self, ev: &WlKeyboardEvent) {
        match ev {
            WlKeyboardEvent::Keymap { .. } => {}

            WlKeyboardEvent::Enter { keys, .. } => {
                self.pressed_keys.clear();

                for chunk in keys.chunks_exact(4) {
                    self.pressed_keys
                        .insert(KeyCode(u32::from_ne_bytes(chunk.try_into().unwrap())));
                }
            }

            WlKeyboardEvent::Leave { .. } => {
                self.pressed_keys.clear();
                self.just_pressed_keys.clear();
                self.just_released_keys.clear();
            }

            WlKeyboardEvent::Key { key, state, .. } => match state {
                WlKeyboardKeyState::Pressed => {
                    self.pressed_keys.insert(KeyCode(*key));
                    self.just_pressed_keys.insert(KeyCode(*key));
                }

                WlKeyboardKeyState::Released => {
                    self.pressed_keys.remove(&KeyCode(*key));
                    self.just_released_keys.insert(KeyCode(*key));
                }

                WlKeyboardKeyState::Repeated => {
                    self.just_repeated_keys.insert(KeyCode(*key));
                }
            },

            WlKeyboardEvent::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                ..
            } => {
                let combined = mods_depressed | mods_latched | mods_locked;
                self.modifiers = Modifiers::from_wayland(combined);
            }

            WlKeyboardEvent::RepeatInfo { rate, delay, .. } => {
                self.repeat_rate = *rate;
                self.repeat_delay = *delay;
            }
        }
    }

    /// Clears all per-frame state.
    ///
    /// This should be called once per frame before processing new events.
    pub fn clear(&mut self) {
        self.just_pressed_keys.clear();
        self.just_released_keys.clear();
        self.just_repeated_keys.clear();
    }

    /// Returns the current modifier keys.
    pub fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// Returns the configured key repeat rate.
    pub fn repeat_rate(&self) -> i32 {
        self.repeat_rate
    }

    /// Returns the configured key repeat delay.
    pub fn repeat_delay(&self) -> i32 {
        self.repeat_delay
    }

    // -----------------------------------------------------------------------------
    // Just Pressed
    // -----------------------------------------------------------------------------

    /// Returns all keys that were pressed this frame.
    pub fn just_pressed_keys(&self) -> &HashSet<KeyCode> {
        &self.just_pressed_keys
    }

    /// Returns true if `key` was pressed this frame.
    pub fn just_pressed(&self, key: KeyCode) -> bool {
        self.just_pressed_keys.contains(&key)
    }

    // -----------------------------------------------------------------------------
    // Pressed
    // -----------------------------------------------------------------------------

    /// Returns all keys that are currently held down.
    pub fn pressed_keys(&self) -> &HashSet<KeyCode> {
        &self.pressed_keys
    }

    /// Returns true if `key` is currently held down.
    pub fn pressed(&self, key: KeyCode) -> bool {
        self.pressed_keys.contains(&key)
    }

    // -----------------------------------------------------------------------------
    // Just Repeated
    // -----------------------------------------------------------------------------

    /// Returns all keys that repeated this frame.
    pub fn just_repeated_keys(&self) -> &HashSet<KeyCode> {
        &self.just_repeated_keys
    }

    /// Returns true if `key` repeated this frame.
    pub fn just_repeated(&self, key: KeyCode) -> bool {
        self.just_repeated_keys.contains(&key)
    }

    // -----------------------------------------------------------------------------
    // Just Released
    // -----------------------------------------------------------------------------

    /// Returns all keys that were released this frame.
    pub fn just_released_keys(&self) -> &HashSet<KeyCode> {
        &self.just_released_keys
    }

    /// Returns true if `key` was released this frame.
    pub fn just_released(&self, key: KeyCode) -> bool {
        self.just_released_keys.contains(&key)
    }
}
