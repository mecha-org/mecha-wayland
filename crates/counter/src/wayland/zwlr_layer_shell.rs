use std::collections::HashMap;

use app::event::Event;

use crate::wayland::{SharedConnection, WaylandRawEvent};
use crate::wire::MessageReader;

// ── Layer enum ────────────────────────────────────────────────────────────────
pub const LAYER_BACKGROUND: u32 = 0;
pub const LAYER_BOTTOM: u32 = 1;
pub const LAYER_TOP: u32 = 2;
pub const LAYER_OVERLAY: u32 = 3;

// ── Anchor bitfield ───────────────────────────────────────────────────────────
pub const ANCHOR_TOP: u32 = 1;
pub const ANCHOR_BOTTOM: u32 = 2;
pub const ANCHOR_LEFT: u32 = 4;
pub const ANCHOR_RIGHT: u32 = 8;

// ── ZwlrLayerShellV1 ─────────────────────────────────────────────────────────

pub struct ZwlrLayerShellV1 {
    conn: SharedConnection,
    pub id: u32,
}

impl ZwlrLayerShellV1 {
    pub fn new(conn: SharedConnection) -> Self {
        Self { conn, id: 0 }
    }

    pub fn set_id(&mut self, id: u32) {
        self.id = id;
    }

    // opcode 0: get_layer_surface(id: new_id, surface: object, output: object,
    //                             layer: uint, namespace: string) -> layer_surface_id
    // Pass output_id = 0 (null object) for any output.
    pub fn get_layer_surface(
        &self,
        surface_id: u32,
        output_id: u32,
        layer: u32,
        namespace: &str,
    ) -> u32 {
        let mut conn = self.conn.borrow_mut();
        let layer_surface_id = conn.alloc_id();
        conn.message_builder(self.id, 0)
            .write_u32(layer_surface_id)
            .write_u32(surface_id)
            .write_u32(output_id)
            .write_u32(layer)
            .write_string(namespace)
            .build();
        layer_surface_id
    }
}

// ── ZwlrLayerSurfaceV1 ────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum LayerSurfaceEvent {
    Configured { id: u32, serial: u32, width: u32, height: u32 },
    Closed { id: u32 },
}

impl Event for LayerSurfaceEvent {}

pub struct LayerSurfaceState {
    pub closed: bool,
    pub width: u32,
    pub height: u32,
}

pub struct ZwlrLayerSurfaceV1 {
    conn: SharedConnection,
    pub surfaces: HashMap<u32, LayerSurfaceState>,
}

impl ZwlrLayerSurfaceV1 {
    pub fn new(conn: SharedConnection) -> Self {
        Self { conn, surfaces: HashMap::new() }
    }

    pub fn register(&mut self, id: u32) {
        self.surfaces.insert(id, LayerSurfaceState {
            closed: false,
            width: 0,
            height: 0,
        });
    }

    // opcode 0: set_size(width: uint, height: uint)
    pub fn set_size(&self, id: u32, width: u32, height: u32) {
        self.conn
            .borrow_mut()
            .message_builder(id, 0)
            .write_u32(width)
            .write_u32(height)
            .build();
    }

    // opcode 1: set_anchor(anchor: uint)
    pub fn set_anchor(&self, id: u32, anchor: u32) {
        self.conn
            .borrow_mut()
            .message_builder(id, 1)
            .write_u32(anchor)
            .build();
    }

    // opcode 2: set_exclusive_zone(zone: int)
    pub fn set_exclusive_zone(&self, id: u32, zone: i32) {
        self.conn
            .borrow_mut()
            .message_builder(id, 2)
            .write_i32(zone)
            .build();
    }

    // opcode 3: set_margin(top: int, right: int, bottom: int, left: int)
    pub fn set_margin(&self, id: u32, top: i32, right: i32, bottom: i32, left: i32) {
        self.conn
            .borrow_mut()
            .message_builder(id, 3)
            .write_i32(top)
            .write_i32(right)
            .write_i32(bottom)
            .write_i32(left)
            .build();
    }

    // opcode 6: ack_configure(serial: uint)
    pub fn ack_configure(&self, id: u32, serial: u32) {
        self.conn
            .borrow_mut()
            .message_builder(id, 6)
            .write_u32(serial)
            .build();
    }

    pub fn process(&mut self, ev: &WaylandRawEvent) -> Option<LayerSurfaceEvent> {
        let state = self.surfaces.get_mut(&ev.sender_id)?;
        let id = ev.sender_id;
        let mut fds = vec![];
        let mut r = MessageReader::new(&ev.body, &mut fds);
        let event = match ev.opcode {
            0 => {
                let serial = r.read_u32().unwrap_or(0);
                let width = r.read_u32().unwrap_or(0);
                let height = r.read_u32().unwrap_or(0);
                state.width = width;
                state.height = height;
                LayerSurfaceEvent::Configured { id, serial, width, height }
            }
            1 => {
                state.closed = true;
                LayerSurfaceEvent::Closed { id }
            }
            _ => return None,
        };
        println!("[zwlr_layer_surface] {:?}", event);
        Some(event)
    }
}

#[macro_export]
macro_rules! register_zwlr_layer_surface {
    () => {
        app::module::Module::<crate::wayland::ZwlrLayerSurfaceV1>::new()
            .processor(|ls: &mut crate::wayland::ZwlrLayerSurfaceV1, ev: &crate::wayland::WaylandRawEvent| {
                ls.process(ev)
            })
    };
}
