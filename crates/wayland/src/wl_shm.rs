use std::collections::{HashMap, HashSet};
use std::os::fd::RawFd;

use app::event::Event;

use crate::proto::Handle;
use crate::proto::wl_shm as proto;
use crate::{SharedConnection, WaylandRawEvent, parse, send};

pub type ShmFormat = crate::proto::wl_shm::WlShmFormat;

#[derive(Debug)]
pub enum ShmEvent {
    Format { format: ShmFormat },
    BufferReleased { id: u32 },
}

impl Event for ShmEvent {}

pub struct BufferState {
    pub released: bool,
}

pub struct WlShm {
    conn: SharedConnection,
    handle: Handle<proto::WlShm>,
    pub formats: Vec<ShmFormat>,
    pool_ids: HashSet<u32>,
    pub buffer_states: HashMap<u32, BufferState>,
}

impl WlShm {
    pub fn new(conn: SharedConnection) -> Self {
        Self {
            conn,
            handle: Handle::new(0),
            formats: Vec::new(),
            pool_ids: HashSet::new(),
            buffer_states: HashMap::new(),
        }
    }

    pub fn set_id(&mut self, id: u32) {
        self.handle = Handle::new(id);
    }

    pub fn create_pool(&self, fd: RawFd, size: i32) -> u32 {
        let pool_id = self.conn.borrow_mut().alloc_id();
        self.conn.borrow_mut().push_fd(fd);
        send(
            &self.conn,
            &self.handle,
            &proto::request::CreatePool {
                id: pool_id,
                fd,
                size,
            },
        );
        pool_id
    }

    pub fn register_pool(&mut self, pool_id: u32) {
        self.pool_ids.insert(pool_id);
    }

    pub fn pool_create_buffer(
        &self,
        pool_id: u32,
        offset: i32,
        width: i32,
        height: i32,
        stride: i32,
        format: ShmFormat,
    ) -> u32 {
        let buf_id = self.conn.borrow_mut().alloc_id();
        let pool_handle = Handle::<crate::proto::wl_shm_pool::WlShmPool>::new(pool_id);
        send(
            &self.conn,
            &pool_handle,
            &crate::proto::wl_shm_pool::request::CreateBuffer {
                id: buf_id,
                offset,
                width,
                height,
                stride,
                format,
            },
        );
        buf_id
    }

    pub fn register_buffer(&mut self, buf_id: u32) {
        self.buffer_states
            .insert(buf_id, BufferState { released: true });
    }

    pub fn pool_destroy(&self, pool_id: u32) {
        let pool_handle = Handle::<crate::proto::wl_shm_pool::WlShmPool>::new(pool_id);
        send(
            &self.conn,
            &pool_handle,
            &crate::proto::wl_shm_pool::request::Destroy,
        );
    }

    pub fn process(&mut self, raw: &WaylandRawEvent) -> Option<ShmEvent> {
        if raw.sender_id == self.handle.id {
            let e = parse::<proto::event::Format>(raw)?;
            self.formats.push(e.format);
            let ev = ShmEvent::Format { format: e.format };
            println!("[wl_shm] {:?}", ev);
            Some(ev)
        } else if self.buffer_states.contains_key(&raw.sender_id) {
            // wl_buffer release — opcode 0
            if raw.opcode != 0 {
                return None;
            }
            if let Some(state) = self.buffer_states.get_mut(&raw.sender_id) {
                state.released = true;
            }
            let ev = ShmEvent::BufferReleased { id: raw.sender_id };
            println!("[wl_shm] {:?}", ev);
            Some(ev)
        } else {
            None
        }
    }
}

pub fn alloc_shm_fd(size: usize) -> RawFd {
    let fd = unsafe { libc::syscall(libc::SYS_memfd_create, b"wl_shm\0".as_ptr(), 0u32) as RawFd };
    assert!(fd >= 0, "memfd_create failed");
    assert_eq!(
        unsafe { libc::ftruncate(fd, size as libc::off_t) },
        0,
        "ftruncate failed"
    );
    fd
}

pub fn mmap_shm(fd: RawFd, size: usize) -> *mut u8 {
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd,
            0,
        )
    };
    assert_ne!(ptr, libc::MAP_FAILED, "mmap failed");
    ptr as *mut u8
}

#[macro_export]
macro_rules! register_wl_shm {
    () => {
        app::module::Module::<crate::WlShm>::new()
            .processor(|s: &mut crate::WlShm, ev: &crate::WaylandRawEvent| s.process(ev))
    };
}
