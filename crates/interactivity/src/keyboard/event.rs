use wayland::KeymapFormat;

/// Decoded keyboard modifier state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub logo: bool,
    pub caps_lock: bool,
    pub num_lock: bool,
}

impl Modifiers {
    /// Decode from the combined (depressed | latched | locked) XKB bitmask.
    pub(super) fn from_xkb(combined: u32) -> Self {
        Self {
            shift: combined & 0x01 != 0,
            ctrl: combined & 0x04 != 0,
            alt: combined & 0x08 != 0,
            logo: combined & 0x40 != 0,
            caps_lock: combined & 0x02 != 0,
            num_lock: combined & 0x10 != 0,
        }
    }

    /// Returns `true` if no modifier keys are active.
    pub fn is_empty(self) -> bool {
        !self.shift && !self.ctrl && !self.alt && !self.logo && !self.caps_lock && !self.num_lock
    }
}

/// High-level keyboard event emitted by the interactivity module.
#[derive(Clone, Debug)]
pub enum KeyEvent {
    /// A key was pressed.
    Press {
        key: u32,
        modifiers: Modifiers,
        time: u32,
    },

    /// A key was released.
    Release {
        key: u32,
        modifiers: Modifiers,
        time: u32,
    },

    /// The active modifier set changed.
    ModifiersChanged { modifiers: Modifiers },

    /// Keyboard focus entered this surface.
    FocusEnter { surface: u32, held_keys: Vec<u32> },

    /// Keyboard focus left this surface.
    FocusLeave { surface: u32 },

    /// The compositor sent its key-repeat configuration.
    ///
    /// - `rate`: repeats per second while a key is held (`0` = repeat
    ///   disabled by the user).
    /// - `delay`: milliseconds before the first repeat fires after initial
    ///   press.
    RepeatInfo { rate: i32, delay: i32 },

    /// The compositor sent an XKB keymap.
    Keymap {
        format: KeymapFormat,
        fd: i32,
        size: u32,
    },
}

impl app::Event for KeyEvent {}
