use app::Event;

use crate::proto::Handle;
use crate::proto::ext_session_lock_v1 as proto;
use crate::{SharedConnection, WaylandRawEvent, parse, send};

#[derive(Debug)]
pub enum ExtSessionLockEvent {
    Locked,
    Finished,
}

impl Event for ExtSessionLockEvent {}

pub struct ExtSessionLockV1 {
    conn: SharedConnection,
    handle: Handle<proto::ExtSessionLockV1>,
}

impl ExtSessionLockV1 {
    pub fn new(conn: SharedConnection) -> Self {
        Self {
            conn,
            handle: Handle::new(0),
        }
    }

    pub fn set_id(&mut self, id: u32) {
        self.handle = Handle::new(id);
    }

    pub fn destroy(&self) {
        send(&self.conn, &self.handle, &proto::request::Destroy);
    }

    pub fn get_lock_surface(&self, id: u32, surface: u32, output: u32) {
        send(
            &self.conn,
            &self.handle,
            &proto::request::GetLockSurface {
                id,
                surface,
                output,
            },
        );
    }

    pub fn unlock_and_destroy(&self) {
        send(&self.conn, &self.handle, &proto::request::UnlockAndDestroy);
    }

    pub fn process(&mut self, raw: &WaylandRawEvent) -> Option<ExtSessionLockEvent> {
        if raw.sender_id != self.handle.id {
            return None;
        }

        let ev = if let Some(_) = parse::<proto::event::Locked>(raw) {
            ExtSessionLockEvent::Locked
        } else if let Some(_) = parse::<proto::event::Finished>(raw) {
            ExtSessionLockEvent::Finished
        } else {
            return None;
        };
        Some(ev)
    }
}

pub fn module<AppState>() -> impl app::RegisteredModule<ExtSessionLockV1, AppState> {
    app::Module::<ExtSessionLockV1, _, _>::new()
        .on(|d: &mut ExtSessionLockV1, ev: &crate::WaylandRawEvent| d.process(ev))
}
