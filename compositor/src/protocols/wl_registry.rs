use app::{RegisteredModule, prelude::*};
use wayland::{WlDisplayRequest, WlRegistryRequest};

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
        .on(|_: &mut WlRegistryState, ev: &WlRegistryRequest| {
            let WlRegistryRequest::Bind {
                client_id,
                name,
                id,
                ..
            } = ev;
            println!("client {:?} bind name={} id={:?}", client_id, name, id);
            hlist![]
        })
}
