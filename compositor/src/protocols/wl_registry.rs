use app::{RegisteredModule, prelude::*};
use wayland::{Interface, WlCompositor, WlDisplayRequest, WlRegistryRequest, WlShm, WlShmFormat, XdgWmBase};

#[derive(Debug, State)]
pub struct WlRegistryState {
    globals: Vec<(u32, &'static str, u32)>,
    next_name: u32,
}

impl WlRegistryState {
    pub fn new() -> Self {
        Self {
            globals: vec![],
            next_name: 1,
        }
    }

    fn register(&mut self, interface: &'static str, version: u32) {
        let name = self.next_name;
        self.next_name += 1;
        self.globals.push((name, interface, version));
    }
}

#[derive(Debug)]
pub struct RegisterGlobal {
    pub interface: &'static str,
    pub version: u32,
}
impl Event for RegisterGlobal {}

pub fn module<S>() -> impl RegisteredModule<WlRegistryState, S> {
    Module::<WlRegistryState, _, _>::new()
        .on(|state: &mut WlRegistryState, ev: &RegisterGlobal| {
            state.register(ev.interface, ev.version);
            hlist![]
        })
        .on(|state: &mut WlRegistryState, ev: &WlDisplayRequest| {
            if let WlDisplayRequest::GetRegistry { registry, .. } = ev {
                for &(name, interface, version) in state.globals.iter() {
                    registry.global(name, interface, version);
                }
            }
            hlist![]
        })
        .on(|state: &mut WlRegistryState, ev: &WlRegistryRequest| {
            let WlRegistryRequest::Bind { sender, name, id, .. } = ev;
            if let Some((_, interface, _)) =
                state.globals.iter().find(|(n, _, _)| n == name)
            {
                match *interface {
                    WlShm::NAME => {
                        let handle = sender.proxy.new_handle::<WlShm>(*id);
                        handle.format(WlShmFormat::Argb8888);
                        handle.format(WlShmFormat::Xrgb8888);
                    }
                    WlCompositor::NAME => {
                        sender.proxy.new_handle::<WlCompositor>(*id);
                    }
                    XdgWmBase::NAME => {
                        sender.proxy.new_handle::<XdgWmBase>(*id);
                    }
                    _ => {}
                }
            }
            hlist![]
        })
}
