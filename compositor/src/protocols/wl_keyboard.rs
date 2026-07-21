use app::{RegisteredModule, prelude::*};
use std::os::fd::AsFd;
use wayland::{
    Handle, WlKeyboard, WlKeyboardEvent, WlKeyboardRequest, WlSeatCapability, WlSeatRequest,
    WlSurface,
};

use crate::Compositor;

#[derive(State)]
pub struct WlKeyboardState {
    pub keyboard: Option<Handle<WlKeyboard>>,
    pub client_keyboards: Vec<Handle<WlKeyboard>>,
    /// The client surface (if any) that currently has keyboard focus. Dependent on pointer focus for now.
    pub focused_surface: Option<Handle<WlSurface>>,
    #[lens(skip)]
    pub focused_client: Option<Handle<WlKeyboard>>,
}

impl WlKeyboardState {
    pub fn new() -> Self {
        Self {
            keyboard: None,
            client_keyboards: Vec::new(),
            focused_surface: None,
            focused_client: None,
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
                        // TODO Send all cached info like repeat rate etc.
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
                    if let Some(surf) = &compositor.seat.pointer_state.focused_surface {
                        for kb in &compositor.seat.keyboard_state.client_keyboards {
                            if kb.proxy.is_same_connection(&surf.proxy) {
                                // Send to client's first keyboard handle
                                kb.enter(*serial, surf, keys);
                                compositor.seat.keyboard_state.focused_surface = Some(surf.clone());
                                compositor.seat.keyboard_state.focused_client = Some(kb.clone());
                                println!("in wl_keyboard enter client {:?}", kb);
                                return;
                            }
                        }
                    }
                }
                WlKeyboardEvent::Leave {
                    sender,
                    serial,
                    surface,
                } => {
                    println!("in wl_keyboard leave {:?}", serial);
                    if let Some(surf) = &compositor.seat.keyboard_state.focused_surface
                        && let Some(kb) = &compositor.seat.keyboard_state.focused_client
                    {
                        kb.leave(*serial, surf);
                        compositor.seat.keyboard_state.focused_surface = None;
                        compositor.seat.keyboard_state.focused_client = None;
                        println!("in wl_keyboard leave client {:?}", serial);
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
                    if let Some(kb) = &compositor.seat.keyboard_state.focused_client {
                        kb.key(*serial, *time, *key, *state);
                        println!("in wl_keyboard key client {:?}: {:?} {:?}", kb, key, state);
                    } else if let Some(surf) = &compositor.seat.pointer_state.focused_surface {
                        for kb in &compositor.seat.keyboard_state.client_keyboards {
                            if kb.proxy.is_same_connection(&surf.proxy) {
                                // Send to client's first keyboard handle
                                // TODO Cache keys? Empty array or fixed length?
                                kb.enter(*serial, surf, &[]);
                                compositor.seat.keyboard_state.focused_surface = Some(surf.clone());
                                compositor.seat.keyboard_state.focused_client = Some(kb.clone());
                                println!("in wl_keyboard enter client {:?}", kb);

                                kb.key(*serial, *time, *key, *state);
                                println!(
                                    "in wl_keyboard key client {:?}: {:?} {:?}",
                                    kb, key, state
                                );
                            }
                        }
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
