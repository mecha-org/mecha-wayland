use app::{RegisteredModule, prelude::*};
use std::os::fd::AsFd;
use wayland::{
    Handle, WlKeyboard, WlKeyboardEvent, WlKeyboardRequest, WlSeatCapability, WlSeatRequest,
};

use crate::Compositor;

#[derive(State)]
pub struct WlKeyboardState {
    pub keyboard: Option<Handle<WlKeyboard>>,
    pub client_keyboards: Vec<Handle<WlKeyboard>>,
}

impl WlKeyboardState {
    pub fn new() -> Self {
        Self {
            keyboard: None,
            client_keyboards: Vec::new(),
        }
    }

    pub fn retain_alive(&mut self) {
        self.client_keyboards.retain(|k| k.is_alive());
    }

    pub fn on_capability_removed(&mut self) {
        self.client_keyboards.clear();
    }
}

pub fn module<S>() -> impl RegisteredModule<Compositor, S> {
    Module::<Compositor, _, _>::new()
        .on(|compositor: &mut Compositor, ev: &WlSeatRequest| {
            match ev {
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
                    } else {
                        // TODO Send WlSeatError - through WlDisplay
                    }
                }
                _ => (),
            }
        })
        .on(|compositor: &mut Compositor, ev: &WlKeyboardEvent| {
            compositor.seat.keyboard_state.retain_alive();
            match ev {
                WlKeyboardEvent::Keymap {
                    sender,
                    format,
                    fd,
                    size,
                } => {
                    println!("in wl_keyboard keymap");
                    for kb in &compositor.seat.keyboard_state.client_keyboards {
                        kb.keymap(*format, fd.as_fd(), *size);
                    }
                }
                WlKeyboardEvent::Enter {
                    sender,
                    serial,
                    surface,
                    keys,
                } => {
                    println!("in wl_keyboard enter {:?}", serial);
                    for kb in &compositor.seat.keyboard_state.client_keyboards {
                        kb.enter(*serial, surface, keys);
                    }
                }
                WlKeyboardEvent::Leave {
                    sender,
                    serial,
                    surface,
                } => {
                    println!("in wl_keyboard leave {:?}", serial);
                    for kb in &compositor.seat.keyboard_state.client_keyboards {
                        kb.leave(*serial, surface);
                    }
                }
                WlKeyboardEvent::Key {
                    sender,
                    serial,
                    time,
                    key,
                    state,
                } => {
                    println!("in wl_keyboard key {:?}: {:?} {:?}", serial, key, state);
                    for kb in &compositor.seat.keyboard_state.client_keyboards {
                        kb.key(*serial, *time, *key, *state);
                    }
                }
                WlKeyboardEvent::Modifiers {
                    sender,
                    serial,
                    mods_depressed,
                    mods_latched,
                    mods_locked,
                    group,
                } => {
                    println!("in wl_keyboard modifiers {:?}", serial);
                    for kb in &compositor.seat.keyboard_state.client_keyboards {
                        kb.modifiers(
                            *serial,
                            *mods_depressed,
                            *mods_latched,
                            *mods_locked,
                            *group,
                        );
                    }
                }
                WlKeyboardEvent::RepeatInfo {
                    sender,
                    rate,
                    delay,
                } => {
                    println!("in wl_keyboard repeat_info {:?} {:?}", rate, delay);
                    for kb in &compositor.seat.keyboard_state.client_keyboards {
                        kb.repeat_info(*rate, *delay);
                    }
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
