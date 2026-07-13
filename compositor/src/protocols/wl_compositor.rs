use app::{RegisteredModule, Start, prelude::*};
use wayland::{Interface, WlCompositor, WlCompositorRequest};

use crate::Compositor;
use crate::protocols::wl_registry::RegisterGlobal;
use crate::protocols::wl_surface::Surface;

pub fn module<S>() -> impl RegisteredModule<Compositor, S> {
    Module::<Compositor, _, _>::new()
        .on(|_: &mut Compositor, _: &Start| -> Option<RegisterGlobal> {
            Some(RegisterGlobal {
                interface: WlCompositor::NAME,
                version: WlCompositor::VERSION,
            })
        })
        .on(|state: &mut Compositor, ev: &WlCompositorRequest| {
            match ev {
                WlCompositorRequest::CreateSurface { id, .. } => {
                    let surface_id = id.object_id().expect("live surface");
                    state.surfaces.surfaces.insert(surface_id, Surface::new());
                }
                WlCompositorRequest::CreateRegion { .. } => {}
                WlCompositorRequest::Release { .. } => {}
            }
            hlist![]
        })
}
