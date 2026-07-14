use app::prelude::*;

use super::{WlCallback, WlDisplay, WlRegistry, read_string, read_u32};
use crate::server::{ClientId, ClientRawEvent, WaylandServer};
use crate::{Handle, Interface, ObjectId, RawWaylandEvent, Wayland, helper};

// ── wl_display requests (client sends to server) ──────────────────────────────

#[derive(Debug)]
pub enum WlDisplayRequest {
    Sync {
        client_id: ClientId,
        sender: Handle<WlDisplay>,
        callback: Handle<WlCallback>,
    },
    GetRegistry {
        client_id: ClientId,
        sender: Handle<WlDisplay>,
        registry: Handle<WlRegistry>,
    },
}
impl Event for WlDisplayRequest {}

impl WlDisplayRequest {
    pub fn parse(
        event: &RawWaylandEvent,
        wayland: &mut Wayland,
        client_id: ClientId,
    ) -> Option<Self> {
        let sender = wayland.get_handle::<WlDisplay>(event.object_id)?;
        let data = &event.data;
        let mut o = 0;
        match event.opcode {
            0 => Some(WlDisplayRequest::Sync {
                client_id,
                sender: sender.clone(),
                callback: wayland.new_handle(ObjectId(read_u32(data, &mut o)?)),
            }),
            1 => Some(WlDisplayRequest::GetRegistry {
                client_id,
                sender,
                registry: wayland.new_handle(ObjectId(read_u32(data, &mut o)?)),
            }),
            _ => None,
        }
    }
}

// ── wl_registry requests ──────────────────────────────────────────────────────

#[derive(Debug)]
pub enum WlRegistryRequest {
    Bind {
        client_id: ClientId,
        sender: Handle<WlRegistry>,
        name: u32,
        interface: String,
        id: ObjectId,
    },
}
impl Event for WlRegistryRequest {}

impl WlRegistryRequest {
    pub fn parse(
        event: &RawWaylandEvent,
        wayland: &mut Wayland,
        client_id: ClientId,
    ) -> Option<Self> {
        let sender = wayland.get_handle::<WlRegistry>(event.object_id)?;
        let data = &event.data;
        let mut o = 0;
        match event.opcode {
            0 => {
                let name = read_u32(data, &mut o)?;
                let interface = read_string(data, &mut o)?;
                let _version = read_u32(data, &mut o)?;
                let id = ObjectId(read_u32(data, &mut o)?);
                Some(WlRegistryRequest::Bind {
                    client_id,
                    sender,
                    name,
                    interface,
                    id,
                })
            }
            _ => None,
        }
    }
}

// ── Handle<T> event methods (server sends events to client) ───────────────────

impl Handle<WlDisplay> {
    pub fn error(&self, object_id: ObjectId, code: u32, message: &str) {
        let sender_id = self.object_id().expect("dead handle").0;
        let mut body = Vec::new();
        body.extend_from_slice(&object_id.0.to_ne_bytes());
        body.extend_from_slice(&code.to_ne_bytes());
        helper::encode_string(&mut body, message);
        self.proxy.write_raw(sender_id, 0, &body, &[]);
    }

    pub fn delete_id(&self, id: u32) {
        let sender_id = self.object_id().expect("dead handle").0;
        self.proxy.write_raw(sender_id, 1, &id.to_ne_bytes(), &[]);
    }
}

impl Handle<WlCallback> {
    pub fn done(&self, callback_data: u32) {
        let sender_id = self.object_id().expect("dead handle").0;
        self.proxy
            .write_raw(sender_id, 0, &callback_data.to_ne_bytes(), &[]);
    }
}

impl Handle<WlRegistry> {
    pub fn global(&self, name: u32, interface: &str, version: u32) {
        let sender_id = self.object_id().expect("dead handle").0;
        let mut body = Vec::new();
        body.extend_from_slice(&name.to_ne_bytes());
        helper::encode_string(&mut body, interface);
        body.extend_from_slice(&version.to_ne_bytes());
        self.proxy.write_raw(sender_id, 0, &body, &[]);
    }

    pub fn global_remove(&self, name: u32) {
        let sender_id = self.object_id().expect("dead handle").0;
        self.proxy.write_raw(sender_id, 1, &name.to_ne_bytes(), &[]);
    }
}

// ── server dispatch module ────────────────────────────────────────────────────

pub fn server_dispatch_module<S>() -> impl app::RegisteredModule<WaylandServer, S> {
    app::Module::new()
        .on(|server: &mut WaylandServer, ev: &ClientRawEvent| {
            let mut inner = server.data.borrow_mut();
            let client = inner.clients.get_mut(&ev.client_id)?;
            if client.conn.get_interface(ev.raw.object_id) == Some(WlDisplay::NAME) {
                WlDisplayRequest::parse(&ev.raw, &mut client.conn, ev.client_id)
            } else {
                None
            }
        })
        .on(|server: &mut WaylandServer, ev: &ClientRawEvent| {
            let mut inner = server.data.borrow_mut();
            let client = inner.clients.get_mut(&ev.client_id)?;
            if client.conn.get_interface(ev.raw.object_id) == Some(WlRegistry::NAME) {
                WlRegistryRequest::parse(&ev.raw, &mut client.conn, ev.client_id)
            } else {
                None
            }
        })
}
