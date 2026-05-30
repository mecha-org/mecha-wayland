use std::collections::HashSet;
use std::os::fd::RawFd;

use app::Event;

use crate::proto::Handle;
use crate::{SharedConnection, WaylandRawEvent, parse, send};

#[derive(Debug)]
pub enum DmabufEvent {
    Format { format: u32 },
    Modifier { format: u32, modifier_hi: u32, modifier_lo: u32 },
}

impl Event for DmabufEvent {}

// ── ZwpLinuxDmabufV1 ──────────────────────────────────────────────────────────

pub struct ZwpLinuxDmabufV1 {
    conn: SharedConnection,
    handle: Handle<crate::proto::zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1>,
    pub formats: Vec<u32>,
}

impl ZwpLinuxDmabufV1 {
    pub fn new(conn: SharedConnection) -> Self {
        Self { conn, handle: Handle::new(0), formats: Vec::new() }
    }

    pub fn set_id(&mut self, id: u32) {
        self.handle = Handle::new(id);
    }

    pub fn create_params(&self) -> u32 {
        let params_id = self.conn.borrow_mut().alloc_id();
        send(&self.conn, &self.handle, &crate::proto::zwp_linux_dmabuf_v1::request::CreateParams { params_id });
        params_id
    }

    pub fn process(&mut self, raw: &WaylandRawEvent) -> Option<DmabufEvent> {
        if raw.sender_id != self.handle.id {
            return None;
        }
        if let Some(e) = parse::<crate::proto::zwp_linux_dmabuf_v1::event::Format>(raw) {
            self.formats.push(e.format);
            Some(DmabufEvent::Format { format: e.format })
        } else if let Some(e) = parse::<crate::proto::zwp_linux_dmabuf_v1::event::Modifier>(raw) {
            Some(DmabufEvent::Modifier {
                format: e.format,
                modifier_hi: e.modifier_hi,
                modifier_lo: e.modifier_lo,
            })
        } else {
            None
        }
    }
}

// ── ZwpLinuxBufferParamsV1 ────────────────────────────────────────────────────

pub struct ZwpLinuxBufferParamsV1 {
    conn: SharedConnection,
    active_ids: HashSet<u32>,
}

impl ZwpLinuxBufferParamsV1 {
    pub fn new(conn: SharedConnection) -> Self {
        Self { conn, active_ids: HashSet::new() }
    }

    pub fn register(&mut self, params_id: u32) {
        self.active_ids.insert(params_id);
    }

    pub fn destroy(&mut self, params_id: u32) {
        self.active_ids.remove(&params_id);
        let h = Handle::<crate::proto::zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1>::new(params_id);
        send(&self.conn, &h, &crate::proto::zwp_linux_buffer_params_v1::request::Destroy);
    }

    pub fn add(
        &self,
        params_id: u32,
        fd: RawFd,
        plane_idx: u32,
        offset: u32,
        stride: u32,
        modifier_hi: u32,
        modifier_lo: u32,
    ) {
        let h = Handle::<crate::proto::zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1>::new(params_id);
        send(
            &self.conn,
            &h,
            &crate::proto::zwp_linux_buffer_params_v1::request::Add { fd, plane_idx, offset, stride, modifier_hi, modifier_lo },
        );
    }

    pub fn create_immed(
        &self,
        params_id: u32,
        width: i32,
        height: i32,
        format: u32,
        flags: u32,
    ) -> u32 {
        let buf_id = self.conn.borrow_mut().alloc_id();
        let h = Handle::<crate::proto::zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1>::new(params_id);
        let flags_flags = crate::proto::zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1Flags::from_bits_retain(flags);
        send(
            &self.conn,
            &h,
            &crate::proto::zwp_linux_buffer_params_v1::request::CreateImmed { buffer_id: buf_id, width, height, format, flags: flags_flags },
        );
        buf_id
    }
}

pub fn module<AppState>() -> impl app::RegisteredModule<ZwpLinuxDmabufV1, AppState> {
    app::Module::<ZwpLinuxDmabufV1, _, _>::new()
        .on(|d: &mut ZwpLinuxDmabufV1, ev: &crate::WaylandRawEvent| d.process(ev))
}
