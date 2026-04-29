use std::collections::HashMap;

use crate::object::Object;
use crate::{WlRegistry, WlRegistryGlobalEvent, WlRegistryGlobalRemoveEvent, WlRegistryHandler};

pub struct Registry {
    pub inner: WlRegistry,
    globals: HashMap<u32, (String, u32)>, // name → (interface, version)
}

impl Registry {
    pub fn new(id: u32) -> Self {
        Registry {
            inner: WlRegistry::new(id),
            globals: HashMap::new(),
        }
    }

    /// Returns `(name, version)` for the first global matching `iface`.
    pub fn find(&self, iface: &str) -> Option<(u32, u32)> {
        self.globals
            .iter()
            .find(|(_, (i, _))| i == iface)
            .map(|(name, (_, ver))| (*name, *ver))
    }
}

impl Object for Registry {
    fn object_id(&self) -> u32 {
        self.inner.object_id()
    }
}

impl WlRegistryHandler for Registry {
    fn on_global(&mut self, event: WlRegistryGlobalEvent) {
        tracing::info!(
            name = event.name,
            interface = %event.interface,
            version = event.version,
            "wl_registry::global"
        );
        self.globals
            .insert(event.name, (event.interface, event.version));
    }

    fn on_global_remove(&mut self, event: WlRegistryGlobalRemoveEvent) {
        tracing::debug!(name = event.name, "wl_registry::global_remove");
        self.globals.remove(&event.name);
    }
}
