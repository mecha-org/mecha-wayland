use crate::{Handle, Interface, ObjectId, RawWaylandEvent, Wayland, helper};
use app::prelude::*;

#[cfg(any(feature = "client", feature = "server"))]
fn read_u32(data: &[u8], offset: &mut usize) -> Option<u32> {
    let bytes = data.get(*offset..*offset + 4)?;
    *offset += 4;
    Some(u32::from_ne_bytes(bytes.try_into().unwrap()))
}

#[cfg(any(feature = "client", feature = "server"))]
fn read_string(data: &[u8], offset: &mut usize) -> Option<String> {
    let len = read_u32(data, offset)? as usize;
    let padded = (len + 3) & !3;
    let raw = data.get(*offset..*offset + padded)?;
    *offset += padded;
    let s = std::str::from_utf8(raw.get(..len.saturating_sub(1))?).ok()?;
    Some(s.to_owned())
}

// ── wl_display ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct WlDisplay;
impl Interface for WlDisplay {
    const NAME: &'static str = "wl_display";
    const VERSION: u32 = 1;
}

#[cfg(feature = "server")]
#[derive(Debug)]
pub enum WlDisplayRequest {
    Sync { callback: Handle<WlCallback> },
    GetRegistry { registry: Handle<WlRegistry> },
}
#[cfg(feature = "server")]
impl Event for WlDisplayRequest {}

#[cfg(feature = "server")]
impl WlDisplayRequest {
    pub fn parse(event: &RawWaylandEvent, wayland: &mut Wayland) -> Option<Self> {
        let data = &event.data;
        let mut o = 0;
        match event.opcode {
            0 => Some(WlDisplayRequest::Sync {
                callback: wayland.new_handle(ObjectId(read_u32(data, &mut o)?)),
            }),
            1 => Some(WlDisplayRequest::GetRegistry {
                registry: wayland.new_handle(ObjectId(read_u32(data, &mut o)?)),
            }),
            _ => None,
        }
    }
}

#[cfg(feature = "client")]
#[derive(Debug)]
pub enum WlDisplayEvent {
    Error {
        object_id: ObjectId,
        code: u32,
        message: String,
    },
    DeleteId {
        id: u32,
    },
}
#[cfg(feature = "client")]
impl Event for WlDisplayEvent {}

#[cfg(feature = "client")]
impl WlDisplayEvent {
    pub fn parse(event: &RawWaylandEvent) -> Option<Self> {
        let data = &event.data;
        let mut o = 0;
        match event.opcode {
            0 => Some(WlDisplayEvent::Error {
                object_id: ObjectId(read_u32(data, &mut o)?),
                code: read_u32(data, &mut o)?,
                message: read_string(data, &mut o)?,
            }),
            1 => Some(WlDisplayEvent::DeleteId {
                id: read_u32(data, &mut o)?,
            }),
            _ => None,
        }
    }
}

#[cfg(feature = "client")]
impl Handle<WlDisplay> {
    pub fn sync(&self) -> Handle<WlCallback> {
        let cb: Handle<WlCallback> = self.proxy.alloc_handle();
        let sender_id = self.object_id().expect("dead handle").0;
        let cb_id = cb.object_id().expect("just allocated").0;
        self.proxy.write_raw(sender_id, 0, &cb_id.to_ne_bytes());
        cb
    }

    pub fn get_registry(&self) -> Handle<WlRegistry> {
        let reg: Handle<WlRegistry> = self.proxy.alloc_handle();
        let sender_id = self.object_id().expect("dead handle").0;
        let reg_id = reg.object_id().expect("just allocated").0;
        self.proxy.write_raw(sender_id, 1, &reg_id.to_ne_bytes());
        reg
    }
}

#[cfg(feature = "client")]
pub enum WlDisplayError {
    InvalidObject,
    InvalidMethod,
    NoMemory,
    Implementation,
}

// ── wl_callback ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct WlCallback;
impl Interface for WlCallback {
    const NAME: &'static str = "wl_callback";
    const VERSION: u32 = 1;
}

#[cfg(feature = "client")]
#[derive(Debug)]
pub enum WlCallbackEvent {
    Done { callback_data: u32 },
}
#[cfg(feature = "client")]
impl Event for WlCallbackEvent {}

#[cfg(feature = "client")]
impl WlCallbackEvent {
    pub fn parse(event: &RawWaylandEvent) -> Option<Self> {
        let data = &event.data;
        let mut o = 0;
        match event.opcode {
            0 => Some(WlCallbackEvent::Done {
                callback_data: read_u32(data, &mut o)?,
            }),
            _ => None,
        }
    }
}

// ── wl_registry ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct WlRegistry;
impl Interface for WlRegistry {
    const NAME: &'static str = "wl_registry";
    const VERSION: u32 = 1;
}

#[cfg(feature = "server")]
#[derive(Debug)]
pub enum WlRegistryRequest {
    Bind { name: u32, id: ObjectId },
}
#[cfg(feature = "server")]
impl Event for WlRegistryRequest {}

#[cfg(feature = "server")]
impl WlRegistryRequest {
    pub fn parse(event: &RawWaylandEvent) -> Option<Self> {
        let data = &event.data;
        let mut o = 0;
        match event.opcode {
            0 => {
                let name = read_u32(data, &mut o)?;
                let _interface = read_string(data, &mut o)?;
                let _version = read_u32(data, &mut o)?;
                let id = ObjectId(read_u32(data, &mut o)?);
                Some(WlRegistryRequest::Bind { name, id })
            }
            _ => None,
        }
    }
}

#[cfg(feature = "client")]
#[derive(Debug)]
pub enum WlRegistryEvent {
    Global {
        name: u32,
        interface: String,
        version: u32,
    },
    GlobalDelete {
        name: u32,
    },
}
#[cfg(feature = "client")]
impl Event for WlRegistryEvent {}

#[cfg(feature = "client")]
impl WlRegistryEvent {
    pub fn parse(event: &RawWaylandEvent) -> Option<Self> {
        let data = &event.data;
        let mut o = 0;
        match event.opcode {
            0 => Some(WlRegistryEvent::Global {
                name: read_u32(data, &mut o)?,
                interface: read_string(data, &mut o)?,
                version: read_u32(data, &mut o)?,
            }),
            1 => Some(WlRegistryEvent::GlobalDelete {
                name: read_u32(data, &mut o)?,
            }),
            _ => None,
        }
    }
}

#[cfg(feature = "client")]
impl Handle<WlRegistry> {
    pub fn bind<T: Interface>(&self, name: u32, version: u32) -> Handle<T> {
        let new_obj: Handle<T> = self.proxy.alloc_handle();
        let sender_id = self.object_id().expect("dead handle").0;
        let new_id = new_obj.object_id().expect("just allocated").0;

        let mut body = Vec::new();
        body.extend_from_slice(&name.to_ne_bytes());
        helper::encode_string(&mut body, T::NAME);
        body.extend_from_slice(&version.to_ne_bytes());
        body.extend_from_slice(&new_id.to_ne_bytes());

        self.proxy.write_raw(sender_id, 0, &body);
        new_obj
    }
}

// ── module ────────────────────────────────────────────────────────────────────

pub fn module<S>() -> impl app::RegisteredModule<Wayland, S> {
    let m = app::Module::new();

    #[cfg(feature = "client")]
    let m = m
        .on(|wayland: &mut Wayland, raw: &RawWaylandEvent| {
            if wayland.get_interface(raw.object_id) == Some(WlDisplay::NAME) {
                WlDisplayEvent::parse(raw)
            } else {
                None
            }
        })
        .on(|wayland: &mut Wayland, raw: &RawWaylandEvent| {
            if wayland.get_interface(raw.object_id) == Some(WlCallback::NAME) {
                WlCallbackEvent::parse(raw)
            } else {
                None
            }
        })
        .on(|wayland: &mut Wayland, raw: &RawWaylandEvent| {
            if wayland.get_interface(raw.object_id) == Some(WlRegistry::NAME) {
                WlRegistryEvent::parse(raw)
            } else {
                None
            }
        });

    #[cfg(feature = "server")]
    let m = m
        .on(|wayland: &mut Wayland, raw: &RawWaylandEvent| {
            if wayland.get_interface(raw.object_id) == Some(WlDisplay::NAME) {
                WlDisplayRequest::parse(raw, wayland)
            } else {
                None
            }
        })
        .on(|wayland: &mut Wayland, raw: &RawWaylandEvent| {
            if wayland.get_interface(raw.object_id) == Some(WlRegistry::NAME) {
                WlRegistryRequest::parse(raw)
            } else {
                None
            }
        });

    m
}
