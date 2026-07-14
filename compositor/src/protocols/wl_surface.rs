use std::collections::HashMap;
use std::rc::Rc;

use app::{RegisteredModule, prelude::*};
use smallvec::SmallVec;
use wayland::{
    DISPLAY_OBJECT_ID, Handle, ObjectId, WlCallback, WlCompositorRequest, WlDisplay, WlSurface,
    WlSurfaceRequest,
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

// ── Current state ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct CurrentState {
    pub buffer: Option<ObjectId>,
    pub offset_x: i32,
    pub offset_y: i32,
    pub scale: i32,
    pub transform: i32,
    pub opaque_region: Option<Rc<Region>>,
    pub input_region: Option<Rc<Region>>,

    pub damage_full: bool,
    pub surface_damage: SmallVec<[DamageRect; 4]>,
    pub buffer_damage: SmallVec<[DamageRect; 4]>,

    pub frame_callbacks: SmallVec<[Handle<WlCallback>; 1]>,
    pub release_callbacks: SmallVec<[Handle<WlCallback>; 1]>,
}

impl CurrentState {
    fn new() -> Self {
        Self {
            buffer: None,
            offset_x: 0,
            offset_y: 0,
            scale: 1,
            transform: 0,
            opaque_region: None,
            input_region: None,
            damage_full: false,
            surface_damage: SmallVec::new(),
            buffer_damage: SmallVec::new(),
            frame_callbacks: SmallVec::new(),
            release_callbacks: SmallVec::new(),
        }
    }
}

// ── Pending state ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct PendingState {
    pub buffer: Option<Option<ObjectId>>,
    pub offset_x: i32,
    pub offset_y: i32,
    pub scale: i32,
    pub transform: i32,
    pub opaque_region: Option<Option<Rc<Region>>>,
    pub input_region: Option<Option<Rc<Region>>>,
    pub damage_full: bool,
    pub surface_damage: SmallVec<[DamageRect; 4]>,
    pub buffer_damage: SmallVec<[DamageRect; 4]>,
    pub frame_callbacks: SmallVec<[Handle<WlCallback>; 1]>,
    pub release_callbacks: SmallVec<[Handle<WlCallback>; 1]>,
}

impl PendingState {
    fn new() -> Self {
        Self {
            buffer: None,
            offset_x: 0,
            offset_y: 0,
            scale: 1,
            transform: 0,
            opaque_region: None,
            input_region: None,
            damage_full: false,
            surface_damage: SmallVec::new(),
            buffer_damage: SmallVec::new(),
            frame_callbacks: SmallVec::new(),
            release_callbacks: SmallVec::new(),
        }
    }
}

// ── Surface data ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct SurfaceData {
    pub current: CurrentState,
    pub pending: PendingState,

    pub handle: Handle<WlSurface>,
    pub output: Option<ObjectId>,

    pub previous_buffer: Option<ObjectId>,

    pub role: Option<SurfaceRole>,
}

impl SurfaceData {
    fn new(handle: Handle<WlSurface>) -> Self {
        Self {
            current: CurrentState::new(),
            pending: PendingState::new(),
            handle,
            output: None,
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
        self.current
            .opaque_region
            .as_ref()
            .map(|r| r.contains(x, y))
            .unwrap_or(false)
    }

    pub fn accepts_input_at(&self, x: i32, y: i32) -> bool {
        match &self.current.input_region {
            None => true,
            Some(r) => r.contains(x, y),
        }
    }

    pub fn fire_frame_callbacks(&mut self, now: u32) {
        for cb in self.current.frame_callbacks.drain(..) {
            cb.done(now);
        }
    }

    pub fn fire_release_callbacks(&mut self) {
        for cb in self.current.release_callbacks.drain(..) {
            cb.done(0);
        }
    }

    pub fn commit(&mut self) {
        if !self.pending.release_callbacks.is_empty()
            && !matches!(self.pending.buffer, Some(Some(_)))
        {
            self.pending.release_callbacks.clear();
        }

        // Fire release callbacks for the buffer being replaced.
        if self.current.buffer.is_some() && self.pending.buffer.is_some() {
            self.fire_release_callbacks();
        }

        self.previous_buffer = self.current.buffer;

        if let Some(new_buf) = self.pending.buffer.take() {
            self.current.buffer = new_buf;
        }
        self.current.offset_x = self.current.offset_x.saturating_add(self.pending.offset_x);
        self.current.offset_y = self.current.offset_y.saturating_add(self.pending.offset_y);
        self.pending.offset_x = 0;
        self.pending.offset_y = 0;
        self.current.scale = self.pending.scale;
        self.current.transform = self.pending.transform;
        if let Some(r) = self.pending.opaque_region.take() {
            self.current.opaque_region = r;
        }
        if let Some(r) = self.pending.input_region.take() {
            self.current.input_region = r;
        }

        self.current.damage_full = self.pending.damage_full;
        self.pending.damage_full = false;
        if self.current.damage_full {
            self.pending.surface_damage.clear();
            self.pending.buffer_damage.clear();
        }
        self.current.surface_damage = std::mem::take(&mut self.pending.surface_damage);
        self.current.buffer_damage = std::mem::take(&mut self.pending.buffer_damage);

        self.current
            .frame_callbacks
            .append(&mut self.pending.frame_callbacks);
        self.current
            .release_callbacks
            .append(&mut self.pending.release_callbacks);
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

// ── Module state ────────────────────────────────────────────────────────────────

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

// ── Helpers ─────────────────────────────────────────────────────────────────────

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

// ── Module ──────────────────────────────────────────────────────────────────────

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
        .on(
            |s: &mut SurfaceState, ev: &WlSurfaceRequest| -> Option<SurfaceCommitted> {
                match ev {
                    WlSurfaceRequest::Destroy { sender, .. } => {
                        if let Some((_, surf)) = with_surface(s, sender) {
                            if surf.current.buffer.is_some() {
                                surf.fire_release_callbacks();
                            }
                            surf.pending.frame_callbacks.clear();
                            surf.pending.release_callbacks.clear();
                            surf.current.frame_callbacks.clear();
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
                            surf.pending.buffer = Some(id);
                            surf.pending.offset_x = *x;
                            surf.pending.offset_y = *y;
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
                                &mut surf.pending.damage_full,
                                &mut surf.pending.surface_damage,
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
                                &mut surf.pending.damage_full,
                                &mut surf.pending.buffer_damage,
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
                            surf.pending.frame_callbacks.push(callback.clone());
                        }
                        None
                    }
                    WlSurfaceRequest::GetRelease { sender, callback } => {
                        if let Some((_, surf)) = with_surface(s, sender) {
                            surf.pending.release_callbacks.push(callback.clone());
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
                            surf.pending.opaque_region = Some(resolved);
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
                            surf.pending.input_region = Some(resolved);
                        }
                        None
                    }
                    WlSurfaceRequest::SetBufferScale { sender, scale } => {
                        if let Some((sid, surf)) = with_surface(s, sender) {
                            if *scale <= 0 {
                                post_error(sender, sid, 0, "buffer scale must be > 0");
                            } else {
                                surf.pending.scale = *scale;
                            }
                        }
                        None
                    }
                    WlSurfaceRequest::SetBufferTransform { sender, transform } => {
                        if let Some((sid, surf)) = with_surface(s, sender) {
                            if !(0..=7).contains(transform) {
                                post_error(sender, sid, 1, "invalid buffer transform");
                            } else {
                                surf.pending.transform = *transform;
                            }
                        }
                        None
                    }
                    WlSurfaceRequest::Offset { sender, x, y } => {
                        if let Some((_, surf)) = with_surface(s, sender) {
                            surf.pending.offset_x = *x;
                            surf.pending.offset_y = *y;
                        }
                        None
                    }
                    WlSurfaceRequest::Commit { sender } => {
                        let mut emitted = None;
                        if let Some((sid, surf)) = with_surface(s, sender) {
                            let has_release = !surf.pending.release_callbacks.is_empty();
                            let has_buffer = matches!(surf.pending.buffer, Some(Some(_)));
                            if has_release && !has_buffer {
                                post_error(sender, sid, 5, "get_release without buffer attached");
                            }
                            surf.commit();
                            emitted = Some(SurfaceCommitted { surface_id: sid });
                        }
                        emitted
                    }
                }
            },
        )
}
