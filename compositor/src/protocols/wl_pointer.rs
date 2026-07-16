use app::{RegisteredModule, prelude::*};
use wayland::{
    Handle, WlPointer, WlPointerEvent, WlPointerRequest, WlSeatCapability, WlSeatRequest,
};

use crate::Compositor;

#[derive(State)]
pub struct WlPointerState {
    pub client_pointers: Vec<Handle<WlPointer>>,
}

impl WlPointerState {
    pub fn new() -> Self {
        Self {
            client_pointers: Vec::new(),
        }
    }

    pub fn retain_alive(&mut self) {
        self.client_pointers.retain(|p| p.is_alive());
    }
}

pub fn module<S>() -> impl RegisteredModule<Compositor, S> {
    Module::<Compositor, _, _>::new()
        .on(|compositor: &mut Compositor, ev: &WlSeatRequest| {
            match ev {
                WlSeatRequest::GetPointer { id, .. } => {
                    if let Some(capabilities) = compositor.seat.capability
                        && capabilities.contains(WlSeatCapability::Pointer)
                    {
                        compositor
                            .seat
                            .pointer_state
                            .client_pointers
                            .push(id.clone());
                        println!("seat pointer: {:?}", id.object_id().expect("live pointer"));
                    } else {
                        // TODO Send WlSeatError - through WlDisplay
                    }
                }
                _ => (),
            }
        })
        .on(|compositor: &mut Compositor, ev: &WlPointerEvent| {
            compositor.seat.pointer_state.retain_alive();
            match ev {
                WlPointerEvent::Enter {
                    sender,
                    serial,
                    surface,
                    surface_x,
                    surface_y,
                } => {
                    println!(
                        "in wl_pointer {:?}: {:?} {:?}",
                        serial, surface_x, surface_y
                    );
                    for pointer in &compositor.seat.pointer_state.client_pointers {
                        pointer.enter(*serial, surface, *surface_x, *surface_y);
                    }
                }
                WlPointerEvent::Leave {
                    sender,
                    serial,
                    surface,
                } => {
                    println!("in wl_pointer leave {:?}", serial);
                    for pointer in &compositor.seat.pointer_state.client_pointers {
                        pointer.leave(*serial, surface);
                    }
                }
                WlPointerEvent::Motion {
                    sender,
                    time,
                    surface_x,
                    surface_y,
                } => {
                    println!(
                        "in wl_pointer motion {:?}: {:?} {:?}",
                        time, surface_x, surface_y
                    );
                    for pointer in &compositor.seat.pointer_state.client_pointers {
                        pointer.motion(*time, *surface_x, *surface_y);
                    }
                }
                WlPointerEvent::Button {
                    sender,
                    serial,
                    time,
                    button,
                    state,
                } => {
                    println!(
                        "in wl_pointer button {:?}: {:?} {:?}",
                        serial, button, state
                    );
                    for pointer in &compositor.seat.pointer_state.client_pointers {
                        pointer.button(*serial, *time, *button, *state);
                    }
                }
                WlPointerEvent::Axis {
                    sender,
                    time,
                    axis,
                    value,
                } => {
                    println!("in wl_pointer axis {:?}: {:?} {:?}", time, axis, value);
                    for pointer in &compositor.seat.pointer_state.client_pointers {
                        pointer.axis(*time, *axis, *value);
                    }
                }
                WlPointerEvent::Frame { sender } => {
                    println!("in wl_pointer frame");
                    for pointer in &compositor.seat.pointer_state.client_pointers {
                        pointer.frame();
                    }
                }
                WlPointerEvent::AxisSource {
                    sender,
                    axis_source,
                } => {
                    println!("in wl_pointer axis_source {:?}", axis_source);
                    for pointer in &compositor.seat.pointer_state.client_pointers {
                        pointer.axis_source(*axis_source);
                    }
                }
                WlPointerEvent::AxisStop { sender, time, axis } => {
                    println!("in wl_pointer axis_stop {:?}: {:?}", time, axis);
                    for pointer in &compositor.seat.pointer_state.client_pointers {
                        pointer.axis_stop(*time, *axis);
                    }
                }
                WlPointerEvent::AxisDiscrete {
                    sender,
                    axis,
                    discrete,
                } => {
                    println!("in wl_pointer axis_discrete {:?}: {:?}", axis, discrete);
                    for pointer in &compositor.seat.pointer_state.client_pointers {
                        pointer.axis_discrete(*axis, *discrete);
                    }
                }
                WlPointerEvent::AxisValue120 {
                    sender,
                    axis,
                    value120,
                } => {
                    println!("in wl_pointer axis_value120 {:?}: {:?}", axis, value120);
                    for pointer in &compositor.seat.pointer_state.client_pointers {
                        pointer.axis_value120(*axis, *value120);
                    }
                }
                WlPointerEvent::AxisRelativeDirection {
                    sender,
                    axis,
                    direction,
                } => {
                    println!(
                        "in wl_pointer axis_relative_direction {:?}: {:?}",
                        axis, direction
                    );
                    for pointer in &compositor.seat.pointer_state.client_pointers {
                        pointer.axis_relative_direction(*axis, *direction);
                    }
                }
            }
        })
        .on(
            |compositor: &mut Compositor, ev: &WlPointerRequest| match ev {
                WlPointerRequest::SetCursor {
                    sender,
                    serial,
                    surface,
                    hotspot_x,
                    hotspot_y,
                } => (),
                WlPointerRequest::Release { sender } => (),
            },
        )
}
