use app::{RegisteredModule, Start, prelude::*};
use wayland::{Interface, WlCompositor, WlCompositorRequest};

use crate::Compositor;
use crate::protocols::wl_registry::RegisterGlobal;

pub fn module<S>() -> impl RegisteredModule<Compositor, S> {
    Module::<Compositor, _, _>::new()
        .on(|_: &mut Compositor, _: &Start| -> Option<RegisterGlobal> {
            Some(RegisterGlobal {
                interface: WlCompositor::NAME,
                version: WlCompositor::VERSION,
            })
        })
        .on(|_: &mut Compositor, ev: &WlCompositorRequest| {
            println!("wl_compositor: {:?}", ev);
            hlist![]
        })
}
