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
                        // Send all cached info like repeat rate etc.
                        let KbRepeatInfo { rate, delay } =
                            compositor.seat.keyboard_state.repeat_info;
                        id.repeat_info(rate, delay);
                        if let KbKeymapInfo {
                            format,
                            fd: Some(fd),
                            size,
                        } = &compositor.seat.keyboard_state.keymap_info
                        {
                            id.keymap(*format, fd.as_fd(), *size);
                        }
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
                    compositor.seat.keyboard_state.keymap_info = KbKeymapInfo {
                        format: *format,
                        fd: Some(fd.try_clone().expect("Failed to clone fd")),
                        size: *size,
                    };
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
                    compositor.seat.keyboard_state.repeat_info = KbRepeatInfo {
                        rate: *rate,
                        delay: *delay,
                    };
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
