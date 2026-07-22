use app::{RegisteredModule, Start, prelude::*};
use std::collections::HashMap;
use wayland::{
    DISPLAY_OBJECT_ID, Handle, Interface, WlDisplay, WlRegistryRequest, WlSurface,
    ZwlrLayerShellV1, ZwlrLayerShellV1Layer, ZwlrLayerShellV1Request, ZwlrLayerSurfaceV1,
    ZwlrLayerSurfaceV1Anchor, ZwlrLayerSurfaceV1KeyboardInteractivity, ZwlrLayerSurfaceV1Request,
};

use crate::Compositor;
use crate::protocols::wl_registry::RegisterGlobal;
use crate::protocols::wl_surface::{SurfaceCommitted, SurfaceRole};

fn layer_priority(layer: ZwlrLayerShellV1Layer) -> u32 {
    match layer {
        ZwlrLayerShellV1Layer::Background => 0,
        ZwlrLayerShellV1Layer::Bottom => 1,
        ZwlrLayerShellV1Layer::Top => 2,
        ZwlrLayerShellV1Layer::Overlay => 3,
    }
}

#[derive(Debug, Default)]
pub struct LayerShellState {
    serial: u32,
    pub layer_surfaces: HashMap<Handle<ZwlrLayerSurfaceV1>, LayerSurfaceData>,
    surface_to_layer: HashMap<Handle<WlSurface>, Handle<ZwlrLayerSurfaceV1>>,
}

#[derive(Debug, Clone)]
pub struct LayerSurfaceProperties {
    pub layer: ZwlrLayerShellV1Layer,
    pub size: (u32, u32),
    pub anchor: ZwlrLayerSurfaceV1Anchor,
    pub exclusive_zone: i32,
    pub margin_top: i32,
    pub margin_right: i32,
    pub margin_bottom: i32,
    pub margin_left: i32,
    pub keyboard_interactivity: ZwlrLayerSurfaceV1KeyboardInteractivity,
    pub exclusive_edge: ZwlrLayerSurfaceV1Anchor,
}

impl Default for LayerSurfaceProperties {
    fn default() -> Self {
        Self {
            layer: ZwlrLayerShellV1Layer::Background,
            size: (0, 0),
            anchor: ZwlrLayerSurfaceV1Anchor::empty(),
            exclusive_zone: 0,
            margin_top: 0,
            margin_right: 0,
            margin_bottom: 0,
            margin_left: 0,
            keyboard_interactivity: ZwlrLayerSurfaceV1KeyboardInteractivity::None,
            exclusive_edge: ZwlrLayerSurfaceV1Anchor::empty(),
        }
    }
}

impl LayerSurfaceProperties {
    fn needs_reconfigure(&self, other: &Self) -> bool {
        self.size != other.size
            || self.anchor != other.anchor
            || self.exclusive_zone != other.exclusive_zone
            || self.margin_top != other.margin_top
            || self.margin_right != other.margin_right
            || self.margin_bottom != other.margin_bottom
            || self.margin_left != other.margin_left
            || self.exclusive_edge != other.exclusive_edge
            || self.layer != other.layer
    }
}

#[derive(Debug)]
pub struct LayerSurfaceData {
    pub wl_surface_id: Handle<WlSurface>,
    pub current: LayerSurfaceProperties,
    pub pending: LayerSurfaceProperties,
    pub configured: bool,
    pub last_configure_serial: u32,
    pub acked_serial: Option<u32>,
    pub mapped: bool,
}

impl LayerSurfaceData {
    fn new(wl_surface_id: Handle<WlSurface>, layer: ZwlrLayerShellV1Layer) -> Self {
        let props = LayerSurfaceProperties {
            layer,
            ..Default::default()
        };
        Self {
            wl_surface_id,
            current: props.clone(),
            pending: props,
            configured: false,
            last_configure_serial: 0,
            acked_serial: None,
            mapped: false,
        }
    }
}

impl LayerShellState {
    fn next_serial(&mut self) -> u32 {
        self.serial = self.serial.wrapping_add(1);
        self.serial
    }

    pub fn keyboard_interactivity_for(
        &self,
        surface: &Handle<WlSurface>,
    ) -> Option<ZwlrLayerSurfaceV1KeyboardInteractivity> {
        let ls_handle = self.surface_to_layer.get(surface)?;
        self.layer_surfaces
            .get(ls_handle)
            .map(|ls| ls.current.keyboard_interactivity)
    }
}

fn post_error(handle: &Handle<impl Interface>, code: u32, msg: &str) {
    if let (Some(id), Some(d)) = (
        handle.object_id(),
        handle.proxy.get_handle::<WlDisplay>(DISPLAY_OBJECT_ID),
    ) {
        d.error(id, code, msg);
    }
}

pub fn module<S>() -> impl RegisteredModule<Compositor, S> {
    Module::<Compositor, _, _>::new()
        .on(|_c: &mut Compositor, ev: &WlRegistryRequest| {
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
                sender,
                id,
                surface,
                layer,
                namespace: _namespace,
                ..
            } = ev
            {
                let Some(surf) = c.surfaces.surfaces.get_mut(surface) else {
                    post_error(sender, 0, "invalid wl_surface");
                    return hlist![];
                };
                if surf
                    .set_role(SurfaceRole::LayerSurface(id.clone()))
                    .is_err()
                {
                    post_error(sender, 0, "wl_surface already has a different role");
                    return hlist![];
                }
                c.layer_shell
                    .layer_surfaces
                    .insert(id.clone(), LayerSurfaceData::new(surface.clone(), *layer));
                c.layer_shell
                    .surface_to_layer
                    .insert(surface.clone(), id.clone());
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
                        ls.pending.size = (*width, *height);
                    }
                }
                ZwlrLayerSurfaceV1Request::SetAnchor { sender, anchor } => {
                    if let Some(ls) = c.layer_shell.layer_surfaces.get_mut(sender) {
                        ls.pending.anchor = *anchor;
                    }
                }
                ZwlrLayerSurfaceV1Request::SetExclusiveZone { sender, zone } => {
                    if let Some(ls) = c.layer_shell.layer_surfaces.get_mut(sender) {
                        ls.pending.exclusive_zone = *zone;
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
                        ls.pending.margin_top = *top;
                        ls.pending.margin_right = *right;
                        ls.pending.margin_bottom = *bottom;
                        ls.pending.margin_left = *left;
                    }
                }
                ZwlrLayerSurfaceV1Request::SetKeyboardInteractivity {
                    sender,
                    keyboard_interactivity,
                } => {
                    if let Some(ls) = c.layer_shell.layer_surfaces.get_mut(sender) {
                        ls.current.keyboard_interactivity = *keyboard_interactivity;
                    }
                }
                ZwlrLayerSurfaceV1Request::GetPopup { .. } => {}
                ZwlrLayerSurfaceV1Request::AckConfigure { sender, serial } => {
                    if let Some(ls) = c.layer_shell.layer_surfaces.get_mut(sender) {
                        ls.acked_serial = Some(*serial);
                    }
                }
                ZwlrLayerSurfaceV1Request::Destroy { sender } => {
                    if let Some(ls) = c.layer_shell.layer_surfaces.remove(sender) {
                        c.layer_shell.surface_to_layer.remove(&ls.wl_surface_id);
                        c.surfaces.remove_from_stack(&ls.wl_surface_id);
                        if let Some(surf) = c.surfaces.surfaces.get_mut(&ls.wl_surface_id) {
                            surf.role = None;
                        }
                    }
                }
                ZwlrLayerSurfaceV1Request::SetLayer { sender, layer } => {
                    if let Some(ls) = c.layer_shell.layer_surfaces.get_mut(sender) {
                        ls.pending.layer = *layer;
                    }
                }
                ZwlrLayerSurfaceV1Request::SetExclusiveEdge { sender, edge } => {
                    if let Some(ls) = c.layer_shell.layer_surfaces.get_mut(sender) {
                        ls.pending.exclusive_edge = *edge;
                    }
                }
            }
            hlist![]
        })
        .on(|c: &mut Compositor, ev: &SurfaceCommitted| {
            let (layer_id, has_buffer) = match c.surfaces.surfaces.get(&ev.surface_id) {
                Some(surf) => {
                    let lid = match &surf.role {
                        Some(SurfaceRole::LayerSurface(id)) => Some(id.clone()),
                        _ => None,
                    };
                    (lid, surf.current.buffer.is_some())
                }
                None => (None, false),
            };

            let Some(layer_id) = layer_id else {
                return hlist![];
            };

            let (props_changed, configured, pending_size, pending_layer, current_layer) = {
                let Some(ls) = c.layer_shell.layer_surfaces.get(&layer_id) else {
                    return hlist![];
                };
                (
                    ls.pending.needs_reconfigure(&ls.current),
                    ls.configured,
                    ls.pending.size,
                    ls.pending.layer,
                    ls.current.layer,
                )
            };

            if !configured {
                let serial = c.layer_shell.next_serial();
                let Some(ls) = c.layer_shell.layer_surfaces.get_mut(&layer_id) else {
                    return hlist![];
                };
                layer_id.configure(serial, pending_size.0, pending_size.1);
                ls.current = ls.pending.clone();
                ls.last_configure_serial = serial;
                ls.configured = true;
            } else if props_changed {
                let serial = c.layer_shell.next_serial();
                let Some(ls) = c.layer_shell.layer_surfaces.get_mut(&layer_id) else {
                    return hlist![];
                };
                layer_id.configure(serial, pending_size.0, pending_size.1);
                ls.current = ls.pending.clone();
                ls.last_configure_serial = serial;
                if ls.mapped {
                    c.surfaces
                        .push_layer_surface(&ev.surface_id, layer_priority(pending_layer));
                }
            } else {
                let Some(ls) = c.layer_shell.layer_surfaces.get_mut(&layer_id) else {
                    return hlist![];
                };

                let acked = ls.acked_serial == Some(ls.last_configure_serial);

                if has_buffer && !ls.mapped {
                    if acked {
                        c.surfaces
                            .push_layer_surface(&ev.surface_id, layer_priority(current_layer));
                        ls.mapped = true;
                    } else {
                        post_error(&layer_id, 0, "buffer committed before ack_configure");
                    }
                } else if !has_buffer && ls.mapped {
                    c.surfaces.remove_from_stack(&ev.surface_id);
                    ls.mapped = false;
                    ls.configured = false;
                    ls.acked_serial = None;
                }
            }
            hlist![]
        })
}
