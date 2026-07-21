use std::{collections::HashMap, time::Instant};

use app::{RegisteredModule, prelude::*};
use wayland::{
    Handle, ObjectId, WaylandProxy, WlPointer, WlPointerEvent, WlPointerRequest, WlSeatCapability,
    WlSeatRequest, WlSurface,
};

use crate::{
    Compositor,
    protocols::wl_surface::{SurfaceData, SurfaceState},
};

// ── State ───────────────────────────────────────────────────────────────────────

#[derive(State)]
pub struct WlPointerState {
    /// Host compositor's wl_pointer (receives events from the host).
    pub pointer: Option<Handle<WlPointer>>,

    /// wl_pointer resources created by internal clients.
    pub client_pointers: Vec<Handle<WlPointer>>,

    /// The client surface (if any) that currently has pointer focus.
    pub focused_surface: Option<Handle<WlSurface>>,

    /// Current host pointer position in compositor-window coordinates.
    pub host_x: f32,
    #[lens(skip)]
    pub host_y: f32,

    /// Maps a client surface's ObjectId to the WaylandProxy of the client
    /// that owns it, so we can route events to the correct wl_pointer.
    pub surface_clients: HashMap<ObjectId, WaylandProxy>,

    /// Monotonically increasing serial for enter/leave/button events we generate.
    #[lens(skip)]
    serial: u32,
    start_time: Instant,
}

impl WlPointerState {
    pub fn new() -> Self {
        Self {
            pointer: None,
            client_pointers: Vec::new(),
            focused_surface: None,
            host_x: 0.0,
            host_y: 0.0,
            surface_clients: HashMap::new(),
            serial: 1,
            start_time: Instant::now(),
        }
    }

    /// Returns elapsed time since creation, in milliseconds, as u32.
    /// Suitable for Wayland event timestamps.
    fn time_ms(&self) -> u32 {
        self.start_time.elapsed().as_millis() as u32
    }

    pub fn retain_alive(&mut self) {
        self.client_pointers.retain(|p| p.is_alive());
    }

    pub fn on_capability_removed(&mut self) {
        self.client_pointers.clear();
        self.focused_surface = None;
        self.surface_clients.clear();
    }

    fn next_serial(&mut self) -> u32 {
        let s = self.serial;
        self.serial = self.serial.wrapping_add(1);
        s
    }

    /// Clear pointer focus: send `leave` to the currently focused surface's client(s)
    /// and reset `state.focused_surface`.
    fn clear_focus(
        &mut self,
        surfaces: &HashMap<Handle<WlSurface>, SurfaceData>,
        serial: u32,
        should_send_frame: bool,
    ) {
        if let Some(fid) = self.focused_surface.take() {
            if surfaces.contains_key(&fid) {
                let n = send_to_client_ptrs(&self.client_pointers, &fid.proxy, |p| {
                    p.leave(serial, &fid);
                    if should_send_frame {
                        p.frame();
                    }
                });
                println!(
                    "ptr leave surface {:?} (serial {}), sent to {} ptr(s)",
                    fid, serial, n
                );
            }
        }
    }
}

// ── Surface layout / hit-test ───────────────────────────────────────────────────

/// Describes a hit-test result from [`SurfaceState::surface_at`].
pub struct HitResult {
    /// Handle of the surface on the *client's* connection.
    pub handle: Handle<WlSurface>,
    /// Surface-local coordinates.
    pub local_x: i32,
    pub local_y: i32,
}

/// Send a pointer event to every *alive* client pointer that belongs to the
/// same connection as `proxy`.  Returns the number of pointers that were sent to.
fn send_to_client_ptrs(
    pointers: &[Handle<WlPointer>],
    proxy: &WaylandProxy,
    f: impl Fn(&Handle<WlPointer>),
) -> usize {
    let mut n = 0;
    for p in pointers {
        // TO REMOVE: Replace proxy equal check with Handle client_id equal check
        if p.is_alive() && p.proxy.is_same_connection(proxy) {
            f(p);
            n += 1;
        }
    }
    n
}

// ── Module ──────────────────────────────────────────────────────────────────────

pub fn module<S>() -> impl RegisteredModule<Compositor, S> {
    Module::<Compositor, _, _>::new()
        // ── wl_seat.get_pointer ──────────────────────────────────────────────
        .on(|compositor: &mut Compositor, ev: &WlSeatRequest| {
            if let WlSeatRequest::GetPointer { id, .. } = ev {
                if let Some(caps) = compositor.seat.capability
                    && caps.contains(WlSeatCapability::Pointer)
                {
                    compositor
                        .seat
                        .pointer_state
                        .client_pointers
                        .push(id.clone());
                    println!("seat pointer: {:?}", id.object_id().expect("live pointer"));
                } else {
                    // TODO Send WlSeatError – through WlDisplay
                }
            }
        })
        // ── Events from the host compositor ──────────────────────────────────
        .on(|compositor: &mut Compositor, ev: &WlPointerEvent| {
            compositor.seat.pointer_state.retain_alive();

            match ev {
                // ── Enter ────────────────────────────────────────────────────
                WlPointerEvent::Enter {
                    sender: _,
                    serial: _,
                    surface: _,
                    surface_x,
                    surface_y,
                } => {
                    let wx = *surface_x;
                    let wy = *surface_y;
                    compositor.seat.pointer_state.host_x = wx;
                    compositor.seat.pointer_state.host_y = wy;

                    println!("ptr enter – {} {})", wx, wy);

                    check_events(compositor);
                }

                // ── Leave ────────────────────────────────────────────────────
                WlPointerEvent::Leave {
                    sender: _,
                    serial: _,
                    surface: _,
                } => {
                    let ps = &mut compositor.seat.pointer_state;
                    let serial = ps.next_serial();
                    ps.clear_focus(&compositor.surfaces.surfaces, serial, true);
                    println!("ptr leave – focus cleared (serial {})", serial);
                }

                // ── Motion ───────────────────────────────────────────────────
                WlPointerEvent::Motion {
                    sender: _,
                    time: _time,
                    surface_x,
                    surface_y,
                } => {
                    let wx = *surface_x;
                    let wy = *surface_y;

                    compositor.seat.pointer_state.host_x = wx;
                    compositor.seat.pointer_state.host_y = wy;

                    check_events(compositor);
                }

                // ── Button ───────────────────────────────────────────────────
                WlPointerEvent::Button {
                    sender: _,
                    serial: _,
                    time: _time,
                    button,
                    state: btn_state,
                } => {
                    let ps = &mut compositor.seat.pointer_state;
                    if let Some(fid) = ps.focused_surface.as_ref().cloned() {
                        let s = ps.next_serial();
                        send_to_client_ptrs(&ps.client_pointers, &fid.proxy, |p| {
                            p.button(s, ps.time_ms(), *button, *btn_state)
                        });
                        println!(
                            "ptr button {:?} state {:?} – forwarded to surface {:?}",
                            button, btn_state, fid
                        );
                    }
                }

                // TODO Accumulate pointer events and batch process on frame
                // // ── Frame ────────────────────────────────────────────────────
                // WlPointerEvent::Frame { .. } => {
                //     let state = &mut compositor.seat.pointer_state;
                //     if let Some(fid) = state.focused_surface {
                //         if let Some(sd) = compositor.surfaces.surfaces.get(&fid) {
                //             send_to_client_ptrs(&state.client_pointers, &sd.handle.proxy, |p| {
                //                 p.frame()
                //             });
                //         }
                //     }
                // }

                // ── Axis ─────────────────────────────────────────────────────
                WlPointerEvent::Axis {
                    sender: _,
                    time: _,
                    axis,
                    value,
                } => {
                    let state = &mut compositor.seat.pointer_state;
                    if let Some(fid) = state.focused_surface.as_ref().clone() {
                        send_to_client_ptrs(&state.client_pointers, &fid.proxy, |p| {
                            p.axis(state.time_ms(), *axis, *value)
                        });
                    }
                }

                // ── AxisSource ───────────────────────────────────────────────
                WlPointerEvent::AxisSource {
                    sender: _,
                    axis_source,
                } => {
                    let state = &mut compositor.seat.pointer_state;
                    if let Some(fid) = state.focused_surface.as_ref().clone() {
                        send_to_client_ptrs(&state.client_pointers, &fid.proxy, |p| {
                            p.axis_source(*axis_source)
                        });
                    }
                }

                // ── AxisStop ─────────────────────────────────────────────────
                WlPointerEvent::AxisStop {
                    sender: _,
                    time: _,
                    axis,
                } => {
                    let state = &mut compositor.seat.pointer_state;
                    if let Some(fid) = state.focused_surface.as_ref().clone() {
                        send_to_client_ptrs(&state.client_pointers, &fid.proxy, |p| {
                            p.axis_stop(state.time_ms(), *axis)
                        });
                    }
                }

                // ── AxisDiscrete ─────────────────────────────────────────────
                WlPointerEvent::AxisDiscrete {
                    sender: _,
                    axis,
                    discrete,
                } => {
                    let state = &mut compositor.seat.pointer_state;
                    if let Some(fid) = state.focused_surface.as_ref().clone() {
                        send_to_client_ptrs(&state.client_pointers, &fid.proxy, |p| {
                            p.axis_discrete(*axis, *discrete)
                        });
                    }
                }

                // TODO Introduced in v8 - forward after version check
                // // ── AxisValue120 ─────────────────────────────────────────────
                // WlPointerEvent::AxisValue120 {
                //     sender: _,
                //     axis,
                //     value120,
                // } => {
                //     let state = &mut compositor.seat.pointer_state;
                //     if let Some(fid) = state.focused_surface.as_ref().clone() {
                //         send_to_client_ptrs(&state.client_pointers, &fid.proxy, |p| {
                //             p.axis_value120(*axis, *value120)
                //         });
                //     }
                // }

                // TODO Introduced in v9 - forward after version check
                // // ── AxisRelativeDirection ────────────────────────────────────
                // WlPointerEvent::AxisRelativeDirection {
                //     sender: _,
                //     axis,
                //     direction,
                // } => {
                //     let state = &mut compositor.seat.pointer_state;
                //     if let Some(fid) = state.focused_surface.as_ref().clone() {
                //         send_to_client_ptrs(&state.client_pointers, &fid.proxy, |p| {
                //             p.axis_relative_direction(*axis, *direction)
                //         });
                //     }
                // }
                _ => (),
            }
        })
        // ── Requests from internal clients ───────────────────────────────────
        .on(|compositor: &mut Compositor, ev: &WlPointerRequest| {
            match ev {
                WlPointerRequest::SetCursor {
                    sender: _,
                    serial: _,
                    surface: _,
                    hotspot_x: _,
                    hotspot_y: _,
                } => {
                    // TODO Forward cursor changes to the host compositor.
                    // if let Some(host_ptr) = &compositor.seat.pointer_state.pointer {
                    //     host_ptr.set_cursor(*serial, surface.as_ref(), *hotspot_x, *hotspot_y);
                    // }
                }
                WlPointerRequest::Release { sender } => {
                    compositor
                        .seat
                        .pointer_state
                        .client_pointers
                        .retain(|p| p.object_id() != sender.object_id());
                }
            }
        })
}

// ── Helpers ──────────────────────────────────────────────────────────────────────

fn surface_hit_test(state: &SurfaceState, gx: i32, gy: i32) -> Option<HitResult> {
    for handle in state.stack.iter().rev() {
        let surf = state.surfaces.get(handle)?;
        let geo = surf.geometry?;
        if !geo.contains(gx, gy) {
            continue;
        }
        if gx < 0 || gy < 0 || gx >= surf.current.buffer_width || gy >= surf.current.buffer_height {
            continue;
        }
        if surf.accepts_input_at(gx, gy) {
            return Some(HitResult {
                handle: handle.clone(),
                local_x: gx,
                local_y: gy,
            });
        }
    }
    None
}

fn check_events(compositor: &mut Compositor) {
    let pointer_state = &mut compositor.seat.pointer_state;
    let (wx, wy) = (pointer_state.host_x, pointer_state.host_y);
    let new_serial = pointer_state.next_serial();

    // Phase 1 – immutable: hit-test surfaces.
    let Some(hit_result) = surface_hit_test(&compositor.surfaces, wx as i32, wy as i32) else {
        // No surface under the pointer – clear focus.
        pointer_state.clear_focus(&compositor.surfaces.surfaces, new_serial, true);
        return;
    };

    // Phase 2 – mutable: update pointer state.
    if let Some(old_id) = pointer_state.focused_surface.as_ref().cloned() {
        if old_id != hit_result.handle {
            // Leave old surface as focused surface changed.
            pointer_state.clear_focus(&compositor.surfaces.surfaces, new_serial, false);

            // Enter the newly focused surface.
            let n = send_to_client_ptrs(
                &pointer_state.client_pointers,
                &hit_result.handle.proxy,
                |p| {
                    p.enter(
                        new_serial,
                        &hit_result.handle,
                        hit_result.local_x as f32,
                        hit_result.local_y as f32,
                    );
                    p.frame();
                },
            );
            pointer_state.focused_surface = Some(hit_result.handle.clone());
            println!(
                "ptr enter new focus surface {:?} @ ({},{}), sent to {} ptr(s)",
                hit_result.handle, hit_result.local_x, hit_result.local_y, n
            );
        } else {
            // Focused surface unchanged - motion event
            let _n = send_to_client_ptrs(
                &pointer_state.client_pointers,
                &hit_result.handle.proxy,
                |p| {
                    p.motion(
                        pointer_state.time_ms(),
                        hit_result.local_x as f32,
                        hit_result.local_y as f32,
                    );
                    p.frame();
                },
            );
        }
    } else {
        // Enter a surface.
        let n = send_to_client_ptrs(
            &pointer_state.client_pointers,
            &hit_result.handle.proxy,
            |p| {
                p.enter(
                    new_serial,
                    &hit_result.handle,
                    hit_result.local_x as f32,
                    hit_result.local_y as f32,
                )
            },
        );
        pointer_state.focused_surface = Some(hit_result.handle.clone());

        println!(
            "ptr enter surface {:?} @ ({},{}), sent to {} ptr(s)",
            hit_result.handle, hit_result.local_x, hit_result.local_y, n
        );
    }
}
