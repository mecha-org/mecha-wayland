use app::{RegisteredModule, Start, prelude::*};
use std::collections::HashMap;
use wayland::{
    Handle, Interface, WlRegistryRequest, WlSurface, XdgPositionerRequest, XdgSurface,
    XdgSurfaceRequest, XdgToplevelRequest, XdgWmBase, XdgWmBaseRequest,
};

use crate::Compositor;
use crate::protocols::wl_registry::RegisterGlobal;
use crate::protocols::wl_surface::SurfaceRole;
use crate::rect::Rect;

#[derive(Debug, Default)]
pub struct XdgShellState {
    serial: u32,
    pub xdg_surfaces: HashMap<Handle<XdgSurface>, XdgSurfaceData>,
}

#[derive(Debug)]
pub struct XdgSurfaceData {
    pub wl_surface_id: Handle<WlSurface>,
}

impl XdgShellState {
    fn next_serial(&mut self) -> u32 {
        self.serial = self.serial.wrapping_add(1);
        self.serial
    }
}

pub fn module<S>() -> impl RegisteredModule<Compositor, S> {
    Module::<Compositor, _, _>::new()
        .on(|c: &mut Compositor, ev: &WlRegistryRequest| {
            let WlRegistryRequest::Bind {
                sender,
                id,
                interface,
                ..
            } = ev;
            // WORKAROUND: trusts client's interface string instead of resolving by server-side name.
            if interface.as_str() == XdgWmBase::NAME {
                sender.proxy.new_handle::<XdgWmBase>(*id);
            }
            hlist![]
        })
        .on(|_: &mut Compositor, _: &Start| -> Option<RegisterGlobal> {
            Some(RegisterGlobal {
                interface: XdgWmBase::NAME,
                version: XdgWmBase::VERSION,
            })
        })
        .on(|c: &mut Compositor, ev: &XdgWmBaseRequest| {
            match ev {
                XdgWmBaseRequest::Destroy { .. } => {}
                XdgWmBaseRequest::CreatePositioner { .. } => {}
                XdgWmBaseRequest::GetXdgSurface { id, surface, .. } => {
                    let wl_surface_id = surface.clone();
                    c.xdg_shell
                        .xdg_surfaces
                        .insert(id.clone(), XdgSurfaceData { wl_surface_id });
                    if let Some(surf) = c.surfaces.surfaces.get_mut(surface) {
                        let _ = surf.set_role(SurfaceRole::XdgSurface(id.clone()));
                    }
                }
                XdgWmBaseRequest::Pong { .. } => {}
            }
            hlist![]
        })
        .on(|c: &mut Compositor, ev: &XdgSurfaceRequest| {
            match ev {
                XdgSurfaceRequest::Destroy { sender } => {
                    if let Some(xdg) = c.xdg_shell.xdg_surfaces.remove(sender) {
                        c.surfaces.remove_from_stack(&xdg.wl_surface_id);
                        if let Some(surf) = c.surfaces.surfaces.get_mut(&xdg.wl_surface_id) {
                            surf.geometry = None;
                            surf.role = None;
                        }
                    }
                }
                XdgSurfaceRequest::GetToplevel { sender, id } => {
                    let serial = c.xdg_shell.next_serial();
                    sender.configure(serial);
                    id.configure(0, 0, &[]);
                    if let Some(xdg) = c.xdg_shell.xdg_surfaces.get(sender) {
                        c.surfaces.push_to_stack(&xdg.wl_surface_id);
                    }
                }
                XdgSurfaceRequest::GetPopup { .. } => {}
                XdgSurfaceRequest::SetWindowGeometry {
                    sender,
                    x,
                    y,
                    width,
                    height,
                } => {
                    let geom = Rect::new_sized_saturating(*x, *y, *width, *height);
                    if let Some(xdg) = c.xdg_shell.xdg_surfaces.get(sender) {
                        if let Some(surf) = c.surfaces.surfaces.get_mut(&xdg.wl_surface_id) {
                            surf.geometry = Some(geom);
                        }
                    }
                }
                XdgSurfaceRequest::AckConfigure { .. } => {}
            }
            hlist![]
        })
        .on(|_: &mut Compositor, _: &XdgToplevelRequest| hlist![])
        .on(|_: &mut Compositor, _: &XdgPositionerRequest| hlist![])
}
