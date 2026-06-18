use std::collections::HashMap;

use app::Event;

use crate::proto::Handle;
use crate::proto::ext_session_lock_surface_v1 as proto;
use crate::{SharedConnection, WaylandRawEvent, parse, send};

#[derive(Debug, Clone)]
pub enum ExtSessionLockSurfaceV1Event {
    Configure {
        id: u32,
        serial: u32,
        width: u32,
        height: u32,
    },
}

impl Event for ExtSessionLockSurfaceV1Event {}

pub struct ExtSessionLockSurfaceState {
    pub width: u32,
    pub height: u32,
}

pub struct ExtSessionLockSurfaceV1 {
    conn: SharedConnection,
    surfaces: HashMap<u32, ExtSessionLockSurfaceState>,
}

impl ExtSessionLockSurfaceV1 {
    pub fn new(conn: SharedConnection) -> Self {
        Self {
            conn,
            surfaces: HashMap::new(),
        }
    }

    pub fn register(&mut self, id: u32) {
        self.surfaces.insert(
            id,
            ExtSessionLockSurfaceState {
                width: 0,
                height: 0,
            },
        );
    }

    pub fn destroy(&mut self, id: u32) {
        let h = Handle::<proto::ExtSessionLockSurfaceV1>::new(id);
        send(&self.conn, &h, &proto::request::Destroy);
        self.surfaces.remove(&id);
    }

    pub fn ack_configure(&self, id: u32, serial: u32) {
        let h = Handle::<proto::ExtSessionLockSurfaceV1>::new(id);
        send(
            &self.conn,
            &h,
            &proto::request::AckConfigure { serial },
        );
    }

    pub fn process(&mut self, raw: &WaylandRawEvent) -> Option<ExtSessionLockSurfaceV1Event> {
        let state = self.surfaces.get_mut(&raw.sender_id)?;
        let id = raw.sender_id;

        if let Some(e) = parse::<proto::event::Configure>(raw) {
            state.width = e.width;
            state.height = e.height;
            Some(ExtSessionLockSurfaceV1Event::Configure {
                id,
                serial: e.serial,
                width: e.width,
                height: e.height,
            })
        } else {
            None
        }
    }
}

pub fn module<AppState>() -> impl app::RegisteredModule<ExtSessionLockSurfaceV1, AppState> {
    app::Module::<ExtSessionLockSurfaceV1, _, _>::new()
        .on(|d: &mut ExtSessionLockSurfaceV1, ev: &crate::WaylandRawEvent| d.process(ev))
}
