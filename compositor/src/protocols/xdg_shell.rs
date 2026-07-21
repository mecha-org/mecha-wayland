use app::{RegisteredModule, Start, prelude::*};
use std::collections::HashMap;
use wayland::{
    Handle, Interface, ObjectId, WlRegistryRequest, WlSurface, XdgPositionerRequest,
    XdgSurfaceRequest, XdgToplevelRequest, XdgWmBase, XdgWmBaseRequest,
};

use crate::Compositor;
use crate::protocols::wl_registry::RegisterGlobal;
use crate::protocols::wl_surface::SurfaceRole;
use crate::rect::Rect;

#[derive(Debug, Default)]
pub struct XdgShellState {
    serial: u32,
    pub xdg_surfaces: HashMap<ObjectId, XdgSurface>,
}

#[derive(Debug)]
pub struct XdgSurface {
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
                    if let Some(xdg_sid) = id.object_id() {
                        let wl_surface_id = surface.clone();
                        c.xdg_shell
                            .xdg_surfaces
                            .insert(xdg_sid, XdgSurface { wl_surface_id });
                        if let Some(surf) = c.surfaces.surfaces.get_mut(surface) {
                            let _ = surf.set_role(SurfaceRole::XdgSurface(xdg_sid));
                        }
                    }
                }
                XdgWmBaseRequest::Pong { .. } => {}
            }
            hlist![]
        })
        .on(|c: &mut Compositor, ev: &XdgSurfaceRequest| {
            match ev {
                XdgSurfaceRequest::Destroy { sender } => {
                    if let Some(xdg_sid) = sender.object_id() {
                        if let Some(xdg) = c.xdg_shell.xdg_surfaces.remove(&xdg_sid) {
                            c.surfaces.remove_from_stack(&xdg.wl_surface_id);
                            if let Some(surf) = c.surfaces.surfaces.get_mut(&xdg.wl_surface_id) {
                                surf.geometry = None;
                                surf.role = None;
                            }
                        }
                    }
                }
                XdgSurfaceRequest::GetToplevel { sender, id } => {
                    let serial = c.xdg_shell.next_serial();
                    sender.configure(serial);
                    id.configure(0, 0, &[]);
                    if let Some(xdg_sid) = sender.object_id() {
                        if let Some(xdg) = c.xdg_shell.xdg_surfaces.get(&xdg_sid) {
                            c.surfaces.push_to_stack(&xdg.wl_surface_id);
                        }
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
                    if let Some(xdg_sid) = sender.object_id() {
                        if let Some(xdg) = c.xdg_shell.xdg_surfaces.get(&xdg_sid) {
                            if let Some(surf) = c.surfaces.surfaces.get_mut(&xdg.wl_surface_id) {
                                surf.geometry = Some(geom);
                            }
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
