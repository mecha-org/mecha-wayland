use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use app::{prelude::*, RegisteredModule};
use smallvec::SmallVec;
use wayland::{
    Handle, ObjectId, WlCallback, WlCompositorRequest, WlDisplay, WlSurface, WlSurfaceRequest,
    DISPLAY_OBJECT_ID,
};

use crate::protocols::wl_region::RegionData;
use crate::rect::Region;

const MAX_DAMAGE: usize = 32;

#[derive(Debug, Clone, Copy)]
pub struct DamageRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug)]
pub struct SurfaceData {
    pub buffer: Option<ObjectId>,
    pub offset_x: i32,
    pub offset_y: i32,
    pub scale: i32,
    pub transform: i32,
    pub opaque_region: Option<Rc<Region>>,
    pub input_region: Option<Rc<Region>>,

    pub handle: Handle<WlSurface>,
    pub output: Option<ObjectId>,

    pub pending_buffer: Option<Option<ObjectId>>,
    pub pending_offset_x: i32,
    pub pending_offset_y: i32,
    pub pending_scale: i32,
    pub pending_transform: i32,
    pub pending_opaque_region: Option<Option<Rc<Region>>>,
    pub pending_input_region: Option<Option<Rc<Region>>>,
    pub pending_damage_full: bool,
    pub pending_surface_damage: SmallVec<[DamageRect; 4]>,
    pub pending_buffer_damage: SmallVec<[DamageRect; 4]>,

    pub pending_frame_callbacks: SmallVec<[Handle<WlCallback>; 1]>,
    pub pending_release_callbacks: SmallVec<[Handle<WlCallback>; 1]>,

    pub committed_frame_callbacks: SmallVec<[Handle<WlCallback>; 1]>,
    pub committed_release_callbacks: SmallVec<[Handle<WlCallback>; 1]>,

    pub committed_damage_full: bool,
    pub committed_surface_damage: SmallVec<[DamageRect; 4]>,
    pub committed_buffer_damage: SmallVec<[DamageRect; 4]>,

    pub previous_buffer: Option<ObjectId>,

    pub role: Option<SurfaceRole>,
}

impl SurfaceData {
    fn new(handle: Handle<WlSurface>) -> Self {
        Self {
            handle,
            output: None,
            buffer: None,
            offset_x: 0,
            offset_y: 0,
            scale: 1,
            transform: 0,
            opaque_region: None,
            input_region: None,
            pending_buffer: None,
            pending_offset_x: 0,
            pending_offset_y: 0,
            pending_scale: 1,
            pending_transform: 0,
            pending_opaque_region: None,
            pending_input_region: None,
            pending_damage_full: false,
            pending_surface_damage: SmallVec::new(),
            pending_buffer_damage: SmallVec::new(),
            pending_frame_callbacks: SmallVec::new(),
            pending_release_callbacks: SmallVec::new(),
            committed_frame_callbacks: SmallVec::new(),
            committed_release_callbacks: SmallVec::new(),
            committed_damage_full: false,
            committed_surface_damage: SmallVec::new(),
            committed_buffer_damage: SmallVec::new(),
            previous_buffer: None,
            role: None,
        }
    }

    pub fn set_role(&mut self, role: SurfaceRole) -> Result<(), u8> {
        if let Some(ref existing) = self.role {
            let k = existing.kind();
            if k != role.kind() {
                return Err(k);
            }
        }
        self.role = Some(role);
        Ok(())
    }

    pub fn is_opaque_at(&self, x: i32, y: i32) -> bool {
        self.opaque_region
            .as_ref()
            .map(|r| r.contains(x, y))
            .unwrap_or(false)
    }

    pub fn accepts_input_at(&self, x: i32, y: i32) -> bool {
        match &self.input_region {
            None => true,
            Some(r) => r.contains(x, y),
        }
    }

    pub fn fire_frame_callbacks(&mut self, now: u32) {
        for cb in self.committed_frame_callbacks.drain(..) {
            cb.done(now);
        }
    }

    pub fn fire_release_callbacks(&mut self) {
        for cb in self.committed_release_callbacks.drain(..) {
            cb.done(0);
        }
    }
}

#[derive(Debug)]
pub enum SurfaceRole {
    XdgSurface(ObjectId),
    LayerSurface(ObjectId),
    LockSurface(ObjectId),
    Cursor,
}

impl SurfaceRole {
    fn kind(&self) -> u8 {
        match self {
            SurfaceRole::XdgSurface(_) => 1,
            SurfaceRole::LayerSurface(_) => 2,
            SurfaceRole::LockSurface(_) => 3,
            SurfaceRole::Cursor => 4,
        }
    }
}

#[derive(Debug, State)]
pub struct SurfaceState {
    pub surfaces: HashMap<ObjectId, SurfaceData>,
    pub regions: HashMap<ObjectId, RegionData>,
}

impl SurfaceState {
    pub fn new() -> Self {
        Self {
            surfaces: HashMap::new(),
            regions: HashMap::new(),
        }
    }
}

#[derive(Debug)]
pub struct SurfaceCommitted {
    pub surface_id: ObjectId,
}
impl Event for SurfaceCommitted {}

fn now_msec() -> u32 {
    static START: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);
    START.elapsed().as_millis() as u32
}

fn post_error(sender: &Handle<impl wayland::Interface>, object_id: ObjectId, code: u32, msg: &str) {
    if let Some(d) = sender.proxy.get_handle::<WlDisplay>(DISPLAY_OBJECT_ID) {
        d.error(object_id, code, msg);
    }
}

fn with_surface<'a>(
    s: &'a mut SurfaceState,
    sender: &Handle<impl wayland::Interface>,
) -> Option<(ObjectId, &'a mut SurfaceData)> {
    let sid = sender.object_id()?;
    Some((sid, s.surfaces.get_mut(&sid)?))
}

fn resolve_region(
    regions: &mut HashMap<ObjectId, RegionData>,
    handle: &Option<Handle<impl wayland::Interface>>,
) -> Rc<Region> {
    handle
        .as_ref()
        .and_then(|h| h.object_id())
        .and_then(|id| regions.get_mut(&id))
        .map(|r| r.resolve())
        .unwrap_or_else(Region::empty)
}

fn normalize_opaque(region: Rc<Region>) -> Option<Rc<Region>> {
    if region.is_empty() {
        None
    } else {
        Some(region)
    }
}

fn add_damage(full: &mut bool, tgt: &mut SmallVec<[DamageRect; 4]>, r: DamageRect) {
    if *full {
        return;
    }
    if tgt.len() >= MAX_DAMAGE {
        *full = true;
        tgt.clear();
        return;
    }
    tgt.push(r);
}

pub fn module<S>() -> impl RegisteredModule<SurfaceState, S> {
    Module::<SurfaceState, _, _>::new()
        .on(|s: &mut SurfaceState, ev: &WlCompositorRequest| {
            if let WlCompositorRequest::CreateSurface { id, .. } = ev {
                if let Some(sid) = id.object_id() {
                    s.surfaces.insert(sid, SurfaceData::new(id.clone()));
                }
            }
            hlist![]
        })
        .on(|s: &mut SurfaceState, ev: &WlSurfaceRequest| -> Option<SurfaceCommitted> {
            match ev {
                WlSurfaceRequest::Destroy { sender, .. } => {
                    if let Some((_, surf)) = with_surface(s, sender) {
                        surf.pending_frame_callbacks.clear();
                        surf.pending_release_callbacks.clear();
                        surf.committed_frame_callbacks.clear();
                        surf.committed_release_callbacks.clear();
                    }
                    if let Some(sid) = sender.object_id() {
                        s.surfaces.remove(&sid);
                    }
                    None
                }
                WlSurfaceRequest::Attach {
                    sender,
                    buffer,
                    x,
                    y,
                } => {
                    if let Some((sid, surf)) = with_surface(s, sender) {
                        if *x != 0 || *y != 0 {
                            post_error(sender, sid, 3, "non-zero attach x/y at v5+");
                        }
                        let id = buffer.as_ref().and_then(|b| b.object_id());
                        surf.pending_buffer = Some(id);
                        surf.pending_offset_x = *x;
                        surf.pending_offset_y = *y;
                    }
                    None
                }
                WlSurfaceRequest::Damage {
                    sender,
                    x,
                    y,
                    width,
                    height,
                } => {
                    if let Some((_, surf)) = with_surface(s, sender) {
                        add_damage(
                            &mut surf.pending_damage_full,
                            &mut surf.pending_surface_damage,
                            DamageRect {
                                x: *x,
                                y: *y,
                                width: *width,
                                height: *height,
                            },
                        );
                    }
                    None
                }
                WlSurfaceRequest::DamageBuffer {
                    sender,
                    x,
                    y,
                    width,
                    height,
                } => {
                    if let Some((_, surf)) = with_surface(s, sender) {
                        add_damage(
                            &mut surf.pending_damage_full,
                            &mut surf.pending_buffer_damage,
                            DamageRect {
                                x: *x,
                                y: *y,
                                width: *width,
                                height: *height,
                            },
                        );
                    }
                    None
                }
                WlSurfaceRequest::Frame { sender, callback } => {
                    if let Some((_, surf)) = with_surface(s, sender) {
                        surf.pending_frame_callbacks.push(callback.clone());
                    }
                    None
                }
                WlSurfaceRequest::GetRelease { sender, callback } => {
                    if let Some((_, surf)) = with_surface(s, sender) {
                        surf.pending_release_callbacks.push(callback.clone());
                    }
                    None
                }
                WlSurfaceRequest::SetOpaqueRegion { sender, region } => {
                    let resolved = if region.is_none() {
                        None
                    } else {
                        normalize_opaque(resolve_region(&mut s.regions, region))
                    };
                    if let Some((_, surf)) = with_surface(s, sender) {
                        surf.pending_opaque_region = Some(resolved);
                    }
                    None
                }
                WlSurfaceRequest::SetInputRegion { sender, region } => {
                    let resolved = if region.is_none() {
                        None
                    } else {
                        Some(resolve_region(&mut s.regions, region))
                    };
                    if let Some((_, surf)) = with_surface(s, sender) {
                        surf.pending_input_region = Some(resolved);
                    }
                    None
                }
                WlSurfaceRequest::SetBufferScale { sender, scale } => {
                    if let Some((sid, surf)) = with_surface(s, sender) {
                        if *scale <= 0 {
                            post_error(sender, sid, 0, "buffer scale must be > 0");
                        } else {
                            surf.pending_scale = *scale;
                        }
                    }
                    None
                }
                WlSurfaceRequest::SetBufferTransform { sender, transform } => {
                    if let Some((sid, surf)) = with_surface(s, sender) {
                        if !(0..=7).contains(transform) {
                            post_error(sender, sid, 1, "invalid buffer transform");
                        } else {
                            surf.pending_transform = *transform;
                        }
                    }
                    None
                }
                WlSurfaceRequest::Offset { sender, x, y } => {
                    if let Some((_, surf)) = with_surface(s, sender) {
                        surf.pending_offset_x = *x;
                        surf.pending_offset_y = *y;
                    }
                    None
                }
                WlSurfaceRequest::Commit { sender } => {
                    let mut emitted = None;
                    if let Some((sid, surf)) = with_surface(s, sender) {
                        if !surf.pending_release_callbacks.is_empty()
                            && !matches!(surf.pending_buffer, Some(Some(_)))
                        {
                            post_error(sender, sid, 5, "get_release without buffer attached");
                            surf.pending_release_callbacks.clear();
                        }

                        surf.previous_buffer = surf.buffer;

                        if let Some(new_buf) = surf.pending_buffer.take() {
                            surf.buffer = new_buf;
                        }
                        surf.offset_x = surf.offset_x.saturating_add(surf.pending_offset_x);
                        surf.offset_y = surf.offset_y.saturating_add(surf.pending_offset_y);
                        surf.pending_offset_x = 0;
                        surf.pending_offset_y = 0;
                        surf.scale = surf.pending_scale;
                        surf.transform = surf.pending_transform;
                        if let Some(r) = surf.pending_opaque_region.take() {
                            surf.opaque_region = r;
                        }
                        if let Some(r) = surf.pending_input_region.take() {
                            surf.input_region = r;
                        }

                        surf.committed_damage_full = surf.pending_damage_full;
                        surf.pending_damage_full = false;
                        if surf.committed_damage_full {
                            surf.pending_surface_damage.clear();
                            surf.pending_buffer_damage.clear();
                        }
                        surf.committed_surface_damage =
                            std::mem::take(&mut surf.pending_surface_damage);
                        surf.committed_buffer_damage =
                            std::mem::take(&mut surf.pending_buffer_damage);

                        surf.committed_frame_callbacks
                            .append(&mut surf.pending_frame_callbacks);
                        surf.committed_release_callbacks
                            .append(&mut surf.pending_release_callbacks);

                        emitted = Some(SurfaceCommitted { surface_id: sid });
                    }
                    emitted
                }
            }
        })
}
