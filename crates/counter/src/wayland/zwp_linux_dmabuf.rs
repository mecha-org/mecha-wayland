use std::collections::HashSet;
use std::os::fd::RawFd;

use app::event::Event;

use crate::wayland::{SharedConnection, WaylandRawEvent};
use crate::wire::MessageReader;

#[derive(Debug)]
pub enum DmabufEvent {
    Format { format: u32 },
    Modifier { format: u32, modifier_hi: u32, modifier_lo: u32 },
}

impl Event for DmabufEvent {}

// ── ZwpLinuxDmabufV1 ──────────────────────────────────────────────────────────

pub struct ZwpLinuxDmabufV1 {
    conn: SharedConnection,
    pub id: u32,
    pub formats: Vec<u32>,
}

impl ZwpLinuxDmabufV1 {
    pub fn new(conn: SharedConnection) -> Self {
        Self { conn, id: 0, formats: Vec::new() }
    }

    pub fn set_id(&mut self, id: u32) {
        self.id = id;
    }

    // opcode 1: create_params(params_id: new_id) -> zwp_linux_buffer_params_v1
    pub fn create_params(&self) -> u32 {
        let mut conn = self.conn.borrow_mut();
        let params_id = conn.alloc_id();
        conn.message_builder(self.id, 1).write_u32(params_id).build();
        params_id
    }

    pub fn process(&mut self, ev: &WaylandRawEvent) -> Option<DmabufEvent> {
        if ev.sender_id != self.id {
            return None;
        }
        let mut fds = vec![];
        let mut r = MessageReader::new(&ev.body, &mut fds);
        let event = match ev.opcode {
            0 => {
                let format = r.read_u32().unwrap_or(0);
                self.formats.push(format);
                DmabufEvent::Format { format }
            }
            1 => {
                let format = r.read_u32().unwrap_or(0);
                let modifier_hi = r.read_u32().unwrap_or(0);
                let modifier_lo = r.read_u32().unwrap_or(0);
                DmabufEvent::Modifier { format, modifier_hi, modifier_lo }
            }
            _ => return None,
        };
        Some(event)
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

    // opcode 0: destroy
    pub fn destroy(&mut self, params_id: u32) {
        self.active_ids.remove(&params_id);
        self.conn.borrow_mut().message_builder(params_id, 0).build();
    }

    // opcode 1: add(fd, plane_idx, offset, stride, modifier_hi, modifier_lo)
    // fd is sent out-of-band via SCM_RIGHTS; flush() must use sendmsg after this call.
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
        self.conn
            .borrow_mut()
            .message_builder(params_id, 1)
            .write_fd(fd)
            .write_u32(plane_idx)
            .write_u32(offset)
            .write_u32(stride)
            .write_u32(modifier_hi)
            .write_u32(modifier_lo)
            .build();
    }

    // opcode 3: create_immed(buffer_id: new_id, width, height, format, flags) -> wl_buffer
    pub fn create_immed(
        &self,
        params_id: u32,
        width: i32,
        height: i32,
        format: u32,
        flags: u32,
    ) -> u32 {
        let mut conn = self.conn.borrow_mut();
        let buf_id = conn.alloc_id();
        conn.message_builder(params_id, 3)
            .write_u32(buf_id)
            .write_i32(width)
            .write_i32(height)
            .write_u32(format)
            .write_u32(flags)
            .build();
        buf_id
    }
}

#[macro_export]
macro_rules! register_zwp_linux_dmabuf {
    () => {
        app::module::Module::<crate::wayland::ZwpLinuxDmabufV1>::new().processor(
            |d: &mut crate::wayland::ZwpLinuxDmabufV1, ev: &crate::wayland::WaylandRawEvent| {
                d.process(ev)
            },
        )
    };
}
