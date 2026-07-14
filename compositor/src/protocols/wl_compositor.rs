use app::{prelude::*, RegisteredModule, Start};
use wayland::{Interface, WlCompositor, WlCompositorRequest};

use crate::protocols::wl_registry::RegisterGlobal;
use crate::Compositor;

pub fn module<S>() -> impl RegisteredModule<Compositor, S> {
    Module::<Compositor, _, _>::new()
        .on(|_: &mut Compositor, _: &Start| -> Option<RegisterGlobal> {
            Some(RegisterGlobal {
                interface: WlCompositor::NAME,
                version: WlCompositor::VERSION,
            })
        })
        .on(|state: &mut Compositor, ev: &WlRegistryRequest| {
            let WlRegistryRequest::Bind {
                sender, name, id, ..
            } = ev;
            if let Some((_, interface, _)) = state.globals.iter().find(|(n, _, _)| n == name) {
                match *interface {
                    WlCompositor::NAME => {
                        sender.proxy.new_handle::<WlCompositor>(*id);
                    }
                    _ => {}
                }
            }
            hlist![]
        })
        .on(|_: &mut Compositor, ev: &WlCompositorRequest| {
            let _ = ev;
            hlist![]
        })
}
