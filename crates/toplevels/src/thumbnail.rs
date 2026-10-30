//! `phosh_private` thumbnail views: the compositor draws a toplevel's
//! live contents into a region of one of our layer surfaces.

use wayland::prelude::*;

use crate::{ToplevelId, Toplevels};

/// The oldest `phosh_private` that has `add_thumbnail_view`.
pub(crate) const MIN_VERSION: u32 = 8;

/// One live view of a toplevel on a layer surface. Not `Copy`: give it
/// back with [`Thumbnail::destroy`]. Dropping it without `destroy` leaks
/// the server object, which the compositor keeps drawing.
///
/// The layer surface must outlive the thumbnail: destroy every thumbnail
/// on a layer surface before destroying the surface. Destroying a
/// thumbnail after its toplevel closed is fine.
#[derive(Debug, PartialEq, Eq)]
pub struct Thumbnail(PhoshPrivateThumbnail);

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
    /// Whether the compositor offers `phosh_private` at version 8 or
    /// later, which is when [`Toplevels::add_thumbnail`] can work.
    pub fn thumbnails_available(&self) -> bool {
        self.phosh.is_some()
    }

    /// Ask the compositor to draw `id` into `layer`. `None` when
    /// thumbnails are unavailable or `id` has already closed.
    pub fn add_thumbnail(
        &self,
        wl: &mut Wayland,
        id: ToplevelId,
        layer: ZwlrLayerSurfaceV1,
    ) -> Option<Thumbnail> {
        let phosh = self.phosh?;
        if !self.holds(id) {
            return None;
        }
        Some(Thumbnail(phosh.add_thumbnail_view(wl, id.handle(), layer)))
    }
}
