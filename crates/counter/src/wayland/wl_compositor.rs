use crate::wayland::SharedConnection;

pub struct WlCompositor {
    conn: SharedConnection,
    pub id: u32,
}

impl WlCompositor {
    pub fn new(conn: SharedConnection) -> Self {
        Self { conn, id: 0 }
    }

    pub fn set_id(&mut self, id: u32) {
        self.id = id;
    }

    // opcode 0: create_surface(id: new_id) -> surface object_id
    pub fn create_surface(&self) -> u32 {
        let mut conn = self.conn.borrow_mut();
        let surface_id = conn.alloc_id();
        conn.message_builder(self.id, 0)
            .write_u32(surface_id)
            .build();
        surface_id
    }

    pub fn handle_event(&mut self, _sender_id: u32, _opcode: u16, _body: &[u8]) {
        // wl_compositor has no events
    }
}

#[macro_export]
macro_rules! register_wl_compositor {
    () => {
        app::module::Module::<crate::wayland::wl_compositor::WlCompositor>::new()
    };
}
