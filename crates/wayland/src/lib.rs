use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::mem;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use app::{Many, RegisteredModule, prelude::*};
use io_ring::{IoEvent, IoToken, RingProxy};
use io_uring::{opcode, types};

pub(crate) mod helper;
pub mod proto;

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

const CMSG_BUF_SIZE: usize = 256; // enough for 28 fds on any platform

struct WaylandInner {
    fd: RawFd,
    ring_proxy: RingProxy,

    // Read side
    read_buf: Vec<u8>,
    cmsg_recv_buf: Vec<u8>,
    recv_iov: libc::iovec,
    recv_msghdr: libc::msghdr,

    // Write side
    write_buf: Vec<u8>,
    write_fds_buf: Vec<OwnedFd>,
    write_in_flight: Vec<u8>,
    write_fds_in_flight: Vec<OwnedFd>,
    write_cmsg_buf: Vec<u8>,
    send_iov: libc::iovec,
    send_msghdr: libc::msghdr,

    // Fd queue populated from recvmsg ancillary data; consumed by generated parse functions
    fd_queue: VecDeque<OwnedFd>,

    read_token: Option<IoToken>,
    write_token: Option<IoToken>,
    next_id: u32,
    object_slots: HashMap<ObjectId, Rc<ObjectId>>,
    object_interfaces: HashMap<ObjectId, &'static str>,
    deleted_object_ids: HashSet<ObjectId>,
}

impl WaylandInner {
    fn submit_read(&mut self) {
        // SAFETY: WaylandInner lives at a stable heap address (behind Rc<RefCell>).
        // read_buf and cmsg_recv_buf data pointers are stable (never reallocated).
        // recv_iov and recv_msghdr fields are at stable offsets within that allocation.
        // All pointed-to memory remains valid until the matching CQE clears read_token.
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

    fn extract_recv_fds(&mut self) {
        // SAFETY: recv_msghdr was filled by the kernel during RecvMsg.
        // msg_controllen reflects actual received control data length.
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

    fn submit_write(&mut self) {
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
            // SAFETY: Same stability argument as submit_read.
            // write_in_flight, write_cmsg_buf, and write_fds_in_flight are kept alive
            // as fields of WaylandInner until the CQE clears write_token.
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

    fn get_interface(&self, id: ObjectId) -> Option<&'static str> {
        self.object_interfaces.get(&id).copied()
    }

    fn alloc_id(&mut self) -> ObjectId {
        if let Some(&id) = self.deleted_object_ids.iter().next() {
            self.deleted_object_ids.remove(&id);
            id
        } else {
            let id = ObjectId(self.next_id);
            self.next_id += 1;
            id
        }
    }

    fn invalidate_object(&mut self, id: ObjectId) {
        self.object_slots.remove(&id);
        self.object_interfaces.remove(&id);
        self.deleted_object_ids.insert(id);
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
    }
}

#[derive(State)]
pub struct Wayland {
    data: Rc<RefCell<WaylandInner>>,
}

impl Wayland {
    pub fn new(ring_proxy: RingProxy) -> Self {
        let xdg_runtime_dir =
            env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".into());
        let wayland_display = env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
        let path = PathBuf::from(xdg_runtime_dir).join(wayland_display);

        let stream = UnixStream::connect(&path).expect("failed to connect to wayland socket");
        stream.set_nonblocking(true).unwrap();
        let fd = stream.into_raw_fd();

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
            next_id: 2,
            object_slots,
            object_interfaces,
            deleted_object_ids: HashSet::new(),
        };
        let data = Rc::new(RefCell::new(inner));
        data.borrow_mut().submit_read();

        Self { data }
    }

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
    pub(crate) proxy: WaylandProxy,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Interface> Handle<T> {
    pub fn name() -> &'static str {
        T::NAME
    }

    pub fn version() -> u32 {
        T::VERSION
    }

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

pub fn module<S>() -> impl app::RegisteredModule<Wayland, S> {
    let m = Module::<Wayland, _, _>::new()
        .on(|wayland: &mut Wayland, io_event: &IoEvent| {
            let IoEvent::Completed { token, result } = io_event;
            let events = {
                let mut inner = wayland.data.borrow_mut();
                if Some(*token) == inner.read_token {
                    inner.read_token = None;
                    let n = *result;
                    if n <= 0 {
                        panic!("wayland socket error: {n}");
                    }
                    let bytes = inner.read_buf[..n as usize].to_vec();
                    inner.extract_recv_fds();
                    drop(inner);
                    wayland.data.borrow_mut().submit_read();
                    helper::parse_messages(bytes)
                } else if Some(*token) == inner.write_token {
                    inner.write_token = None;
                    inner.write_in_flight.clear();
                    inner.write_fds_in_flight.clear();
                    inner.submit_write();
                    Vec::new()
                } else {
                    Vec::new()
                }
            };
            Many(events.into_iter())
        })
        .mount(proto::manual::module::<S>().into_module())
        .mount(proto::generated::module::<S>().into_module());
    #[cfg(feature = "client")]
    let m = m.on(|wayland: &mut Wayland, event: &WlDisplayEvent| {
        if let WlDisplayEvent::DeleteId { id, .. } = event {
            wayland.invalidate_object(ObjectId(*id));
        }
    });

    m
}
