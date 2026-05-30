use std::collections::HashMap;

use app::Event;

use crate::proto::Handle;
use crate::{SharedConnection, WaylandRawEvent, parse, send};

pub type Layer = crate::proto::zwlr_layer_shell_v1::ZwlrLayerShellV1Layer;
pub type Anchor = crate::proto::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1Anchor;
pub type KeyboardInteractivity = crate::proto::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1KeyboardInteractivity;

#[derive(Debug)]
pub enum LayerSurfaceEvent {
    Configured {
        id: u32,
        serial: u32,
        width: u32,
        height: u32,
    },
    Closed {
        id: u32,
    },
}

impl Event for LayerSurfaceEvent {}

// ── ZwlrLayerShellV1 ─────────────────────────────────────────────────────────

pub struct ZwlrLayerShellV1 {
    conn: SharedConnection,
    handle: Handle<crate::proto::zwlr_layer_shell_v1::ZwlrLayerShellV1>,
}

impl ZwlrLayerShellV1 {
    pub fn new(conn: SharedConnection) -> Self {
        Self {
            conn,
            handle: Handle::new(0),
        }
    }

    pub fn set_id(&mut self, id: u32) {
        self.handle = Handle::new(id);
    }

    pub fn get_layer_surface(
        &self,
        surface_id: u32,
        output_id: u32,
        layer: Layer,
        namespace: &str,
    ) -> u32 {
        let layer_surface_id = self.conn.borrow_mut().alloc_id();
        let output = if output_id == 0 {
            None
        } else {
            Some(output_id)
        };
        send(
            &self.conn,
            &self.handle,
            &crate::proto::zwlr_layer_shell_v1::request::GetLayerSurface {
                id: layer_surface_id,
                surface: surface_id,
                output,
                layer,
                namespace: namespace.to_string(),
            },
        );
        layer_surface_id
    }
}

// ── ZwlrLayerSurfaceV1 ────────────────────────────────────────────────────────

pub struct LayerSurfaceState {
    pub closed: bool,
    pub width: u32,
    pub height: u32,
}

pub struct ZwlrLayerSurfaceV1 {
    conn: SharedConnection,
    surfaces: HashMap<u32, LayerSurfaceState>,
}

impl ZwlrLayerSurfaceV1 {
    pub fn new(conn: SharedConnection) -> Self {
        Self {
            conn,
            surfaces: HashMap::new(),
        }
    }

    pub fn register(&mut self, id: u32) {
        self.surfaces.insert(
            id,
            LayerSurfaceState {
                closed: false,
                width: 0,
                height: 0,
            },
        );
    }

    pub fn set_size(&self, id: u32, width: u32, height: u32) {
        let h = Handle::<crate::proto::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1>::new(id);
        send(
            &self.conn,
            &h,
            &crate::proto::zwlr_layer_surface_v1::request::SetSize { width, height },
        );
    }

    pub fn set_anchor(&self, id: u32, anchor: Anchor) {
        let h = Handle::<crate::proto::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1>::new(id);
        send(
            &self.conn,
            &h,
            &crate::proto::zwlr_layer_surface_v1::request::SetAnchor {
                anchor,
            },
        );
    }

    pub fn set_exclusive_zone(&self, id: u32, zone: i32) {
        let h = Handle::<crate::proto::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1>::new(id);
        send(
            &self.conn,
            &h,
            &crate::proto::zwlr_layer_surface_v1::request::SetExclusiveZone { zone },
        );
    }

    pub fn set_margin(&self, id: u32, top: i32, right: i32, bottom: i32, left: i32) {
        let h = Handle::<crate::proto::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1>::new(id);
        send(
            &self.conn,
            &h,
            &crate::proto::zwlr_layer_surface_v1::request::SetMargin {
                top,
                right,
                bottom,
                left,
            },
        );
    }

    pub fn set_keyboard_interactivity(&self, id: u32, interactivity: KeyboardInteractivity) {
        let h = Handle::<crate::proto::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1>::new(id);
        send(
            &self.conn,
            &h,
            &crate::proto::zwlr_layer_surface_v1::request::SetKeyboardInteractivity {
                keyboard_interactivity: interactivity,
            },
        );
    }

    pub fn ack_configure(&self, id: u32, serial: u32) {
        let h = Handle::<crate::proto::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1>::new(id);
        send(
            &self.conn,
            &h,
            &crate::proto::zwlr_layer_surface_v1::request::AckConfigure { serial },
        );
    }

    pub fn process(&mut self, raw: &WaylandRawEvent) -> Option<LayerSurfaceEvent> {
        let state = self.surfaces.get_mut(&raw.sender_id)?;
        let id = raw.sender_id;
        let ev =
            if let Some(e) = parse::<crate::proto::zwlr_layer_surface_v1::event::Configure>(raw) {
                state.width = e.width;
                state.height = e.height;
                LayerSurfaceEvent::Configured {
                    id,
                    serial: e.serial,
                    width: e.width,
                    height: e.height,
                }
            } else if parse::<crate::proto::zwlr_layer_surface_v1::event::Closed>(raw).is_some() {
                state.closed = true;
                LayerSurfaceEvent::Closed { id }
            } else {
                return None;
            };
        println!("[zwlr_layer_surface] {:?}", ev);
        Some(ev)
    }
}

pub fn module<AppState>() -> impl app::RegisteredModule<ZwlrLayerSurfaceV1, AppState> {
    app::Module::<ZwlrLayerSurfaceV1, _, _>::new()
        .on(|ls: &mut ZwlrLayerSurfaceV1, ev: &crate::WaylandRawEvent| ls.process(ev))
}
