use std::{marker::PhantomData, os::unix::net::UnixStream, path::PathBuf};

pub trait WaylandInterface {
    const NAME: &'static str;
    const VERSION: u32;
}

pub trait WaylandMessage {
    type Interface: WaylandInterface;
    type MessageParams;
    const OPCODE: u16;
    fn create(params: MessageParams);
}

pub struct Handle<T: WaylandInterface> {
    id: u32,
    _phantom: PhantomData<T>,
}
pub struct WlRegistry;
impl WaylandInterface for WlRegistry {
    const NAME: &'static str = "wl_registry";
    const VERSION: u32 = 1;
}

pub struct WlDisplay;
impl WaylandInterface for WlDisplay {
    const NAME: &'static str = "wl_display";
    const VERSION: u32 = 1;
}

pub struct Connection {
    inner: std::rc::Rc<std::cell::RefCell<Inner>>,
}

struct Inner {
    stream: UnixStream,
}

impl Connection {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();

        let stream = UnixStream::connect(&path).unwrap();
        stream.set_nonblocking(true).unwrap();

        let inner = std::rc::Rc::new(std::cell::RefCell::new(Inner { stream }));
        Self { inner }
    }

    pub fn bind<Interface: WaylandInterface>(message: &Msg) {
        let opcode = Msg::OPCODE;
    }
}
