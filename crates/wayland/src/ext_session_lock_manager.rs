use crate::proto::Handle;
use crate::proto::ext_session_lock_manager_v1 as proto;
use crate::{SharedConnection, send};

pub struct ExtSessionLockManagerV1 {
    conn: SharedConnection,
    handle: Handle<proto::ExtSessionLockManagerV1>,
}

impl ExtSessionLockManagerV1 {
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

    pub fn lock(&self, new_id: u32) {
        send(
            &self.conn,
            &self.handle,
            &proto::request::Lock { id: new_id },
        );
    }
}
