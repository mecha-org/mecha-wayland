use std::time::Instant;

use app::{RegisteredModule, prelude::*};
use std::os::fd::{AsFd, OwnedFd};
use wayland::{
    Handle, WlKeyboard, WlKeyboardEvent, WlKeyboardKeymapFormat, WlKeyboardRequest,
    WlSeatCapability, WlSeatRequest, WlSurface,
};

use crate::Compositor;

pub struct KbRepeatInfo {
    pub rate: i32,
    pub delay: i32,
}

pub struct KbKeymapInfo {
    pub format: WlKeyboardKeymapFormat,
    pub fd: Option<OwnedFd>,
    pub size: u32,
}

#[derive(State)]
pub struct WlKeyboardState {
    pub keyboard: Option<Handle<WlKeyboard>>,
    pub client_keyboards: Vec<Handle<WlKeyboard>>,
    /// The client surface (if any) that currently has keyboard focus. Dependent on pointer focus for now.
    pub focused_surface: Option<Handle<WlSurface>>,
    #[lens(skip)]
    pub focused_client: Option<Handle<WlKeyboard>>,
    pub repeat_info: KbRepeatInfo,
    pub keymap_info: KbKeymapInfo,
    /// Monotonically increasing serial for enter/leave/key events we generate.
    #[lens(skip)]
    serial: u32,
    start_time: Instant,
}

impl WlKeyboardState {
    pub fn new() -> Self {
        Self {
            keyboard: None,
            client_keyboards: Vec::new(),
            focused_surface: None,
            focused_client: None,
            repeat_info: KbRepeatInfo { rate: 0, delay: 0 },
            keymap_info: KbKeymapInfo {
                format: WlKeyboardKeymapFormat::NoKeymap,
                fd: None,
                size: 0,
            },
            serial: 1,
            start_time: Instant::now(),
        }
    }

    fn time_ms(&self) -> u32 {
        self.start_time.elapsed().as_millis() as u32
    }

    fn next_serial(&mut self) -> u32 {
        let s = self.serial;
        self.serial = self.serial.wrapping_add(1);
        s
    }

    pub fn retain_alive(&mut self) {
        self.client_keyboards.retain(|k| k.is_alive());
    }

    pub fn on_capability_removed(&mut self) {
        self.client_keyboards.clear();
    }

    pub fn send_initial_state(&self, kb: &Handle<WlKeyboard>) {
        let KbRepeatInfo { rate, delay } = self.repeat_info;
        kb.repeat_info(rate, delay);

        if let KbKeymapInfo {
            format,
            fd: Some(fd),
            size,
        } = &self.keymap_info
        {
            kb.keymap(*format, fd.as_fd(), *size);
        }
    }

    pub fn set_keymap(&mut self, format: WlKeyboardKeymapFormat, fd: &OwnedFd, size: u32) {
        for kb in &self.client_keyboards {
            kb.keymap(format, fd.as_fd(), size);
        }
        self.keymap_info = KbKeymapInfo {
            format,
            fd: fd.try_clone().ok(),
            size,
        };
    }

    pub fn set_repeat_info(&mut self, rate: i32, delay: i32) {
        for kb in &self.client_keyboards {
            kb.repeat_info(rate, delay);
        }
        self.repeat_info = KbRepeatInfo { rate, delay };
    }

    pub fn send_focused<F>(
        &mut self,
        pointer_focus: Option<Handle<WlSurface>>,
        mut send_event: F,
    ) where
        F: FnMut(&Handle<WlKeyboard>, u32),
    {
        let serial = self.next_serial();

        let Some(surf) = pointer_focus else {
            // No surface under pointer focus
            self.clear_focus(serial);
            return;
        };

        if let Some(kb_surf) = &self.focused_surface {
            if surf == *kb_surf {
                // Same surface in keyboard focus so just send event
                if let Some(kb) = self.focused_client.clone() {
                    send_event(&kb, serial);
                }
                return;
            }
            // Leave old surface
            // TODO Unable to leave keyboard focus without key event
            let leave_serial = self.next_serial();
            self.clear_focus(leave_serial);
        }

        // Enter new client and send event
        let enter_serial = self.next_serial();
        if let Some(kb) = self.focus_surface(&surf, enter_serial, &[]) {
            let event_serial = self.next_serial();
            send_event(&kb, event_serial);
        }
    }

    pub fn focus_surface(
        &mut self,
        surf: &Handle<WlSurface>,
        serial: u32,
        keys: &[u8],
    ) -> Option<Handle<WlKeyboard>> {
        if let Some(kb) = self
            .client_keyboards
            .iter()
            .find(|kb| kb.proxy.is_same_connection(&surf.proxy))
        {
            kb.enter(serial, surf, keys);
            self.focused_surface = Some(surf.clone());
            self.focused_client = Some(kb.clone());
            println!("in wl_keyboard enter client {:?}", kb);
            Some(kb.clone())
        } else {
            None
        }
    }

    pub fn clear_focus(&mut self, serial: u32) {
        if let Some(surf) = &self.focused_surface
            && let Some(kb) = &self.focused_client
        {
            kb.leave(serial, surf);
            self.focused_surface = None;
            self.focused_client = None;
            println!("in wl_keyboard leave client {:?}", serial);
        }
    }
}

pub fn module<S>() -> impl RegisteredModule<Compositor, S> {
    Module::<Compositor, _, _>::new()
        .on(|compositor: &mut Compositor, ev: &WlSeatRequest| match ev {
            WlSeatRequest::GetKeyboard { id, .. } => {
                if let Some(capabilities) = compositor.seat.capability
                    && capabilities.contains(WlSeatCapability::Keyboard)
                {
                    compositor
                        .seat
                        .keyboard_state
                        .client_keyboards
                        .push(id.clone());
                    println!(
                        "seat keyboard: {:?}",
                        id.object_id().expect("live keyboard")
                    );
                    compositor.seat.keyboard_state.send_initial_state(id);
                } else {
                    // TODO Send WlSeatError - through WlDisplay
                }
            }
            _ => (),
        })
        .on(|compositor: &mut Compositor, ev: &WlKeyboardEvent| {
            let state = &mut compositor.seat.keyboard_state;
            state.retain_alive();
            match ev {
                WlKeyboardEvent::Keymap {
                    format, fd, size, ..
                } => {
                    println!("in wl_keyboard keymap");
                    state.set_keymap(*format, fd, *size);
                }
                WlKeyboardEvent::Enter { keys, .. } => {
                    let serial = state.next_serial();
                    println!("in wl_keyboard enter {:?}", serial);
                    if let Some(surf) = compositor.seat.pointer_state.focused_surface.clone() {
                        state.focus_surface(&surf, serial, keys);
                    }
                }
                WlKeyboardEvent::Leave { .. } => {
                    let serial = state.next_serial();
                    println!("in wl_keyboard leave {:?}", serial);
                    state.clear_focus(serial);
                }
                WlKeyboardEvent::Key {
                    key,
                    state: keystate,
                    ..
                } => {
                    let time = state.time_ms();
                    println!("in wl_keyboard key: {:?} {:?}", key, keystate);

                    let pointer_focus = compositor.seat.pointer_state.focused_surface.clone();
                    state.send_focused(pointer_focus, |kb, serial| {
                        kb.key(serial, time, *key, *keystate);
                        println!(
                            "in wl_keyboard key client {:?}: {:?} {:?}",
                            kb, key, keystate
                        );
                    });
                }
                WlKeyboardEvent::Modifiers {
                    mods_depressed,
                    mods_latched,
                    mods_locked,
                    group,
                    ..
                } => {
                    println!("in wl_keyboard modifiers");

                    let pointer_focus = compositor.seat.pointer_state.focused_surface.clone();
                    state.send_focused(pointer_focus, |kb, serial| {
                        kb.modifiers(
                            serial,
                            *mods_depressed,
                            *mods_latched,
                            *mods_locked,
                            *group,
                        );
                        println!("in wl_keyboard modifiers client {:?}: {:?}", kb, serial);
                    });
                }
                WlKeyboardEvent::RepeatInfo { rate, delay, .. } => {
                    println!("in wl_keyboard repeat_info {:?} {:?}", rate, delay);
                    state.set_repeat_info(*rate, *delay);
                }
            }
        })
        .on(
            |compositor: &mut Compositor, ev: &WlKeyboardRequest| match ev {
                WlKeyboardRequest::Release { sender } => {
                    compositor
                        .seat
                        .keyboard_state
                        .client_keyboards
                        .retain(|k| k.object_id() != sender.object_id());
                }
            },
        )
}
