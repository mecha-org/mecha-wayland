//! `mx_thumbnail_view_v1`: the compositor draws a toplevel's live contents
//! into a region of one of our layer surfaces.

use wayland::prelude::*;

use crate::{ToplevelId, Toplevels};

/// One live view of a toplevel on a layer surface. Not `Copy`: give it
/// back with [`Thumbnail::destroy`]. Dropping it without `destroy` leaks
/// the server object, which the compositor keeps drawing.
///
/// The layer surface must outlive the thumbnail: destroy every thumbnail
/// on a layer surface before destroying the surface. Destroying a
/// thumbnail after its toplevel closed is fine.
#[derive(Debug, PartialEq, Eq)]
pub struct Thumbnail(MxThumbnailViewV1);

impl Thumbnail {
    /// Place the view: top-left at (`x`, `y`) in the layer surface's
    /// local coordinates, its size multiplied by `scale`, drawn with
    /// opacity `alpha`.
    ///
    /// The compositor double-buffers this: it takes effect on the layer
    /// surface's next commit, so a caller that animates the view must
    /// also commit that surface each frame.
    pub fn set(&self, wl: &mut Wayland, x: i32, y: i32, scale: f32, alpha: f32) {
        self.0.set_properties(wl, x, y, scale, alpha);
    }

    pub fn destroy(self, wl: &mut Wayland) {
        self.0.destroy(wl);
    }
}

impl Toplevels {
    /// Whether the compositor offers `mx_thumbnail_view_manager_v1`, which
    /// is when [`Toplevels::add_thumbnail`] can work.
    pub fn thumbnails_available(&self) -> bool {
        self.thumbnail_manager.is_some()
    }

    /// Ask the compositor to draw `id` into `layer`. `None` when
    /// `mx_thumbnail_view_manager_v1` was not offered or `id` is already
    /// closed.
    pub fn add_thumbnail(
        &self,
        wl: &mut Wayland,
        id: ToplevelId,
        layer: ZwlrLayerSurfaceV1,
    ) -> Option<Thumbnail> {
        let manager = self.thumbnail_manager?;
        if !self.holds(id) {
            return None;
        }
        Some(Thumbnail(manager.get_thumbnail(wl, id.handle(), layer)))
    }
}
