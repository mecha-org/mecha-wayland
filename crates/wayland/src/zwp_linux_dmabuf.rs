use crate::object::Object;
use crate::{ZwpLinuxDmabufV1, ZwpLinuxDmabufV1Handler};

pub struct DmaBuf {
    pub inner: ZwpLinuxDmabufV1,
}

impl DmaBuf {
    pub fn new(inner: ZwpLinuxDmabufV1) -> Self {
        DmaBuf { inner }
    }
}

impl Object for DmaBuf {
    fn object_id(&self) -> u32 {
        self.inner.object_id()
    }
}

impl ZwpLinuxDmabufV1Handler for DmaBuf {}
