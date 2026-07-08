use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::mem;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::rc::{Rc, Weak};

use app::prelude::*;
use io_ring::{IoToken, RingProxy};
use io_uring::{opcode, types};

pub(crate) mod helper;
pub mod proto;

#[cfg(feature = "client")]
mod client;
#[cfg(feature = "client")]
pub use client::module;

#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "server")]
pub use server::{
    ClientConnected, ClientId, ClientRawEvent, WaylandServer, server_module,
};

pub use proto::*;

pub trait Interface {
    const NAME: &'static str;
    const VERSION: u32;
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct ObjectId(pub(crate) u32);

#[derive(Debug)]
pub struct RawWaylandEvent {
    pub(crate) object_id: ObjectId,
    pub(crate) opcode: u32,
    pub(crate) data: Vec<u8>,
}
impl Event for RawWaylandEvent {}

const CMSG_BUF_SIZE: usize = 256;

pub(crate) struct WaylandInner {
    pub(crate) fd: RawFd,
    pub(crate) ring_proxy: RingProxy,

    pub(crate) read_buf: Vec<u8>,
    pub(crate) cmsg_recv_buf: Vec<u8>,
    pub(crate) recv_iov: libc::iovec,
    pub(crate) recv_msghdr: libc::msghdr,

    pub(crate) write_buf: Vec<u8>,
    pub(crate) write_fds_buf: Vec<OwnedFd>,
    pub(crate) write_in_flight: Vec<u8>,
    pub(crate) write_fds_in_flight: Vec<OwnedFd>,
    pub(crate) write_cmsg_buf: Vec<u8>,
    pub(crate) send_iov: libc::iovec,
    pub(crate) send_msghdr: libc::msghdr,

    pub(crate) fd_queue: VecDeque<OwnedFd>,

    pub(crate) read_token: Option<IoToken>,
    pub(crate) write_token: Option<IoToken>,
    pub(crate) next_id: u32,
    pub(crate) object_slots: HashMap<ObjectId, Rc<ObjectId>>,
    pub(crate) object_interfaces: HashMap<ObjectId, &'static str>,
    pub(crate) deleted_object_ids: HashSet<ObjectId>,
}

impl WaylandInner {
    pub(crate) fn submit_read(&mut self) {
        if self.read_token.is_some() {
            return;
        }
        unsafe {
            let iov_ptr = &mut self.recv_iov as *mut libc::iovec;
            (*iov_ptr).iov_base = self.read_buf.as_mut_ptr() as *mut libc::c_void;
            (*iov_ptr).iov_len = self.read_buf.len();

            let msghdr_ptr = &mut self.recv_msghdr as *mut libc::msghdr;
            std::ptr::write(msghdr_ptr, mem::zeroed());
            (*msghdr_ptr).msg_iov = iov_ptr;
            (*msghdr_ptr).msg_iovlen = 1;
            (*msghdr_ptr).msg_control = self.cmsg_recv_buf.as_mut_ptr() as *mut libc::c_void;
            (*msghdr_ptr).msg_controllen = self.cmsg_recv_buf.len() as libc::size_t;

            let sqe = opcode::RecvMsg::new(types::Fd(self.fd), msghdr_ptr).build();
            let token = self.ring_proxy.push(sqe);
            self.read_token = Some(token);
        }
    }

    pub(crate) fn extract_recv_fds(&mut self) {
        unsafe {
            let msghdr_ptr = &self.recv_msghdr as *const libc::msghdr;
            let mut cmsg = libc::CMSG_FIRSTHDR(msghdr_ptr);
            while !cmsg.is_null() {
                if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                    let data_len =
                        ((*cmsg).cmsg_len as usize).saturating_sub(libc::CMSG_LEN(0) as usize);
                    let n_fds = data_len / mem::size_of::<RawFd>();
                    let fd_ptr = libc::CMSG_DATA(cmsg) as *const RawFd;
                    for i in 0..n_fds {
                        self.fd_queue
                            .push_back(OwnedFd::from_raw_fd(*fd_ptr.add(i)));
                    }
                }
                cmsg = libc::CMSG_NXTHDR(msghdr_ptr, cmsg);
            }
        }
    }

    pub(crate) fn submit_write(&mut self) {
        if self.write_token.is_some() || !self.write_in_flight.is_empty() {
            return;
        }
        if self.write_buf.is_empty() {
            return;
        }
        mem::swap(&mut self.write_in_flight, &mut self.write_buf);
        mem::swap(&mut self.write_fds_in_flight, &mut self.write_fds_buf);

        if self.write_fds_in_flight.is_empty() {
            let sqe = opcode::Write::new(
                types::Fd(self.fd),
                self.write_in_flight.as_ptr(),
                self.write_in_flight.len() as u32,
            )
            .build();
            let token = self.ring_proxy.push(sqe);
            self.write_token = Some(token);
        } else {
            unsafe {
                let fd_count = self.write_fds_in_flight.len();
                let fd_bytes = (fd_count * mem::size_of::<RawFd>()) as u32;
                let cmsg_space = libc::CMSG_SPACE(fd_bytes) as usize;
                self.write_cmsg_buf.resize(cmsg_space, 0);

                let iov_ptr = &mut self.send_iov as *mut libc::iovec;
                (*iov_ptr).iov_base = self.write_in_flight.as_ptr() as *mut libc::c_void;
                (*iov_ptr).iov_len = self.write_in_flight.len();

                let msghdr_ptr = &mut self.send_msghdr as *mut libc::msghdr;
                std::ptr::write(msghdr_ptr, mem::zeroed());
                (*msghdr_ptr).msg_iov = iov_ptr;
                (*msghdr_ptr).msg_iovlen = 1;
                (*msghdr_ptr).msg_control = self.write_cmsg_buf.as_mut_ptr() as *mut libc::c_void;
                (*msghdr_ptr).msg_controllen = cmsg_space as libc::size_t;

                let cmsg = libc::CMSG_FIRSTHDR(msghdr_ptr);
                (*cmsg).cmsg_level = libc::SOL_SOCKET;
                (*cmsg).cmsg_type = libc::SCM_RIGHTS;
                (*cmsg).cmsg_len = libc::CMSG_LEN(fd_bytes) as _;

                let raw_fds: Vec<RawFd> = self
                    .write_fds_in_flight
                    .iter()
                    .map(|f| f.as_raw_fd())
                    .collect();
                std::ptr::copy_nonoverlapping(
                    raw_fds.as_ptr(),
                    libc::CMSG_DATA(cmsg) as *mut RawFd,
                    fd_count,
                );

                let sqe =
                    opcode::SendMsg::new(types::Fd(self.fd), msghdr_ptr as *const libc::msghdr)
                        .build();
                let token = self.ring_proxy.push(sqe);
                self.write_token = Some(token);
            }
        }
    }

    pub(crate) fn get_interface(&self, id: ObjectId) -> Option<&'static str> {
        self.object_interfaces.get(&id).copied()
    }

    pub(crate) fn alloc_id(&mut self) -> ObjectId {
        if let Some(&id) = self.deleted_object_ids.iter().next() {
            self.deleted_object_ids.remove(&id);
            id
        } else {
            let id = ObjectId(self.next_id);
            self.next_id += 1;
            id
        }
    }

    pub(crate) fn invalidate_object(&mut self, id: ObjectId) {
        self.object_slots.remove(&id);
        self.object_interfaces.remove(&id);
        self.deleted_object_ids.insert(id);
    }
}

// Shared constructor used by both Wayland::new() (client) and Wayland::new_server() (server).
pub(crate) fn make_inner(
    fd: RawFd,
    ring_proxy: RingProxy,
    next_id: u32,
) -> Rc<RefCell<WaylandInner>> {
    let display_id = ObjectId(1);
    let display_rc = Rc::new(display_id);
    let mut object_slots = HashMap::new();
    let mut object_interfaces = HashMap::new();
    object_slots.insert(display_id, display_rc);
    object_interfaces.insert(display_id, "wl_display");

    let inner = WaylandInner {
        fd,
        ring_proxy,
        read_buf: vec![0u8; 65536],
        cmsg_recv_buf: vec![0u8; CMSG_BUF_SIZE],
        recv_iov: unsafe { mem::zeroed() },
        recv_msghdr: unsafe { mem::zeroed() },
        write_buf: Vec::with_capacity(4096),
        write_fds_buf: Vec::new(),
        write_in_flight: Vec::new(),
        write_fds_in_flight: Vec::new(),
        write_cmsg_buf: Vec::new(),
        send_iov: unsafe { mem::zeroed() },
        send_msghdr: unsafe { mem::zeroed() },
        fd_queue: VecDeque::new(),
        read_token: None,
        write_token: None,
        next_id,
        object_slots,
        object_interfaces,
        deleted_object_ids: HashSet::new(),
    };
    Rc::new(RefCell::new(inner))
}

pub(crate) enum IoCompletion {
    Read(Vec<RawWaylandEvent>),
    Write,
    Disconnect,
    Unrelated,
}

// Shared I/O completion handler. Client panics on Disconnect; server emits ClientDisconnected.
pub(crate) fn handle_io_event(
    data: &Rc<RefCell<WaylandInner>>,
    token: IoToken,
    result: i32,
) -> IoCompletion {
    let mut inner = data.borrow_mut();
    if Some(token) == inner.read_token {
        inner.read_token = None;
        if result <= 0 {
            return IoCompletion::Disconnect;
        }
        let bytes = inner.read_buf[..result as usize].to_vec();
        inner.extract_recv_fds();
        drop(inner);
        IoCompletion::Read(helper::parse_messages(bytes))
    } else if Some(token) == inner.write_token {
        inner.write_token = None;
        inner.write_in_flight.clear();
        inner.write_fds_in_flight.clear();
        inner.submit_write();
        IoCompletion::Write
    } else {
        IoCompletion::Unrelated
    }
}

#[derive(Clone)]
pub struct WaylandProxy(Rc<RefCell<WaylandInner>>);

impl WaylandProxy {
    pub fn new_handle<T: Interface>(&self, id: ObjectId) -> Handle<T> {
        let mut inner = self.0.borrow_mut();
        let rc = Rc::new(id);
        inner.object_slots.insert(id, rc.clone());
        inner.object_interfaces.insert(id, T::NAME);
        Handle {
            slot: Rc::downgrade(&rc),
            proxy: self.clone(),
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn get_handle<T: Interface>(&self, id: ObjectId) -> Option<Handle<T>> {
        let inner = self.0.borrow();
        inner.object_slots.get(&id).map(|rc| Handle {
            slot: Rc::downgrade(rc),
            proxy: self.clone(),
            _phantom: std::marker::PhantomData,
        })
    }

    pub fn alloc_handle<T: Interface>(&self) -> Handle<T> {
        let id = self.0.borrow_mut().alloc_id();
        self.new_handle(id)
    }

    pub fn flush(&self) {
        self.0.borrow_mut().submit_write();
    }

    pub(crate) fn write_raw(
        &self,
        sender_id: u32,
        opcode: u16,
        body: &[u8],
        fds: &[BorrowedFd<'_>],
    ) {
        let mut inner = self.0.borrow_mut();
        let total = (8 + body.len()) as u32;
        inner.write_buf.extend_from_slice(&sender_id.to_ne_bytes());
        inner
            .write_buf
            .extend_from_slice(&((total << 16) | opcode as u32).to_ne_bytes());
        inner.write_buf.extend_from_slice(body);
        for fd in fds {
            inner
                .write_fds_buf
                .push(fd.try_clone_to_owned().expect("failed to dup fd"));
        }
        inner.submit_write();
    }
}

#[derive(State)]
pub struct Wayland {
    pub(crate) data: Rc<RefCell<WaylandInner>>,
}

impl Wayland {
    pub fn proxy(&self) -> WaylandProxy {
        WaylandProxy(Rc::clone(&self.data))
    }

    pub fn get_interface(&self, id: ObjectId) -> Option<&'static str> {
        self.data.borrow().get_interface(id)
    }

    pub fn new_handle<T: Interface>(&self, id: ObjectId) -> Handle<T> {
        self.proxy().new_handle(id)
    }

    pub fn get_handle<T: Interface>(&self, id: ObjectId) -> Option<Handle<T>> {
        self.proxy().get_handle(id)
    }

    pub fn alloc_handle<T: Interface>(&self) -> Handle<T> {
        self.proxy().alloc_handle()
    }

    pub fn display(&self) -> Handle<WlDisplay> {
        self.proxy()
            .get_handle::<WlDisplay>(ObjectId(1))
            .expect("display always exists")
    }

    pub fn invalidate_object(&self, id: ObjectId) {
        self.data.borrow_mut().invalidate_object(id);
    }

    pub fn take_fd(&self) -> Option<OwnedFd> {
        self.data.borrow_mut().fd_queue.pop_front()
    }
}

pub struct Handle<T: Interface> {
    slot: Weak<ObjectId>,
    pub proxy: WaylandProxy,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Interface> Handle<T> {
    pub fn object_id(&self) -> Option<ObjectId> {
        self.slot.upgrade().map(|rc| *rc)
    }

    pub fn is_alive(&self) -> bool {
        self.slot.strong_count() > 0
    }
}

impl<T: Interface> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Self {
            slot: self.slot.clone(),
            proxy: self.proxy.clone(),
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T: Interface> std::fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle")
            .field("interface", &T::NAME)
            .field("object_id", &self.slot.upgrade().map(|rc| rc.0))
            .finish()
    }
}
