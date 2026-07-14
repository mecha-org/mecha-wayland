use app::{prelude::*, RegisteredModule, Start};
use wayland::{
    Interface,
    XdgPositionerRequest, XdgSurfaceRequest,
    XdgToplevelRequest, XdgWmBase, XdgWmBaseRequest,
};

use crate::protocols::wl_registry::RegisterGlobal;

#[derive(Debug, Default, State)]
pub struct XdgShellState {
    serial: u32,
}

impl XdgShellState {
    fn next_serial(&mut self) -> u32 {
        self.serial = self.serial.wrapping_add(1);
        self.serial
    }
}

pub fn module<S>() -> impl RegisteredModule<XdgShellState, S> {
    Module::<XdgShellState, _, _>::new()
        .on(|_: &mut XdgShellState, _: &Start| -> Option<RegisterGlobal> {
            Some(RegisterGlobal {
                interface: XdgWmBase::NAME,
                version: XdgWmBase::VERSION,
            })
        })
        .on(|_: &mut XdgShellState, ev: &XdgWmBaseRequest| {
            match ev {
                XdgWmBaseRequest::Destroy { .. } => {}
                XdgWmBaseRequest::CreatePositioner { .. } => {}
                XdgWmBaseRequest::GetXdgSurface { .. } => {}
                XdgWmBaseRequest::Pong { .. } => {}
            }
            hlist![]
        })
        .on(|state: &mut XdgShellState, ev: &XdgSurfaceRequest| {
            match ev {
                XdgSurfaceRequest::Destroy { .. } => {}
                XdgSurfaceRequest::GetToplevel { sender, id } => {
                    let serial = state.next_serial();
                    sender.configure(serial);
                    id.configure(0, 0, &[]);
                }
                XdgSurfaceRequest::GetPopup { .. } => {}
                XdgSurfaceRequest::SetWindowGeometry { .. } => {}
                XdgSurfaceRequest::AckConfigure { .. } => {}
            }
            hlist![]
        })
        .on(|_: &mut XdgShellState, _: &XdgToplevelRequest| {
            hlist![]
        })
        .on(|_: &mut XdgShellState, _: &XdgPositionerRequest| {
            hlist![]
        })
}
