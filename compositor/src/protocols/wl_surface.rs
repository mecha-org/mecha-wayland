use std::collections::HashMap;

use app::{RegisteredModule, prelude::*};
use wayland::{Handle, ObjectId, WlCallback, WlSurfaceRequest};

#[derive(Default)]
pub struct SurfacePendingState {
    pub buffer: Option<Option<ObjectId>>,
    pub buffer_x: i32,
    pub buffer_y: i32,
    pub frame_callbacks: Vec<Handle<WlCallback>>,
    pub damage: Vec<(i32, i32, i32, i32)>,
}

pub struct SurfaceCurrentState {
    pub buffer: Option<ObjectId>,
    pub buffer_x: i32,
    pub buffer_y: i32,
    pub frame_callbacks: Vec<Handle<WlCallback>>,
}

pub struct Surface {
    pub pending: SurfacePendingState,
    pub current: SurfaceCurrentState,
    pub previous_buffer: Option<ObjectId>,
}

impl Surface {
    pub fn new() -> Self {
        Self {
            pending: SurfacePendingState::default(),
            current: SurfaceCurrentState {
                buffer: None,
                buffer_x: 0,
                buffer_y: 0,
                frame_callbacks: Vec::new(),
            },
            previous_buffer: None,
        }
    }

    pub fn commit(&mut self) {
        self.previous_buffer = self.current.buffer.take();
        match self.pending.buffer.take() {
            Some(new_buf) => self.current.buffer = new_buf,
            None => self.current.buffer = self.previous_buffer,
        }
        self.current.buffer_x = self.pending.buffer_x;
        self.current.buffer_y = self.pending.buffer_y;
        self.current.frame_callbacks.append(&mut self.pending.frame_callbacks);
        self.pending.damage.clear();
    }
}

#[derive(State)]
pub struct WlSurfaceState {
    pub surfaces: HashMap<ObjectId, Surface>,
}

impl WlSurfaceState {
    pub fn new() -> Self {
        Self { surfaces: HashMap::new() }
    }
}

#[derive(Debug)]
pub struct SurfaceCommitted {
    pub surface_id: ObjectId,
}
impl Event for SurfaceCommitted {}

pub fn module<S>() -> impl RegisteredModule<WlSurfaceState, S> {
    Module::<WlSurfaceState, _, _>::new()
        .on(|state: &mut WlSurfaceState, ev: &WlSurfaceRequest| -> Option<SurfaceCommitted> {
            match ev {
                WlSurfaceRequest::Destroy { sender } => {
                    state.surfaces.remove(&sender.object_id().expect("live surface"));
                    None
                }
                WlSurfaceRequest::Attach { sender, buffer, x, y } => {
                    let id = sender.object_id().expect("live surface");
                    if let Some(surface) = state.surfaces.get_mut(&id) {
                        surface.pending.buffer =
                            Some(buffer.as_ref().and_then(|b| b.object_id()));
                        surface.pending.buffer_x = *x;
                        surface.pending.buffer_y = *y;
                    }
                    None
                }
                WlSurfaceRequest::Damage { sender, x, y, width, height } => {
                    let id = sender.object_id().expect("live surface");
                    if let Some(surface) = state.surfaces.get_mut(&id) {
                        surface.pending.damage.push((*x, *y, *width, *height));
                    }
                    None
                }
                WlSurfaceRequest::DamageBuffer { sender, x, y, width, height } => {
                    let id = sender.object_id().expect("live surface");
                    if let Some(surface) = state.surfaces.get_mut(&id) {
                        surface.pending.damage.push((*x, *y, *width, *height));
                    }
                    None
                }
                WlSurfaceRequest::Frame { sender, callback } => {
                    let id = sender.object_id().expect("live surface");
                    if let Some(surface) = state.surfaces.get_mut(&id) {
                        surface.pending.frame_callbacks.push(callback.clone());
                    }
                    None
                }
                WlSurfaceRequest::Commit { sender } => {
                    let id = sender.object_id().expect("live surface");
                    if let Some(surface) = state.surfaces.get_mut(&id) {
                        surface.commit();
                        Some(SurfaceCommitted { surface_id: id })
                    } else {
                        None
                    }
                }
                WlSurfaceRequest::SetOpaqueRegion { .. }
                | WlSurfaceRequest::SetInputRegion { .. }
                | WlSurfaceRequest::SetBufferTransform { .. }
                | WlSurfaceRequest::SetBufferScale { .. }
                | WlSurfaceRequest::Offset { .. }
                | WlSurfaceRequest::GetRelease { .. } => None,
            }
        })
}
