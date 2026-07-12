use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    env,
    marker::PhantomData,
    os::{
        fd::{AsRawFd, IntoRawFd, OwnedFd, RawFd},
        unix::net::UnixStream,
    },
    rc::Rc,
};

use app::{Event, Many, Module, RegisteredModule, prelude::State};
use io_ring::{IoEvent, IoToken, RingProxy};
use io_uring::{opcode, types};
use serde::Serialize;
use zbus::zvariant::serialized::{Context, Data};
use zbus::zvariant::{LE, OwnedValue};
use zbus::{
    message::{Message, Type as MessageType},
    zvariant::DynamicType,
};

use crate::{
    dbus::{DbusMethod, DbusSignal, MatchRule, Subscription},
    fd::{CMSG_BUF, build_scm_rights, dbus_unix_fd_count, dup_owned, parse_scm_rights},
    fdo,
    util::{dbus_message_len, parse_unix_path, sasl_handshake},
};

const READ_CHUNK: usize = 8192;

/// One queued outgoing message: its bytes plus any fds to pass alongside it. The
/// fds are our own dup'd copies, held open until the `SendMsg` completes.
pub struct OutFrame {
    bytes: Vec<u8>,
    fds: Vec<OwnedFd>,
}

pub trait Bus: 'static {
    const NAME: &'static str;

    fn address() -> String;
}

// DBUS_SESSION_BUS_ADDRESS
#[derive(Debug, Clone, Copy)]
pub struct SessionBus;
impl Bus for SessionBus {
    const NAME: &'static str = "session";
    fn address() -> String {
        env::var("DBUS_SESSION_BUS_ADDRESS").expect("DBUS_SESSION_BUS_ADDRESS not set")
    }
}

// DBUS_SYSTEM_BUS_ADDRESS
#[derive(Debug, Clone, Copy)]
pub struct SystemBus;
impl Bus for SystemBus {
    const NAME: &'static str = "system";
    fn address() -> String {
        env::var("DBUS_SYSTEM_BUS_ADDRESS")
            .unwrap_or_else(|_| "unix:path=/var/run/dbus/system_bus_socket".into())
    }
}

#[derive(Debug)]
pub enum DbusMessage {
    // Signal broadcast from a service
    Signal(Rc<Message>),
    // Reply from a method
    Reply { serial: u32, message: Rc<Message> },
    // A method call addressed to the service
    Call(Rc<Message>),
}

fn method_message<M: DbusMethod>(path: &str, args: &M::Args) -> Message {
    Message::method_call(path, M::MEMBER)
        .expect("valid path/member")
        .destination(M::DESTINATION)
        .expect("valid destination")
        .interface(M::INTERFACE)
        .expect("valid interface")
        .build(args)
        .expect("serialize method call")
}

pub struct DbusEvent<B: Bus> {
    pub msg: DbusMessage,
    _bus: PhantomData<B>,
}

impl<B: Bus> DbusEvent<B> {
    fn new(msg: DbusMessage) -> Self {
        Self {
            msg,
            _bus: PhantomData,
        }
    }
}

impl<B: Bus> std::fmt::Debug for DbusEvent<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbusEvent")
            .field("bus", &B::NAME)
            .field("msg", &self.msg)
            .finish()
    }
}

impl<B: Bus> Event for DbusEvent<B> {}

struct DbusInner {
    fd: RawFd,
    ring: RingProxy,

    // scratch for kernel to write
    read_scratch: Box<[u8; READ_CHUNK]>,
    // iov/cmsg/msghdr drive `RecvMsg`
    read_iov: Box<libc::iovec>,
    read_cmsg: Box<[u8; CMSG_BUF]>,
    read_msghdr: Box<libc::msghdr>,
    // accumulator for full message bytes and a FIFO of fds received
    read_acc: Vec<u8>,
    in_fds: VecDeque<OwnedFd>,
    read_token: Option<IoToken>,

    // outgoing frames (bytes+fds), one `SendMsg` per frame
    // so each message's fds are included with its bytes
    out: VecDeque<OutFrame>,
    // in flight bytes
    in_flight: Option<OutFrame>,
    write_iov: Box<libc::iovec>,
    write_cmsg: Box<[u8; CMSG_BUF]>,
    write_msghdr: Box<libc::msghdr>,
    write_token: Option<IoToken>,

    // Dbus standard - serial for hello, and unique_name returned in handshake
    hello_serial: Option<u32>,
    unique_name: Option<String>,
}

impl DbusInner {
    /// Arm a single `Recv`. Called once at startup and re-armed after every read completion
    fn submit_read(&mut self) {
        if self.read_token.is_some() {
            return;
        }
        let scratch_ptr = self.read_scratch.as_mut_ptr() as *mut libc::c_void;
        self.read_iov.iov_base = scratch_ptr;
        self.read_iov.iov_len = READ_CHUNK;
        let iov_ptr = self.read_iov.as_mut() as *mut libc::iovec;
        let cmsg_ptr = self.read_cmsg.as_mut_ptr() as *mut libc::c_void;
        let hdr = self.read_msghdr.as_mut();
        hdr.msg_name = std::ptr::null_mut();
        hdr.msg_namelen = 0;
        hdr.msg_iov = iov_ptr;
        hdr.msg_iovlen = 1;
        hdr.msg_control = cmsg_ptr;
        hdr.msg_controllen = CMSG_BUF as _;
        hdr.msg_flags = 0;

        // SAFETY: scratch, iovec, cmsg buffer and msghdr all live behind
        // Rc<RefCell<DbusInner>> at stable heap addresses, untouched until the
        // CQE clears `read_token`. MSG_CMSG_CLOEXEC keeps received fds from
        // leaking across exec.
        let sqe = opcode::RecvMsg::new(
            types::Fd(self.fd),
            self.read_msghdr.as_mut() as *mut libc::msghdr,
        )
        .flags(libc::MSG_CMSG_CLOEXEC as u32)
        .build();
        self.read_token = Some(self.ring.push(sqe));
    }

    /// Arm a `SendMsg` for the next queued frame, carrying its fds (if any) as
    /// `SCM_RIGHTS`. One message per send so fds stay associated with bytes.
    fn submit_write(&mut self) {
        if self.write_token.is_some() || self.in_flight.is_some() {
            return;
        }
        let Some(frame) = self.out.pop_front() else {
            return;
        };
        self.in_flight = Some(frame);

        let (bytes_ptr, bytes_len, raw_fds) = {
            let f = self.in_flight.as_ref().unwrap();
            let raw: Vec<RawFd> = f.fds.iter().map(|fd| fd.as_raw_fd()).collect();
            (f.bytes.as_ptr() as *mut libc::c_void, f.bytes.len(), raw)
        };
        self.write_iov.iov_base = bytes_ptr;
        self.write_iov.iov_len = bytes_len;
        let iov_ptr = self.write_iov.as_mut() as *mut libc::iovec;

        // SAFETY: write_cmsg is sized for CMSG_BUF; build_scm_rights asserts fit.
        let controllen = unsafe { build_scm_rights(&mut self.write_cmsg[..], &raw_fds) };
        let cmsg_ptr = if controllen > 0 {
            self.write_cmsg.as_mut_ptr() as *mut libc::c_void
        } else {
            std::ptr::null_mut()
        };

        let hdr = self.write_msghdr.as_mut();
        hdr.msg_name = std::ptr::null_mut();
        hdr.msg_namelen = 0;
        hdr.msg_iov = iov_ptr;
        hdr.msg_iovlen = 1;
        hdr.msg_control = cmsg_ptr;
        hdr.msg_controllen = controllen as _;
        hdr.msg_flags = 0;

        // SAFETY: frame bytes/fds (in `in_flight`), iovec, cmsg and msghdr all
        // live behind Rc<RefCell> at stable addresses until the CQE clears
        // `write_token`; the fds are our dup'd copies, closed only on completion.
        let sqe = opcode::SendMsg::new(
            types::Fd(self.fd),
            self.write_msghdr.as_ref() as *const libc::msghdr,
        )
        .build();
        self.write_token = Some(self.ring.push(sqe));
    }

    /// Queue a built message and return its zbus-assigned serial. Any fds the
    /// message carries are dup'd into copies we own until the send completes.
    fn send(&mut self, msg: &Message) -> u32 {
        let serial = u32::from(msg.primary_header().serial_num());
        let bytes: Vec<u8> = msg.data().iter().copied().collect();
        // NOTE (verify against your zbus pin): `data().fds()` yields the message's
        // fds; we dup each so the OutFrame owns them independent of the message.
        let fds: Vec<OwnedFd> = msg
            .data()
            .fds()
            .iter()
            .map(|fd| dup_owned(fd.as_raw_fd()))
            .collect();
        self.out.push_back(OutFrame { bytes, fds });
        self.submit_write();
        serial
    }

    /// Pull every *complete* message from `read_acc`, leaving any trailing
    /// partial message in place for the next read.
    fn drain_complete_messages(&mut self) -> Vec<Rc<Message>> {
        let mut out = Vec::new();
        loop {
            let Some(total) = dbus_message_len(&self.read_acc) else {
                break; // not enough bytes to even know the length yet
            };
            if self.read_acc.len() < total {
                break; // header known, body not fully arrived yet
            }
            let frame: Vec<u8> = self.read_acc.drain(..total).collect();
            // Hand this message exactly the fds it claims (UNIX_FDS header field),
            // pulled in order from the received-fd FIFO.
            let want = dbus_unix_fd_count(&frame) as usize;
            let mut fds: Vec<OwnedFd> = Vec::with_capacity(want);
            for _ in 0..want {
                match self.in_fds.pop_front() {
                    Some(fd) => fds.push(fd),
                    None => {
                        eprintln!("dbus: message claims {want} fds but FIFO is short");
                        break;
                    }
                }
            }

            let ctx = Context::new_dbus(LE, 0);
            // NOTE (verify against your zbus pin): `Data::new_fds` attaches fds so
            // the body's `h` indices resolve.
            let data = if fds.is_empty() {
                Data::new(frame, ctx)
            } else {
                Data::new_fds(frame, ctx, fds)
            };
            match unsafe { Message::from_bytes(data) } {
                Ok(msg) => out.push(Rc::new(msg)),
                Err(e) => eprintln!("dbus: dropping undecodable message: {e}"),
            }
        }
        out
    }
}

#[derive(Clone)]
pub struct DbusProxy<B: Bus> {
    inner: Rc<RefCell<DbusInner>>,
    _bus: PhantomData<B>,
}

impl<B: Bus> DbusProxy<B> {
    /// Call a method at its declared `PATH`.
    pub fn call<M: DbusMethod>(&self, args: &M::Args) -> u32 {
        self.call_at::<M>(M::PATH, args)
    }

    /// Call the same method at a runtime-chosen object path.
    pub fn call_at<M: DbusMethod>(&self, path: &str, args: &M::Args) -> u32 {
        let msg = method_message::<M>(path, args);
        self.inner.borrow_mut().send(&msg)
    }

    /// Subscribe to a typed signal (installs its generated match rule). Returns
    /// a `Subscription` you can later pass to `unsubscribe`.
    pub fn subscribe<S: DbusSignal>(&self) -> Subscription {
        self.add_match_rule(&S::match_rule().to_string())
    }

    /// Subscribe with an explicit (e.g. path-narrowed) match rule.
    /// Example
    /// ```rust
    ///     proxy.subscribe_rule(StateChanged::match_rule().path("/org/freedesktop/NetworkManager/Devices/3"));
    /// ```
    pub fn subscribe_rule<S: DbusSignal>(&self, rule: MatchRule<S>) -> Subscription {
        self.add_match_rule(&rule.to_string())
    }

    /// Escape hatch: raw match-rule string.
    pub fn add_match_rule(&self, rule: &str) -> Subscription {
        self.call::<fdo::AddMatch>(&(rule.to_string(),));
        Subscription {
            rule: rule.to_string(),
        }
    }

    /// Remove a previously installed subscription (sends `RemoveMatch` with the
    /// exact rule string the bus recorded). Returns the call serial.
    pub fn unsubscribe(&self, sub: &Subscription) -> u32 {
        self.call::<fdo::RemoveMatch>(&(sub.rule.clone(),))
    }

    /// Send a `MethodReturn` reply to a received method call.
    pub fn reply<Body: Serialize + DynamicType>(&self, call: &Message, body: &Body) -> u32 {
        let hdr = call.header();
        let msg = Message::method_return(&hdr)
            .expect("build method return")
            .build(body)
            .expect("serialize method return");
        self.inner.borrow_mut().send(&msg)
    }

    /// Send an error reply to a received method call.
    pub fn reply_error(&self, call: &Message, name: &str, text: &str) -> u32 {
        let hdr = call.header();
        let msg = Message::error(&hdr, name)
            .expect("valid error name")
            .build(&(text,))
            .expect("serialize error");
        self.inner.borrow_mut().send(&msg)
    }

    /// Reply with unknown method when we don't identify the method
    pub fn reply_unknown_method(&self, call: &Message) -> u32 {
        self.reply_error(
            call,
            "org.freedesktop.DBus.Error.UnknownMethod",
            "no such method",
        )
    }

    /// Broadcast a typed signal from object `path`.
    pub fn emit<S: DbusSignal + Serialize>(&self, path: &str, body: &S::Args) -> u32 {
        let msg = Message::signal(path, S::INTERFACE, S::MEMBER)
            .expect("valid signal coordinates")
            .build(body)
            .expect("serialize signal");
        self.inner.borrow_mut().send(&msg)
    }

    /// Emit `org.freedesktop.DBus.Properties.PropertiesChanged` for `interface`
    /// at `path`, with the changed name→value map and any invalidated names.
    pub fn emit_properties_changed(
        &self,
        path: &str,
        interface: &str,
        changed: HashMap<String, OwnedValue>,
        invalidated: &[&str],
    ) -> u32 {
        let inv: Vec<String> = invalidated.iter().map(|s| s.to_string()).collect();
        self.emit::<fdo::PropertiesChanged>(path, &(interface.to_string(), changed, inv))
    }

    /// Flush queued writes now (otherwise they flush lazily on the next ring
    /// poll)
    pub fn flush(&self) {
        self.inner.borrow_mut().submit_write();
    }

    /// Our unique bus name once the Hello reply has arrived.
    pub fn unique_name(&self) -> Option<String> {
        self.inner.borrow().unique_name.clone()
    }
}

// Dbus Connection should be created one per bus (Session and System)
#[derive(State)]
pub struct DbusConnection<B: Bus> {
    data: Rc<RefCell<DbusInner>>,
    #[lens(skip)]
    _bus: PhantomData<B>,
}

impl<B: Bus> DbusConnection<B> {
    /// Connect to bus `B`, perform the SASL handshake, send `Hello`, and arm the
    /// first read — all synchronously, up front. After this the connection is
    /// purely event-driven through the ring.
    pub fn new(ring: RingProxy) -> Self {
        let path = parse_unix_path(&B::address()).expect("unsupported/malformed bus address");

        let mut stream = UnixStream::connect(&path).expect("failed to connect to dbus socket");

        // --- SASL EXTERNAL handshake (blocking, one-time) --------------------
        //   -> \0                       (mandatory leading nul byte)
        //   -> AUTH EXTERNAL <hex uid>\r\n
        //   <- OK <server guid>\r\n
        //   -> NEGOTIATE_UNIX_FD\r\n     (optional; enables fd passing)
        //   <- AGREE_UNIX_FD\r\n
        //   -> BEGIN\r\n
        // After BEGIN, the binary D-Bus protocol starts.
        sasl_handshake(&mut stream).expect("dbus SASL handshake failed");

        stream.set_nonblocking(true).unwrap();
        let fd = stream.into_raw_fd();

        let inner = DbusInner {
            fd,
            ring,
            read_scratch: Box::new([0u8; READ_CHUNK]),
            // SAFETY: zeroed iovec/msghdr are valid empty descriptors; fields are
            // filled in before each submit.
            read_iov: Box::new(unsafe { std::mem::zeroed() }),
            read_cmsg: Box::new([0u8; CMSG_BUF]),
            read_msghdr: Box::new(unsafe { std::mem::zeroed() }),
            read_acc: Vec::with_capacity(READ_CHUNK),
            in_fds: VecDeque::new(),
            read_token: None,
            out: VecDeque::new(),
            in_flight: None,
            write_iov: Box::new(unsafe { std::mem::zeroed() }),
            write_cmsg: Box::new([0u8; CMSG_BUF]),
            write_msghdr: Box::new(unsafe { std::mem::zeroed() }),
            write_token: None,
            hello_serial: None,
            unique_name: None,
        };
        let data = Rc::new(RefCell::new(inner));

        // Mandatory first message: Hello() to obtain our unique name. Remember
        // its serial so we can recognise the reply, then arm reads.
        {
            let mut i = data.borrow_mut();
            let hello = method_message::<fdo::Hello>(fdo::Hello::PATH, &());
            i.hello_serial = Some(i.send(&hello));
            i.submit_read();
        }

        Self {
            data,
            _bus: PhantomData,
        }
    }

    pub fn proxy(&self) -> DbusProxy<B> {
        DbusProxy {
            inner: Rc::clone(&self.data),
            _bus: PhantomData,
        }
    }
}

// // The app module: match ring completions by token, turn read bytes into
// // `DbusEvent<B>`
pub fn module<B: Bus, S>() -> impl RegisteredModule<DbusConnection<B>, S> {
    Module::<DbusConnection<B>, _, _>::new().on(|conn: &mut DbusConnection<B>, io: &IoEvent| {
        let IoEvent::Completed { token, result } = io;
        let events = {
            let mut inner = conn.data.borrow_mut();

            // Token isolation: act only on tokens submmitted for dbus. Any other
            // token (another bus, or Wayland) falls through to `else`.
            if Some(*token) == inner.read_token {
                inner.read_token = None;
                let n = *result;
                if n <= 0 {
                    eprintln!("dbus [{}] socket closed/error: {n}", B::NAME);
                    Vec::new()
                } else {
                    // Harvest any passed fds from the control buffer first.
                    if (inner.read_msghdr.msg_flags & libc::MSG_CTRUNC) != 0 {
                        eprintln!(
                            "dbus [{}]: SCM_RIGHTS truncated (MSG_CTRUNC) - fds lost",
                            B::NAME
                        );
                    }
                    // SAFETY: read_msghdr was populated by the completed RecvMsg.
                    let got = unsafe { parse_scm_rights(&inner.read_msghdr) };
                    for fd in got {
                        inner.in_fds.push_back(fd);
                    }

                    let n = n as usize;
                    let chunk = inner.read_scratch[..n].to_vec();
                    inner.read_acc.extend_from_slice(&chunk);
                    let messages = inner.drain_complete_messages();
                    inner.submit_read(); // re-arm immediately

                    let mut events: Vec<DbusEvent<B>> = Vec::new();
                    for m in messages {
                        match m.message_type() {
                            MessageType::Signal => {
                                events.push(DbusEvent::new(DbusMessage::Signal(m)));
                            }
                            MessageType::MethodReturn | MessageType::Error => {
                                let serial = m.header().reply_serial().map(u32::from).unwrap_or(0);
                                // Capture unique name from the Hello reply.
                                if inner.hello_serial == Some(serial) {
                                    inner.hello_serial = None;
                                    if let Ok(name) = m.body().deserialize::<String>() {
                                        inner.unique_name = Some(name);
                                    }
                                }
                                events.push(DbusEvent::new(DbusMessage::Reply {
                                    serial,
                                    message: m,
                                }));
                            }
                            MessageType::MethodCall => {
                                events.push(DbusEvent::new(DbusMessage::Call(m)));
                            }
                        }
                    }
                    events
                }
            } else if Some(*token) == inner.write_token {
                inner.write_token = None;
                // Dropping the frame closes our dup'd fd copies (kernel keeps its own).
                inner.in_flight = None;
                inner.submit_write(); // kick off next queued frame, if any
                Vec::new()
            } else {
                Vec::new() // it is not dbus token
            }
        };
        Many(events.into_iter())
    })
}
