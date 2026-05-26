use std::collections::HashMap;

use app::event::Event;

use crate::proto::wl_surface as proto;
use crate::proto::Handle;
use crate::{SharedConnection, WaylandRawEvent, parse, send};

#[derive(Debug)]
pub enum SurfaceEvent {
    Enter { id: u32, output: u32 },
    Leave { id: u32, output: u32 },
}

impl Event for SurfaceEvent {}

pub struct WlSurface {
    conn: SharedConnection,
    surface_ids: HashMap<u32, ()>,
}

impl WlSurface {
    pub fn new(conn: SharedConnection) -> Self {
        Self { conn, surface_ids: HashMap::new() }
    }

    pub fn register(&mut self, id: u32) {
        self.surface_ids.insert(id, ());
    }

    pub fn attach(&self, id: u32, buffer_id: u32, x: i32, y: i32) {
        let h = Handle::<proto::WlSurface>::new(id);
        let buffer = if buffer_id == 0 { None } else { Some(buffer_id) };
        send(&self.conn, &h, &proto::request::Attach { buffer, x, y });
    }

    pub fn damage(&self, id: u32, x: i32, y: i32, width: i32, height: i32) {
        let h = Handle::<proto::WlSurface>::new(id);
        send(&self.conn, &h, &proto::request::Damage { x, y, width, height });
    }

    pub fn frame(&self, surface_id: u32) -> u32 {
        let cb_id = self.conn.borrow_mut().alloc_id();
        let h = Handle::<proto::WlSurface>::new(surface_id);
        send(&self.conn, &h, &proto::request::Frame { callback: cb_id });
        cb_id
    }

    pub fn commit(&self, id: u32) {
        let h = Handle::<proto::WlSurface>::new(id);
        send(&self.conn, &h, &proto::request::Commit);
    }

    pub fn process(&mut self, raw: &WaylandRawEvent) -> Option<SurfaceEvent> {
        if !self.surface_ids.contains_key(&raw.sender_id) {
            return None;
        }
        let id = raw.sender_id;
        let ev = if let Some(e) = parse::<proto::event::Enter>(raw) {
            SurfaceEvent::Enter { id, output: e.output }
        } else if let Some(e) = parse::<proto::event::Leave>(raw) {
            SurfaceEvent::Leave { id, output: e.output }
        } else {
            return None;
        };
        println!("[wl_surface] {:?}", ev);
        Some(ev)
    }
}

#[macro_export]
macro_rules! register_wl_surface {
    () => {
        app::module::Module::<$crate::WlSurface>::new().processor(
            |s: &mut $crate::WlSurface, ev: &$crate::WaylandRawEvent| s.process(ev),
        )
    };
}
