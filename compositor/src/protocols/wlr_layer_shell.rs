use app::{RegisteredModule, Start, prelude::*};
use std::collections::HashMap;
use wayland::{
    Handle, Interface, WlRegistryRequest, WlSurface, ZwlrLayerShellV1, ZwlrLayerShellV1Layer,
    ZwlrLayerShellV1Request, ZwlrLayerSurfaceV1, ZwlrLayerSurfaceV1Anchor,
    ZwlrLayerSurfaceV1KeyboardInteractivity, ZwlrLayerSurfaceV1Request,
};

use crate::Compositor;
use crate::protocols::wl_registry::RegisterGlobal;
use crate::protocols::wl_surface::{SurfaceCommitted, SurfaceRole};

#[derive(Debug, Default)]
pub struct LayerShellState {
    serial: u32,
    pub layer_surfaces: HashMap<Handle<ZwlrLayerSurfaceV1>, LayerSurfaceData>,
}

#[derive(Debug)]
pub struct LayerSurfaceData {
    pub wl_surface_id: Handle<WlSurface>,
    pub layer: ZwlrLayerShellV1Layer,
    pub pending_size: (u32, u32),
    pub anchor: ZwlrLayerSurfaceV1Anchor,
    pub exclusive_zone: i32,
    pub margin_top: i32,
    pub margin_right: i32,
    pub margin_bottom: i32,
    pub margin_left: i32,
    pub keyboard_interactivity: ZwlrLayerSurfaceV1KeyboardInteractivity,
    pub exclusive_edge: ZwlrLayerSurfaceV1Anchor,
    pub configured: bool,
    pub mapped: bool,
}

impl LayerSurfaceData {
    fn new(wl_surface_id: Handle<WlSurface>, layer: ZwlrLayerShellV1Layer) -> Self {
        Self {
            wl_surface_id,
            layer,
            pending_size: (0, 0),
            anchor: ZwlrLayerSurfaceV1Anchor::empty(),
            exclusive_zone: 0,
            margin_top: 0,
            margin_right: 0,
            margin_bottom: 0,
            margin_left: 0,
            keyboard_interactivity: ZwlrLayerSurfaceV1KeyboardInteractivity::None,
            exclusive_edge: ZwlrLayerSurfaceV1Anchor::empty(),
            configured: false,
            mapped: false,
        }
    }
}

impl LayerShellState {
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
            if interface.as_str() == ZwlrLayerShellV1::NAME {
                sender.proxy.new_handle::<ZwlrLayerShellV1>(*id);
            }
            hlist![]
        })
        .on(|_: &mut Compositor, _: &Start| -> Option<RegisterGlobal> {
            Some(RegisterGlobal {
                interface: ZwlrLayerShellV1::NAME,
                version: ZwlrLayerShellV1::VERSION,
            })
        })
        .on(|c: &mut Compositor, ev: &ZwlrLayerShellV1Request| {
            if let ZwlrLayerShellV1Request::GetLayerSurface {
                id,
                surface,
                layer,
                namespace: _namespace,
                ..
            } = ev
            {
                c.layer_shell
                    .layer_surfaces
                    .insert(id.clone(), LayerSurfaceData::new(surface.clone(), *layer));
                if let Some(surf) = c.surfaces.surfaces.get_mut(surface) {
                    let _ = surf.set_role(SurfaceRole::LayerSurface(id.clone()));
                }
            }
            hlist![]
        })
        .on(|c: &mut Compositor, ev: &ZwlrLayerSurfaceV1Request| {
            match ev {
                ZwlrLayerSurfaceV1Request::SetSize {
                    sender,
                    width,
                    height,
                } => {
                    if let Some(ls) = c.layer_shell.layer_surfaces.get_mut(sender) {
                        ls.pending_size = (*width, *height);
                    }
                }
                ZwlrLayerSurfaceV1Request::SetAnchor { sender, anchor } => {
                    if let Some(ls) = c.layer_shell.layer_surfaces.get_mut(sender) {
                        ls.anchor = *anchor;
                    }
                }
                ZwlrLayerSurfaceV1Request::SetExclusiveZone { sender, zone } => {
                    if let Some(ls) = c.layer_shell.layer_surfaces.get_mut(sender) {
                        ls.exclusive_zone = *zone;
                    }
                }
                ZwlrLayerSurfaceV1Request::SetMargin {
                    sender,
                    top,
                    right,
                    bottom,
                    left,
                } => {
                    if let Some(ls) = c.layer_shell.layer_surfaces.get_mut(sender) {
                        ls.margin_top = *top;
                        ls.margin_right = *right;
                        ls.margin_bottom = *bottom;
                        ls.margin_left = *left;
                    }
                }
                ZwlrLayerSurfaceV1Request::SetKeyboardInteractivity {
                    sender,
                    keyboard_interactivity,
                } => {
                    if let Some(ls) = c.layer_shell.layer_surfaces.get_mut(sender) {
                        ls.keyboard_interactivity = *keyboard_interactivity;
                    }
                }
                ZwlrLayerSurfaceV1Request::GetPopup { .. } => {}
                ZwlrLayerSurfaceV1Request::AckConfigure { .. } => {}
                ZwlrLayerSurfaceV1Request::Destroy { sender } => {
                    if let Some(ls) = c.layer_shell.layer_surfaces.remove(sender) {
                        c.surfaces.remove_from_stack(&ls.wl_surface_id);
                        if let Some(surf) = c.surfaces.surfaces.get_mut(&ls.wl_surface_id) {
                            surf.role = None;
                        }
                    }
                }
                ZwlrLayerSurfaceV1Request::SetLayer { sender, layer } => {
                    if let Some(ls) = c.layer_shell.layer_surfaces.get_mut(sender) {
                        ls.layer = *layer;
                    }
                }
                ZwlrLayerSurfaceV1Request::SetExclusiveEdge { sender, edge } => {
                    if let Some(ls) = c.layer_shell.layer_surfaces.get_mut(sender) {
                        ls.exclusive_edge = *edge;
                    }
                }
            }
            hlist![]
        })
        .on(|c: &mut Compositor, ev: &SurfaceCommitted| {
            let layer_id = match c.surfaces.surfaces.get(&ev.surface_id) {
                Some(surf) => match &surf.role {
                    Some(SurfaceRole::LayerSurface(id)) => Some(id.clone()),
                    _ => None,
                },
                None => None,
            };

            let Some(layer_id) = layer_id else {
                return hlist![];
            };

            if let Some(ls) = c.layer_shell.layer_surfaces.get_mut(&layer_id) {
                if !ls.configured {
                    let serial = c.layer_shell.serial;
                    c.layer_shell.serial = c.layer_shell.serial.wrapping_add(1);
                    let (w, h) = ls.pending_size;
                    layer_id.configure(serial, w, h);
                    ls.configured = true;
                } else {
                    let has_buffer = c
                        .surfaces
                        .surfaces
                        .get(&ev.surface_id)
                        .and_then(|s| s.current.buffer)
                        .is_some();

                    if has_buffer && !ls.mapped {
                        c.surfaces.push_to_stack(&ev.surface_id);
                        ls.mapped = true;
                    } else if !has_buffer && ls.mapped {
                        c.surfaces.remove_from_stack(&ev.surface_id);
                        ls.mapped = false;
                        ls.configured = false;
                    }
                }
            }
            hlist![]
        })
}
