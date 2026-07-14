use std::rc::Rc;

use app::{RegisteredModule, prelude::*};
use wayland::{
    DISPLAY_OBJECT_ID, Handle, ObjectId, WlCompositorRequest, WlDisplay, WlRegionRequest,
};

use crate::protocols::wl_surface::SurfaceState;
use crate::rect::{Rect, Region, RegionBuilder};

/// Wayland region state
#[derive(Debug, Default)]
pub struct RegionData {
    builder: RegionBuilder,
}

impl RegionData {
    /// Snapshot the current region (shared; no rect list copy).
    pub fn resolve(&mut self) -> Rc<Region> {
        self.builder.get()
    }
}

fn post_error(sender: &Handle<impl wayland::Interface>, object_id: ObjectId, code: u32, msg: &str) {
    if let Some(d) = sender.proxy.get_handle::<WlDisplay>(DISPLAY_OBJECT_ID) {
        d.error(object_id, code, msg);
    }
}

/// Apply add/sub if extents are valid. Negative width/height → protocol error.
fn apply_rect(
    sender: &Handle<impl wayland::Interface>,
    data: &mut RegionData,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    add: bool,
) {
    if width < 0 || height < 0 {
        if let Some(sid) = sender.object_id() {
            // wl_region has no error enum; use wl_display.invalid_method.
            post_error(sender, sid, 1, "width and/or height are negative");
        }
        return;
    }
    if width == 0 || height == 0 {
        return;
    }
    let rect = Rect::new_sized_saturating(x, y, width, height);
    if add {
        data.builder.add(rect);
    } else {
        data.builder.sub(rect);
    }
}

pub fn module<S>() -> impl RegisteredModule<SurfaceState, S> {
    Module::<SurfaceState, _, _>::new()
        .on(|s: &mut SurfaceState, ev: &WlCompositorRequest| {
            if let WlCompositorRequest::CreateRegion { id, .. } = ev {
                if let Some(sid) = id.object_id() {
                    s.regions.insert(sid, RegionData::default());
                }
            }
            hlist![]
        })
        .on(|s: &mut SurfaceState, ev: &WlRegionRequest| {
            match ev {
                WlRegionRequest::Destroy { sender, .. } => {
                    if let Some(sid) = sender.object_id() {
                        s.regions.remove(&sid);
                    }
                }
                WlRegionRequest::Add {
                    sender,
                    x,
                    y,
                    width,
                    height,
                } => {
                    if let Some(sid) = sender.object_id() {
                        if let Some(r) = s.regions.get_mut(&sid) {
                            apply_rect(sender, r, *x, *y, *width, *height, true);
                        }
                    }
                }
                WlRegionRequest::Subtract {
                    sender,
                    x,
                    y,
                    width,
                    height,
                } => {
                    if let Some(sid) = sender.object_id() {
                        if let Some(r) = s.regions.get_mut(&sid) {
                            apply_rect(sender, r, *x, *y, *width, *height, false);
                        }
                    }
                }
            }
            hlist![]
        })
}
