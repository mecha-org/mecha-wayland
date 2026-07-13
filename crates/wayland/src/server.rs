use std::cell::RefCell;
use std::collections::HashMap;
use std::mem;
use std::os::fd::{FromRawFd, IntoRawFd, OwnedFd, RawFd};
use std::path::PathBuf;
use std::rc::Rc;
use std::env;

use app::{Many, PrePoll, RegisteredModule, prelude::*};
use io_ring::{IoEvent, IoToken, RingProxy};
use io_uring::{opcode, types};

use crate::{Handle, IoCompletion, RawWaylandEvent, Wayland, handle_io_event, make_inner, proto};
use crate::proto::manual::WlCallback;

pub const SERVER_ID_START: u32 = 0xFF000000;

impl Wayland {
    pub fn new_server(fd: OwnedFd, ring_proxy: RingProxy) -> Self {
        let data = make_inner(fd.into_raw_fd(), ring_proxy, SERVER_ID_START);
        data.borrow_mut().submit_read();
        Wayland { data }
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct ClientId(pub(crate) u32);

pub(crate) struct ClientConn {
    pub(crate) conn: Wayland,
    pub(crate) pending_sync: Option<Handle<WlCallback>>,
}

#[derive(Debug)]
pub struct ClientConnected {
    pub id: ClientId,
}
impl Event for ClientConnected {}

#[derive(Debug)]
pub struct ClientRawEvent {
    pub client_id: ClientId,
    pub(crate) raw: RawWaylandEvent,
}
impl Event for ClientRawEvent {}

pub(crate) struct WaylandServerInner {
    listen_fd: RawFd,
    ring_proxy: RingProxy,
    accept_token: Option<IoToken>,
    accept_addr: libc::sockaddr_un,
    accept_addrlen: libc::socklen_t,
    #[allow(dead_code)]
    socket_path: PathBuf,
    next_client_id: u32,
    pub(crate) clients: HashMap<ClientId, ClientConn>,
}

impl WaylandServerInner {
    fn submit_accept(&mut self) {
        unsafe {
            let sqe = opcode::Accept::new(
                types::Fd(self.listen_fd),
                &mut self.accept_addr as *mut libc::sockaddr_un as *mut libc::sockaddr,
                &mut self.accept_addrlen as *mut libc::socklen_t,
            )
            .build();
            let token = self.ring_proxy.push(sqe);
            self.accept_token = Some(token);
        }
    }
}

#[derive(State)]
pub struct WaylandServer {
    pub(crate) data: Rc<RefCell<WaylandServerInner>>,
}

impl WaylandServer {
    pub fn set_pending_sync(&self, client_id: ClientId, callback: Handle<WlCallback>) {
        let mut inner = self.data.borrow_mut();
        if let Some(client) = inner.clients.get_mut(&client_id) {
            client.pending_sync = Some(callback);
        }
    }

    pub fn new(socket_name: &str, ring_proxy: RingProxy) -> Self {
        let xdg_runtime_dir =
            env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".into());
        let socket_path = PathBuf::from(&xdg_runtime_dir).join(socket_name);

        let _ = std::fs::remove_file(&socket_path);

        let path_bytes = socket_path.as_os_str().as_encoded_bytes();
        assert!(path_bytes.len() < 108, "socket path too long for sun_path");

        let mut bind_addr: libc::sockaddr_un = unsafe { mem::zeroed() };
        bind_addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
        unsafe {
            std::ptr::copy_nonoverlapping(
                path_bytes.as_ptr(),
                bind_addr.sun_path.as_mut_ptr() as *mut u8,
                path_bytes.len(),
            );
        }
        let addr_len = (std::mem::offset_of!(libc::sockaddr_un, sun_path) + path_bytes.len() + 1)
            as libc::socklen_t;

        let listen_fd = unsafe {
            let fd = libc::socket(
                libc::AF_UNIX,
                libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                0,
            );
            assert!(fd >= 0, "socket failed: {}", std::io::Error::last_os_error());
            let ret = libc::bind(fd, &bind_addr as *const _ as *const libc::sockaddr, addr_len);
            assert!(ret == 0, "bind failed: {}", std::io::Error::last_os_error());
            let ret = libc::listen(fd, 128);
            assert!(ret == 0, "listen failed: {}", std::io::Error::last_os_error());
            fd
        };

        let mut inner = WaylandServerInner {
            listen_fd,
            ring_proxy: ring_proxy.clone(),
            accept_token: None,
            accept_addr: unsafe { mem::zeroed() },
            accept_addrlen: std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t,
            socket_path,
            next_client_id: 0,
            clients: HashMap::new(),
        };
        inner.submit_accept();

        Self { data: Rc::new(RefCell::new(inner)) }
    }
}

pub fn server_module<S>() -> impl app::RegisteredModule<WaylandServer, S> {
    Module::<WaylandServer, _, _>::new()
        // Drain pending syncs and re-arm reads for all clients.
        .on(|server: &mut WaylandServer, _: &PrePoll| {
            let mut inner = server.data.borrow_mut();
            for (_, client) in inner.clients.iter_mut() {
                if let Some(cb) = client.pending_sync.take() {
                    cb.done(0);
                }
                client.conn.data.borrow_mut().submit_read();
            }
            hlist![]
        })
        // Accept new client connections.
        .on(|server: &mut WaylandServer, io_event: &IoEvent| {
            let IoEvent::Completed { token, result } = io_event;
            let mut inner = server.data.borrow_mut();
            if inner.accept_token != Some(*token) {
                return None;
            }
            inner.accept_token = None;
            let event = if *result >= 0 {
                let id = ClientId(inner.next_client_id);
                inner.next_client_id += 1;
                let fd = unsafe { OwnedFd::from_raw_fd(*result) };
                let conn = Wayland::new_server(fd, inner.ring_proxy.clone());
                inner.clients.insert(id, ClientConn { conn, pending_sync: None });
                Some(ClientConnected { id })
            } else {
                None
            };
            inner.submit_accept();
            event
        })
        // Route completed I/O for each connected client.
        .on(|server: &mut WaylandServer, io_event: &IoEvent| {
            let IoEvent::Completed { token, result } = io_event;
            let inner = server.data.borrow();

            let matching_id = inner.clients.iter().find_map(|(id, c)| {
                let d = c.conn.data.borrow();
                if d.read_token == Some(*token) || d.write_token == Some(*token) {
                    Some(*id)
                } else {
                    None
                }
            });

            drop(inner);

            let raw_events: Vec<ClientRawEvent> = if let Some(client_id) = matching_id {
                let inner = server.data.borrow();
                let client = &inner.clients[&client_id];
                let completion = handle_io_event(&client.conn.data, *token, *result);
                drop(inner);
                match completion {
                    IoCompletion::Read(events) => events
                        .into_iter()
                        .map(|raw| ClientRawEvent { client_id, raw })
                        .collect(),
                    IoCompletion::Disconnect => {
                        server.data.borrow_mut().clients.remove(&client_id);
                        vec![]
                    }
                    _ => vec![],
                }
            } else {
                vec![]
            };

            Many(raw_events.into_iter())
        })
        .mount(proto::manual::server_dispatch_module::<S>().into_module())
        .mount(proto::generated::server_dispatch_module::<S>().into_module())
}
