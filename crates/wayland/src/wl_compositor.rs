use crate::proto::wl_compositor as proto;
use crate::proto::Handle;
use crate::{SharedConnection, send};

pub struct WlCompositor {
    conn: SharedConnection,
    handle: Handle<proto::WlCompositor>,
}

impl WlCompositor {
    pub fn new(conn: SharedConnection) -> Self {
        Self { conn, handle: Handle::new(0) }
    }

    pub fn set_id(&mut self, id: u32) {
        self.handle = Handle::new(id);
    }

    pub fn create_surface(&self) -> u32 {
        let surface_id = self.conn.borrow_mut().alloc_id();
        send(&self.conn, &self.handle, &proto::request::CreateSurface { id: surface_id });
        surface_id
    }
}
