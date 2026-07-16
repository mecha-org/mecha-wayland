use app::{RegisteredModule, Start, prelude::*};
use wayland::{Interface, WlCompositor, WlCompositorRequest, WlRegistryRequest};

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
        .on(|_: &mut Compositor, ev: &WlRegistryRequest| {
            let WlRegistryRequest::Bind {
                sender,
                id,
                interface,
                ..
            } = ev;
            // WORKAROUND: trusts client's interface string instead of resolving by server-side name.
            if interface.as_str() == WlCompositor::NAME {
                sender.proxy.new_handle::<WlCompositor>(*id);
            }
            hlist![]
        })
        .on(|_: &mut Compositor, ev: &WlCompositorRequest| {
            let _ = ev;
            hlist![]
        })
}
