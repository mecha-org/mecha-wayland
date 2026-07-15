use app::{RegisteredModule, Start, prelude::*};
use wayland::{
    Handle, Interface, WlRegistry, WlRegistryRequest, WlSeat, WlSeatCapability, WlSeatEvent,
    WlSeatRequest,
};

use crate::protocols::{wl_pointer::WlPointerState, wl_registry::RegisterGlobal};

#[derive(State)]
pub struct WlSeatState {
    pub seat: Option<Handle<WlSeat>>, // TODO Multi-seat
    pub capability: Option<WlSeatCapability>,
    pub name: String,
    pub pointer_state: WlPointerState,
}

impl WlSeatState {
    pub fn new() -> Self {
        Self {
            seat: None,
            capability: None,
            name: String::new(),
            pointer_state: WlPointerState::new(),
        }
    }

    fn on_registry_global(
        &mut self,
        name: u32,
        interface: &str,
        version: u32,
        sender: &Handle<WlRegistry>,
    ) {
        match interface {
            WlSeat::NAME => {
                println!("seat found, version {}", version);
                // Connection to host compositor
                let seat = sender.bind(name, version);
                self.seat = Some(seat);
            }
            _ => {}
        }
    }
}

pub fn module<S>() -> impl RegisteredModule<WlSeatState, S> {
    Module::<WlSeatState, _, _>::new()
        .on(|_: &mut WlSeatState, _: &Start| -> Option<RegisterGlobal> {
            Some(RegisterGlobal {
                interface: WlSeat::NAME,
                version: WlSeat::VERSION,
            })
        })
        .on(
            |state: &mut WlSeatState, event: &wayland::WlRegistryEvent| {
                if let wayland::WlRegistryEvent::Global {
                    sender,
                    name,
                    interface,
                    version,
                } = event
                {
                    state.on_registry_global(*name, interface, *version, sender);
                }
            },
        )
        // Events from the host compositor
        .on(|state: &mut WlSeatState, ev: &WlSeatEvent| {
            match ev {
                WlSeatEvent::Capabilities { capabilities, .. } => {
                    println!("seat capabilities: {:?}", capabilities);
                    state.capability = Some(*capabilities);
                    // TODO Forward event to client on input capabilities change after initial flow
                }
                WlSeatEvent::Name { name, .. } => {
                    println!("seat name: {}", name);
                    state.name = name.to_string();
                    // TODO Forward event to client in multi-seat configuration
                }
            }
        })
        // Requests from the clients
        .on(|state: &mut WlSeatState, ev: &WlRegistryRequest| {
            let WlRegistryRequest::Bind {
                sender,
                id,
                interface,
                version,
                ..
            } = ev;
            if interface.as_str() == WlSeat::NAME {
                let handle = sender.proxy.new_handle::<WlSeat>(*id);
                if let Some(capability) = state.capability {
                    handle.capabilities(capability);
                } else {
                    println!("Error(seat): no capabilities registered");
                }
                // wl_seat.name event was added in version 2
                if *version >= 2 {
                    if !state.name.is_empty() {
                        handle.name(&state.name);
                    } else {
                        println!("Error(seat): no name registered");
                    }
                }
            }
        })
        .on(|state: &mut WlSeatState, ev: &WlSeatRequest| match ev {
            WlSeatRequest::Release { .. } => {
                if let Some(seat) = &state.seat {
                    seat.release();
                }
                state.seat = None;
            }
            _ => (),
        })
}
