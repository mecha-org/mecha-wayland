pub mod proto;
pub mod wire;

pub mod wl_buffer;
pub mod wl_callback;
pub mod wl_compositor;
pub mod wl_display;
pub mod wl_keyboard;
pub mod wl_pointer;
pub mod wl_registry;
pub mod wl_seat;
pub mod wl_shm;
pub mod wl_surface;
pub mod wl_touch;
pub mod zwlr_layer_shell;
pub mod zwp_linux_dmabuf;

pub use wl_buffer::{WlBuffer, WlBufferEvent};
pub use wl_callback::{WlCallback, WlCallbackEvent};
pub use wl_compositor::WlCompositor;
pub use wl_display::WlDisplay;
pub use wl_keyboard::{KeyState, KeyboardEvent, KeymapFormat, WlKeyboard};
pub use wl_pointer::{ButtonState, PointerAxis, PointerAxisSource, PointerEvent, WlPointer};
pub use wl_registry::WlRegistry;
pub use wl_seat::{CAP_KEYBOARD, CAP_POINTER, CAP_TOUCH, SeatEvent, WlSeat};
pub use wl_shm::WlShm;
pub use wl_surface::WlSurface;
pub use wl_touch::{TouchEvent, WlTouch};
pub use zwlr_layer_shell::{
    Anchor, KeyboardInteractivity, Layer, LayerSurfaceEvent, ZwlrLayerShellV1, ZwlrLayerSurfaceV1,
};
pub use zwp_linux_dmabuf::{ZwpLinuxBufferParamsV1, ZwpLinuxDmabufV1};

use std::cell::RefCell;
use std::collections::VecDeque;
use std::env;
use std::mem;
use std::os::fd::{IntoRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::rc::Rc;

use app::RegisteredModule as _;
use io_ring::{IoEvent, IoToken, RingProxy};
use io_uring::{opcode, types};

use crate::wire::{HEADER_SIZE, MessageBuilder, MessageHeader};
use proto::{Handle, WaylandInterface, WaylandParse, WaylandSend};

#[derive(Debug)]
pub struct Initilised;
impl app::Event for Initilised {}

#[derive(Debug)]
pub struct WaylandRawEvent {
    pub sender_id: u32,
    pub opcode: u16,
    pub body: Vec<u8>,
}

impl app::Event for WaylandRawEvent {}

// ── send / parse free functions ───────────────────────────────────────────────

pub fn send<M: WaylandSend>(conn: &SharedConnection, handle: &Handle<M::Interface>, msg: &M) {
    let mut c = conn.borrow_mut();
    let builder = c.message_builder(handle.id, M::OPCODE);
    msg.serialize(builder);
}

pub fn parse<M: WaylandParse>(raw: &WaylandRawEvent) -> Option<M> {
    if raw.opcode != M::OPCODE {
        return None;
    }
    M::deserialize(&raw.body)
}

// ── Connection ────────────────────────────────────────────────────────────────

pub struct Connection {
    pub fd: RawFd,
    pub write_buf: Vec<u8>,
    pub fds_buf: Vec<RawFd>,
    next_id: u32,
}

impl Connection {
    pub fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn push_fd(&mut self, fd: RawFd) {
        self.fds_buf.push(fd);
    }

    pub fn message_builder(&mut self, sender_id: u32, opcode: u16) -> MessageBuilder<'_> {
        MessageBuilder::new(&mut self.write_buf, &mut self.fds_buf, sender_id, opcode)
    }
}

pub type SharedConnection = Rc<RefCell<Connection>>;

// ── Wayland ───────────────────────────────────────────────────────────────────

pub struct Wayland {
    conn: SharedConnection,
    pub ring_proxy: RingProxy,
    read_buf: Vec<u8>,
    read_token: Option<IoToken>,
    write_in_flight: Vec<u8>,
    write_token: Option<IoToken>,
    recv_overflow: Vec<u8>,

    pub pending: VecDeque<WaylandRawEvent>,

    pub display: WlDisplay,
    pub registry: WlRegistry,
    pub callback: WlCallback,
    pub compositor: WlCompositor,
    pub surface: WlSurface,
    pub layer_shell: ZwlrLayerShellV1,
    pub layer_surface: ZwlrLayerSurfaceV1,
    pub seat: WlSeat,
    pub pointer: WlPointer,
    pub keyboard: WlKeyboard,
    pub touch: WlTouch,
    pub dmabuf: ZwpLinuxDmabufV1,
    pub buf_params: ZwpLinuxBufferParamsV1,
    pub wl_buffer: WlBuffer,
}

impl Wayland {
    pub fn new(ring_proxy: RingProxy) -> std::io::Result<Self> {
        let xdg_runtime_dir =
            env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".into());
        let wayland_display = env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
        let path = PathBuf::from(xdg_runtime_dir).join(wayland_display);

        let stream = UnixStream::connect(path)?;
        stream.set_nonblocking(true)?;
        let fd = stream.into_raw_fd();

        let conn: SharedConnection = Rc::new(RefCell::new(Connection {
            fd,
            write_buf: Vec::with_capacity(4096),
            fds_buf: Vec::new(),
            next_id: 2,
        }));

        Ok(Self {
            display: WlDisplay::new(conn.clone()),
            registry: WlRegistry::new(conn.clone()),
            callback: WlCallback::new(conn.clone()),
            compositor: WlCompositor::new(conn.clone()),
            surface: WlSurface::new(conn.clone()),
            layer_shell: ZwlrLayerShellV1::new(conn.clone()),
            layer_surface: ZwlrLayerSurfaceV1::new(conn.clone()),
            seat: WlSeat::new(conn.clone()),
            pointer: WlPointer::new(conn.clone()),
            keyboard: WlKeyboard::new(conn.clone()),
            touch: WlTouch::new(conn.clone()),
            dmabuf: ZwpLinuxDmabufV1::new(conn.clone()),
            buf_params: ZwpLinuxBufferParamsV1::new(conn.clone()),
            wl_buffer: WlBuffer::new(conn.clone()),
            conn,
            ring_proxy,
            read_buf: vec![0u8; 4096],
            read_token: None,
            write_in_flight: Vec::new(),
            write_token: None,
            recv_overflow: Vec::new(),
            pending: VecDeque::new(),
        })
    }

    pub fn init(&mut self) {
        let fd = self.conn.borrow().fd;

        let registry_id = self.conn.borrow_mut().alloc_id();
        let callback_id = self.conn.borrow_mut().alloc_id();
        self.registry.set_id(registry_id);
        self.callback.set_id(callback_id);

        self.display.get_registry(registry_id);
        self.display.sync(callback_id);

        unsafe { libc::fcntl(fd, libc::F_SETFL, 0) };
        self.flush_sync(fd);

        loop {
            let (sender_id, opcode, body) = self.recv_sync(fd);
            self.display.handle_event_sync(sender_id, opcode, &body);
            self.registry.handle_event_sync(sender_id, opcode, &body);
            self.callback.handle_event_sync(sender_id, opcode, &body);
            if self.callback.is_done() {
                break;
            }
        }

        let (comp_name, comp_ver) = self
            .registry
            .find("wl_compositor")
            .expect("wl_compositor not found");
        let (_shm_name, _shm_ver) = self.registry.find("wl_shm").expect("wl_shm not found");
        let (layer_name, layer_ver) = self
            .registry
            .find("zwlr_layer_shell_v1")
            .expect("zwlr_layer_shell_v1 not found");

        let comp_id = self.conn.borrow_mut().alloc_id();
        let layer_id = self.conn.borrow_mut().alloc_id();

        self.compositor.set_id(comp_id);
        self.layer_shell.set_id(layer_id);

        self.registry
            .bind(comp_name, "wl_compositor", comp_ver.min(4), comp_id);
        self.registry.bind(
            layer_name,
            "zwlr_layer_shell_v1",
            layer_ver.min(4),
            layer_id,
        );

        if let Some((seat_name, seat_ver)) = self.registry.find("wl_seat") {
            let seat_id = self.conn.borrow_mut().alloc_id();
            self.seat.set_id(seat_id);
            self.registry
                .bind(seat_name, "wl_seat", seat_ver.min(7), seat_id);
        }

        if let Some((dmabuf_name, dmabuf_ver)) = self.registry.find("zwp_linux_dmabuf_v1") {
            let dmabuf_id = self.conn.borrow_mut().alloc_id();
            self.dmabuf.set_id(dmabuf_id);
            self.registry.bind(
                dmabuf_name,
                "zwp_linux_dmabuf_v1",
                dmabuf_ver.min(3),
                dmabuf_id,
            );
        }

        self.flush_sync(fd);
        unsafe { libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK) };
        self.submit_read();
    }

    pub fn flush(&mut self) {
        if !self.conn.borrow().fds_buf.is_empty() {
            self.send_with_fds();
        } else {
            self.submit_write();
        }
    }

    pub fn handle_io(&mut self, event: &IoEvent) {
        match event {
            IoEvent::Completed { token, result } => {
                if Some(*token) == self.write_token {
                    self.write_token = None;
                    self.write_in_flight.clear();
                    self.submit_write();
                } else if Some(*token) == self.read_token {
                    self.read_token = None;
                    if *result > 0 {
                        let new_bytes = &self.read_buf[..*result as usize];
                        let data = if self.recv_overflow.is_empty() {
                            new_bytes.to_vec()
                        } else {
                            let mut buf = mem::take(&mut self.recv_overflow);
                            buf.extend_from_slice(new_bytes);
                            buf
                        };
                        self.process_messages(data);
                        self.submit_read();
                    } else if *result == 0 {
                        eprintln!("[Wayland] compositor disconnected, exiting");
                        std::process::exit(1);
                    } else {
                        eprintln!("[Wayland] read error: {}", result);
                    }
                }
            }
        }
    }

    fn process_messages(&mut self, data: Vec<u8>) {
        let mut offset = 0;
        while offset + HEADER_SIZE <= data.len() {
            let Some(header) = MessageHeader::parse(&data[offset..]) else {
                break;
            };
            let msg_size = header.size as usize;
            if msg_size < HEADER_SIZE || data[offset..].len() < msg_size {
                self.recv_overflow.extend_from_slice(&data[offset..]);
                return;
            }
            let body = data[offset + HEADER_SIZE..offset + msg_size].to_vec();
            self.pending.push_back(WaylandRawEvent {
                sender_id: header.sender_id,
                opcode: header.opcode,
                body,
            });
            offset += msg_size;
        }
        if offset < data.len() {
            self.recv_overflow.extend_from_slice(&data[offset..]);
        }
    }

    fn submit_write(&mut self) {
        if self.write_token.is_some() || !self.write_in_flight.is_empty() {
            return;
        }
        {
            let mut conn = self.conn.borrow_mut();
            if conn.write_buf.is_empty() {
                return;
            }
            mem::swap(&mut self.write_in_flight, &mut conn.write_buf);
        }
        let sqe = opcode::Write::new(
            types::Fd(self.conn.borrow().fd),
            self.write_in_flight.as_ptr(),
            self.write_in_flight.len() as u32,
        )
        .build();
        let token = self.ring_proxy.push(sqe);
        self.write_token = Some(token);
    }

    fn submit_read(&mut self) {
        if self.read_token.is_some() {
            return;
        }
        let sqe = opcode::Read::new(
            types::Fd(self.conn.borrow().fd),
            self.read_buf.as_mut_ptr(),
            self.read_buf.len() as u32,
        )
        .build();
        let token = self.ring_proxy.push(sqe);
        self.read_token = Some(token);
    }

    fn flush_sync(&self, fd: RawFd) {
        let mut conn = self.conn.borrow_mut();
        let mut offset = 0;
        while offset < conn.write_buf.len() {
            let n = unsafe {
                libc::write(
                    fd,
                    conn.write_buf[offset..].as_ptr() as *const libc::c_void,
                    conn.write_buf.len() - offset,
                )
            };
            assert!(n > 0, "wayland write failed during init");
            offset += n as usize;
        }
        conn.write_buf.clear();
    }

    fn recv_sync(&self, fd: RawFd) -> (u32, u16, Vec<u8>) {
        let mut header = [0u8; 8];
        let mut offset = 0;
        while offset < 8 {
            let n = unsafe {
                libc::read(
                    fd,
                    header[offset..].as_mut_ptr() as *mut libc::c_void,
                    8 - offset,
                )
            };
            assert!(n > 0, "wayland socket closed during init");
            offset += n as usize;
        }
        let sender_id = u32::from_ne_bytes(header[0..4].try_into().unwrap());
        let word2 = u32::from_ne_bytes(header[4..8].try_into().unwrap());
        let total_size = (word2 >> 16) as usize;
        let opcode = (word2 & 0xffff) as u16;
        let body_len = total_size.saturating_sub(8);

        let mut body = vec![0u8; body_len];
        let mut offset = 0;
        while offset < body_len {
            let n = unsafe {
                libc::read(
                    fd,
                    body[offset..].as_mut_ptr() as *mut libc::c_void,
                    body_len - offset,
                )
            };
            assert!(n > 0, "wayland socket closed during init");
            offset += n as usize;
        }
        (sender_id, opcode, body)
    }

    fn send_with_fds(&mut self) {
        let mut conn = self.conn.borrow_mut();
        if conn.write_buf.is_empty() {
            return;
        }
        let payload = mem::take(&mut conn.write_buf);
        let raw_fds: Vec<RawFd> = mem::take(&mut conn.fds_buf);
        let fd = conn.fd;
        drop(conn);

        let fd_bytes = raw_fds.len() * mem::size_of::<RawFd>();
        let cmsg_space = unsafe { libc::CMSG_SPACE(fd_bytes as u32) as usize };
        let mut cmsg_buf = vec![0u8; cmsg_space];

        let iov = libc::iovec {
            iov_base: payload.as_ptr() as *mut libc::c_void,
            iov_len: payload.len(),
        };
        let mut mhdr: libc::msghdr = unsafe { mem::zeroed() };
        mhdr.msg_iov = &iov as *const _ as *mut _;
        mhdr.msg_iovlen = 1;
        mhdr.msg_control = cmsg_buf.as_mut_ptr() as *mut libc::c_void;
        mhdr.msg_controllen = cmsg_space as _;

        unsafe {
            let cmsg = libc::CMSG_FIRSTHDR(&mhdr);
            (*cmsg).cmsg_level = libc::SOL_SOCKET;
            (*cmsg).cmsg_type = libc::SCM_RIGHTS;
            (*cmsg).cmsg_len = libc::CMSG_LEN(fd_bytes as u32) as _;
            std::ptr::copy_nonoverlapping(
                raw_fds.as_ptr(),
                libc::CMSG_DATA(cmsg) as *mut RawFd,
                raw_fds.len(),
            );
            libc::sendmsg(fd, &mhdr, 0);
        }
    }
}

pub fn module<AppState>() -> impl app::RegisteredModule<Wayland, AppState> {
    app::Module::<Wayland, _, _>::new()
        .on(|wl: &mut Wayland, _: &app::Start| {
            wl.init();
            Some(Initilised)
        })
        .on(|wl: &mut Wayland, ev: &io_ring::IoEvent| {
            wl.handle_io(ev);
        })
        .on(|wl: &mut Wayland, _: &app::PrePoll| {
            if wl.pending.len() > 1 {
                wl.ring_proxy.skip_next_wait();
            }
            wl.pending.pop_front()
        })
        .mount(
            |wl| &mut wl.display,
            wl_display::module::<AppState>().into_module(),
        )
        .mount(
            |wl| &mut wl.registry,
            wl_registry::module::<AppState>().into_module(),
        )
        .mount(
            |wl| &mut wl.callback,
            wl_callback::module::<AppState>().into_module(),
        )
        .mount(
            |wl| &mut wl.surface,
            wl_surface::module::<AppState>().into_module(),
        )
        .mount(
            |wl| &mut wl.layer_surface,
            zwlr_layer_shell::module::<AppState>().into_module(),
        )
        .mount(
            |wl| &mut wl.seat,
            wl_seat::module::<AppState>().into_module(),
        )
        .mount(
            |wl| &mut wl.pointer,
            wl_pointer::module::<AppState>().into_module(),
        )
        .mount(
            |wl| &mut wl.keyboard,
            wl_keyboard::module::<AppState>().into_module(),
        )
        .mount(
            |wl| &mut wl.touch,
            wl_touch::module::<AppState>().into_module(),
        )
        .mount(
            |wl| &mut wl.dmabuf,
            zwp_linux_dmabuf::module::<AppState>().into_module(),
        )
        .mount(
            |wl| &mut wl.wl_buffer,
            wl_buffer::module::<AppState>().into_module(),
        )
        .on(|wl: &mut Wayland, ev: &SeatEvent| match ev {
            SeatEvent::Capabilities { capabilities } => {
                if (capabilities & CAP_POINTER) != 0 && wl.pointer.id() == 0 {
                    let id = wl.seat.get_pointer();
                    wl.pointer.set_id(id);
                }
                if (capabilities & CAP_KEYBOARD) != 0 && wl.keyboard.id() == 0 {
                    let id = wl.seat.get_keyboard();
                    wl.keyboard.set_id(id);
                }
                if (capabilities & CAP_TOUCH) != 0 && wl.touch.id() == 0 {
                    let id = wl.seat.get_touch();
                    wl.touch.set_id(id);
                }
                wl.flush();
            }
            _ => {}
        })
}
