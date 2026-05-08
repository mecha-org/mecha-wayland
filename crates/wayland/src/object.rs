pub trait WlObjectHandle: 'static {
    fn dispatch(&self, opcode: u16, body: &[u8]) -> std::io::Result<()>;
    fn as_any(&self) -> &dyn std::any::Any; // needed for downcast
}

pub trait WlObject: WlObjectHandle {
    type Proxy: WlObjectProxy;
    fn object_id(&self) -> u32;
    fn spawn(object_id: u32) -> (Self, Self::Proxy)
    where
        Self: Sized;
}

pub trait WlObjectProxy {
    fn object_id(&self) -> u32;
}
