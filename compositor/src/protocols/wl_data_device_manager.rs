use app::{RegisteredModule, Start, prelude::*};
use wayland::{
    Handle, Interface, WlDataDevice, WlDataDeviceManager, WlDataDeviceManagerRequest, WlDataSource,
};

use crate::{Compositor, protocols::wl_registry::RegisterGlobal};

#[derive(State)]
pub struct WlDataDeviceManagerState {
    pub data_sources: Vec<Handle<WlDataSource>>,
    pub data_devices: Vec<Handle<WlDataDevice>>,
}

impl WlDataDeviceManagerState {
    pub fn new() -> Self {
        Self {
            data_sources: Vec::new(),
            data_devices: Vec::new(),
        }
    }
}

pub fn module<S>() -> impl RegisteredModule<Compositor, S> {
    Module::<Compositor, _, _>::new()
        .on(|_: &mut Compositor, _: &Start| -> Option<RegisterGlobal> {
            Some(RegisterGlobal {
                interface: WlDataDeviceManager::NAME,
                version: WlDataDeviceManager::VERSION,
            })
        })
        // Requests from the clients
        .on(
            |compositor: &mut Compositor, ev: &WlDataDeviceManagerRequest| match ev {
                WlDataDeviceManagerRequest::CreateDataSource { id, .. } => {
                    compositor.data_device_manager.data_sources.push(id.clone());
                }
                WlDataDeviceManagerRequest::GetDataDevice { id, .. } => {
                    compositor.data_device_manager.data_devices.push(id.clone());
                }
                WlDataDeviceManagerRequest::Release { .. } => {
                    compositor.data_device_manager.data_sources.clear();
                    compositor.data_device_manager.data_devices.clear();
                }
            },
        )
}
