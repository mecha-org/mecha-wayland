use std::collections::{HashMap, HashSet};
use std::os::fd::RawFd;

use app::event::Event;

use crate::wayland::{SharedConnection, WaylandRawEvent};
use crate::wire::MessageReader;

#[derive(Debug)]
pub enum ShmEvent {
    Format { format: u32 },
    BufferReleased { id: u32 },
}

impl Event for ShmEvent {}

pub struct BufferState {
    pub released: bool,
}

pub struct WlShm {
    conn: SharedConnection,
    pub id: u32,
    pub formats: Vec<u32>,
    pool_ids: HashSet<u32>,
    pub buffer_states: HashMap<u32, BufferState>,
}

impl WlShm {
    pub fn new(conn: SharedConnection) -> Self {
        Self {
            conn,
            id: 0,
            formats: Vec::new(),
            pool_ids: HashSet::new(),
            buffer_states: HashMap::new(),
        }
    }

    pub fn set_id(&mut self, id: u32) {
        self.id = id;
    }

    // opcode 0: create_pool(id: new_id, fd: fd, size: int) -> pool_id
    // The fd is sent via SCM_RIGHTS; flush() must use sendmsg after this call.
    pub fn create_pool(&self, fd: RawFd, size: i32) -> u32 {
        let mut conn = self.conn.borrow_mut();
        let pool_id = conn.alloc_id();
        conn.push_fd(fd);
        conn.message_builder(self.id, 0)
            .write_u32(pool_id)
            .write_i32(size)
            .build();
        pool_id
    }

    pub fn register_pool(&mut self, pool_id: u32) {
        self.pool_ids.insert(pool_id);
    }

    // wl_shm_pool opcode 0: create_buffer(id: new_id, offset: int,
    //   width: int, height: int, stride: int, format: uint) -> buffer_id
    pub fn pool_create_buffer(
        &self,
        pool_id: u32,
        offset: i32,
        width: i32,
        height: i32,
        stride: i32,
        format: u32,
    ) -> u32 {
        let mut conn = self.conn.borrow_mut();
        let buf_id = conn.alloc_id();
        conn.message_builder(pool_id, 0)
            .write_u32(buf_id)
            .write_i32(offset)
            .write_i32(width)
            .write_i32(height)
            .write_i32(stride)
            .write_u32(format)
            .build();
        buf_id
    }

    pub fn register_buffer(&mut self, buf_id: u32) {
        self.buffer_states.insert(buf_id, BufferState { released: true });
    }

    // wl_shm_pool opcode 1: destroy(pool_id)
    pub fn pool_destroy(&self, pool_id: u32) {
        self.conn.borrow_mut().message_builder(pool_id, 1).build();
    }

    pub fn process(&mut self, ev: &WaylandRawEvent) -> Option<ShmEvent> {
        let event = if ev.sender_id == self.id {
            if ev.opcode != 0 {
                return None;
            }
            let mut fds = vec![];
            let mut r = MessageReader::new(&ev.body, &mut fds);
            let format = r.read_u32().unwrap_or(0);
            self.formats.push(format);
            ShmEvent::Format { format }
        } else if self.buffer_states.contains_key(&ev.sender_id) {
            if ev.opcode != 0 {
                return None;
            }
            if let Some(state) = self.buffer_states.get_mut(&ev.sender_id) {
                state.released = true;
            }
            ShmEvent::BufferReleased { id: ev.sender_id }
        } else {
            return None;
        };
        println!("[wl_shm] {:?}", event);
        Some(event)
    }
}

/// Creates an anonymous memfd and ftruncates it to `size` bytes.
pub fn alloc_shm_fd(size: usize) -> RawFd {
    let fd = unsafe {
        libc::syscall(libc::SYS_memfd_create, b"wl_shm\0".as_ptr(), 0u32) as RawFd
    };
    assert!(fd >= 0, "memfd_create failed");
    assert_eq!(
        unsafe { libc::ftruncate(fd, size as libc::off_t) },
        0,
        "ftruncate failed"
    );
    fd
}

/// Maps the fd into process memory as MAP_SHARED read-write.
/// Caller must munmap the returned pointer when done.
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
        app::module::Module::<crate::wayland::WlShm>::new()
            .processor(|s: &mut crate::wayland::WlShm, ev: &crate::wayland::WaylandRawEvent| {
                s.process(ev)
            })
    };
}
