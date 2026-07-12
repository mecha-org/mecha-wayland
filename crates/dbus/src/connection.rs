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
use log::{error, warn};
use zbus::export::serde::Serialize;
use zbus::zvariant::serialized::{Context, Data};
use zbus::zvariant::{BE, LE, OwnedValue};
use zbus::{
    message::{Message, Type as MessageType},
    zvariant::DynamicType,
};

use crate::{
    dbus::{DbusMethod, DbusSignal, MatchRule, Subscription},
    fd::{
        CMSG_BUF, MAX_SEND_FDS, MsgBuffers, build_scm_rights, dbus_unix_fd_count, dup_owned,
        parse_scm_rights,
    },
    fdo,
    util::{dbus_message_len, parse_unix_path, sasl_handshake},
};

const READ_CHUNK: usize = 8192;

/// Cap on fds sitting in the received but unclaimed FIFO
const MAX_QUEUED_FDS: usize = 64;

/// One queued outgoing message: its bytes plus any fds to pass alongside it. The
/// fds are our own dup'd copies, held open until the `SendMsg` completes.
struct OutFrame {
    bytes: Vec<u8>,
    fds: Vec<OwnedFd>,
}

pub trait Bus: 'static {
    const NAME: &'static str;

    fn address() -> Option<String>;
}

// DBUS_SESSION_BUS_ADDRESS
#[derive(Debug, Clone, Copy)]
pub struct SessionBus;
impl Bus for SessionBus {
    const NAME: &'static str = "session";
    fn address() -> Option<String> {
        env::var("DBUS_SESSION_BUS_ADDRESS").ok()
    }
}

// DBUS_SYSTEM_BUS_ADDRESS
#[derive(Debug, Clone, Copy)]
pub struct SystemBus;
impl Bus for SystemBus {
    const NAME: &'static str = "system";
    fn address() -> Option<String> {
        Some(
            env::var("DBUS_SYSTEM_BUS_ADDRESS")
                .unwrap_or_else(|_| "unix:path=/var/run/dbus/system_bus_socket".into()),
        )
    }
}

/// Why connecting to a bus failed. Returned by `DbusConnection::try_new`.
#[derive(Debug)]
pub enum ConnectError {
    /// No/unusable bus address (missing env var, or not a `unix:` transport).
    Address(String),
    /// Socket connect, handshake, or fcntl failure.
    Io(std::io::Error),
    /// Building the mandatory `Hello()` message failed (should not happen).
    Build(zbus::Error),
    /// The previous socket still has in-flight io_uring operations whose
    /// completions must drain before its buffers can be re-used
    Busy,
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectError::Address(a) => write!(f, "unusable bus address: {a}"),
            ConnectError::Io(e) => write!(f, "bus connection failed: {e}"),
            ConnectError::Build(e) => write!(f, "could not build Hello message: {e}"),
            ConnectError::Busy => write!(f, "previous socket ops still in flight; retry later"),
        }
    }
}
impl std::error::Error for ConnectError {}

#[derive(Debug)]
pub enum DbusMessage {
    // Signal broadcast from a service
    Signal(Rc<Message>),
    // Reply from a method
    Reply { serial: u32, message: Rc<Message> },
    // A method call addressed to the service
    Call(Rc<Message>),
    // The connection is dead (no auto-reconnect).
    Disconnected,
    // The connection was re-established and Hello has completed
    Reconnected,
}

/// Whether the caller flagged this call `NO_REPLY_EXPECTED`
fn no_reply_expected(call: &Message) -> bool {
    call.primary_header()
        .flags()
        .contains(zbus::message::Flags::NoReplyExpected)
}

/// Socket + SASL, shared by `try_new` and `reconnect`: resolve the bus
/// address, connect, handshake, set nonblocking. Returns the raw fd and
/// whether the bus agreed to unix-fd passing.
fn connect_socket<B: Bus>() -> Result<(RawFd, bool), ConnectError> {
    let addr = B::address()
        .ok_or_else(|| ConnectError::Address(format!("no {} bus address set", B::NAME)))?;
    let path = parse_unix_path(&addr).ok_or(ConnectError::Address(addr))?;

    let mut stream = UnixStream::connect(&path).map_err(ConnectError::Io)?;

    // --- SASL EXTERNAL handshake (blocking, one-time) ------------------------
    //   -> \0                       (mandatory leading nul byte)
    //   -> AUTH EXTERNAL <hex uid>\r\n
    //   <- OK <server guid>\r\n
    //   -> NEGOTIATE_UNIX_FD\r\n     (optional; enables fd passing)
    //   <- AGREE_UNIX_FD\r\n
    //   -> BEGIN\r\n
    // After BEGIN, the binary D-Bus protocol starts.
    let unix_fd = sasl_handshake(&mut stream).map_err(ConnectError::Io)?;

    stream.set_nonblocking(true).map_err(ConnectError::Io)?;
    Ok((stream.into_raw_fd(), unix_fd))
}

/// Build a method-call message. Fails on an invalid runtime path (e.g. a
/// malformed string handed to `call_at`) or unserializable args (e.g. a string
/// containing an interior NUL, which D-Bus forbids) — both can be user input,
/// so this must not panic.
fn method_message<M: DbusMethod>(path: &str, args: &M::Args) -> Result<Message, zbus::Error> {
    Message::method_call(path, M::MEMBER)?
        .destination(M::DESTINATION)?
        .interface(M::INTERFACE)?
        .build(args)
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
    read_buf: Box<MsgBuffers>,
    // accumulator for full message bytes and a FIFO of fds received
    read_acc: Vec<u8>,
    in_fds: VecDeque<OwnedFd>,
    read_token: Option<IoToken>,

    // outgoing frames (bytes+fds), one `SendMsg` per frame
    // so each message's fds are included with its bytes
    out: VecDeque<OutFrame>,
    // in flight bytes
    in_flight: Option<OutFrame>,
    write_buf: Box<MsgBuffers>,
    write_token: Option<IoToken>,

    unix_fd: bool,
    reconnecting: bool,

    // Dbus standard - serial for hello, and unique_name returned in handshake
    hello_serial: Option<u32>,
    unique_name: Option<String>,
}

impl Drop for DbusInner {
    fn drop(&mut self) {
        // Drop the fd for UnixStream
        unsafe { libc::close(self.fd) };
    }
}

impl DbusInner {
    /// Arm a single `Recv`. Called once at startup and re-armed after every read completion
    fn submit_read(&mut self) {
        if self.read_token.is_some() {
            return;
        }
        let scratch_ptr = self.read_scratch.as_mut_ptr() as *mut libc::c_void;
        self.read_buf.wire(scratch_ptr, READ_CHUNK, CMSG_BUF);

        // SAFETY: scratch and the boxed MsgBuffers live behind
        // Rc<RefCell<DbusInner>> at stable heap addresses, untouched until the
        // CQE clears `read_token`. MSG_CMSG_CLOEXEC keeps received fds from
        // leaking across exec.
        let sqe = opcode::RecvMsg::new(
            types::Fd(self.fd),
            &mut self.read_buf.hdr as *mut libc::msghdr,
        )
        .flags(libc::MSG_CMSG_CLOEXEC as u32)
        .build();
        self.read_token = Some(self.ring.push(sqe));
    }

    /// Arm a `SendMsg` for the next queued frame, carrying its fds (if any) as
    /// `SCM_RIGHTS`. One message per send so fds stay associated with bytes.
    fn submit_write(&mut self) {
        if self.write_token.is_some() {
            return;
        }

        // Retry the unsent remainder of a short write first; otherwise start
        // the next queued frame
        if self.in_flight.is_none() {
            let Some(frame) = self.out.pop_front() else {
                return;
            };
            self.in_flight = Some(frame);
        }

        let Some(f) = self.in_flight.as_ref() else {
            return;
        };

        let raw_fds: Vec<RawFd> = f.fds.iter().map(|fd| fd.as_raw_fd()).collect();
        let (bytes_ptr, bytes_len) = (f.bytes.as_ptr() as *mut libc::c_void, f.bytes.len());

        // SAFETY: cmsg buffer is sized for CMSG_BUF; build_scm_rights asserts fit.
        let controllen = unsafe { build_scm_rights(&mut self.write_buf.cmsg.0[..], &raw_fds) };
        self.write_buf.wire(bytes_ptr, bytes_len, controllen);

        // SAFETY: frame bytes/fds (in `in_flight`) and the boxed MsgBuffers all
        // live behind Rc<RefCell> at stable addresses until the CQE clears
        // `write_token`; the fds are our dup'd copies, closed only on completion.
        let sqe = opcode::SendMsg::new(
            types::Fd(self.fd),
            &self.write_buf.hdr as *const libc::msghdr,
        )
        .build();
        self.write_token = Some(self.ring.push(sqe));
    }

    /// Queue a built message and return its zbus-assigned serial. Any fds the
    /// message carries are dup'd into copies we own until the send completes.
    fn send(&mut self, msg: &Message) -> u32 {
        let serial = u32::from(msg.primary_header().serial_num());

        // A failed dup (fd exhaustion) refuses the whole message: sending with
        // fewer fds than the UNIX_FDS header claims would get us disconnected.
        let msg_fds = msg.data().fds();
        if msg_fds.len() > MAX_SEND_FDS {
            error!(
                "dbus: dropping message with {} fds (max {MAX_SEND_FDS})",
                msg_fds.len()
            );
            return 0;
        }
        let mut fds: Vec<OwnedFd> = Vec::with_capacity(msg_fds.len());
        for fd in msg_fds {
            match dup_owned(fd.as_raw_fd()) {
                Some(owned) => fds.push(owned),
                None => {
                    error!("dbus: dropping message: dup failed (out of fds?)");
                    return 0;
                }
            }
        }

        if !fds.is_empty() && !self.unix_fd {
            error!(
                "dbus: dropping message with {} fd(s): bus did not AGREE_UNIX_FD",
                fds.len()
            );
            return 0;
        }
        let bytes: Vec<u8> = msg.data().iter().copied().collect();
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
                        error!("dbus: message claims {want} fds but FIFO is short");
                        break;
                    }
                }
            }

            // Check byte 0 for endianness
            let endian = if frame[0] == b'B' { BE } else { LE };
            let ctx = Context::new_dbus(endian, 0);

            // NOTE (verify against your zbus pin): `Data::new_fds` attaches fds so
            // the body's `h` indices resolve.
            let data = if fds.is_empty() {
                Data::new(frame, ctx)
            } else {
                Data::new_fds(frame, ctx, fds)
            };
            match unsafe { Message::from_bytes(data) } {
                Ok(msg) => out.push(Rc::new(msg)),
                Err(e) => warn!("dbus: dropping undecodable message: {e}"),
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
        match method_message::<M>(path, args) {
            Ok(msg) => self.inner.borrow_mut().send(&msg),
            Err(e) => {
                error!(
                    "dbus: cannot build call {}.{}: {e}",
                    M::INTERFACE,
                    M::MEMBER
                );
                0
            }
        }
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
    pub fn subscribe_rule(&self, rule: MatchRule) -> Subscription {
        self.add_match_rule(&rule.to_string())
    }

    /// Escape hatch: raw match-rule string.
    pub fn add_match_rule(&self, rule: &str) -> Subscription {
        let serial = self.call::<fdo::AddMatch>(&(rule.to_string(),));
        Subscription {
            rule: rule.to_string(),
            serial,
        }
    }

    /// Remove a previously installed subscription (sends `RemoveMatch` with the
    /// exact rule string the bus recorded). Returns the call serial.
    pub fn unsubscribe(&self, sub: &Subscription) -> u32 {
        self.call::<fdo::RemoveMatch>(&(sub.rule.clone(),))
    }

    /// Send a `MethodReturn` reply to a received method call
    pub fn reply<Body: Serialize + DynamicType>(&self, call: &Message, body: &Body) -> u32 {
        if no_reply_expected(call) {
            return 0;
        }

        let built = Message::method_return(&call.header()).and_then(|b| b.build(body));
        match built {
            Ok(msg) => self.inner.borrow_mut().send(&msg),
            Err(e) => {
                error!("dbus: reply serialization failed: {e}");
                self.reply_error(
                    call,
                    "org.freedesktop.DBus.Error.Failed",
                    "reply serialization failed",
                )
            }
        }
    }

    /// Send an error reply to a received method call.
    pub fn reply_error(&self, call: &Message, name: &str, text: &str) -> u32 {
        if no_reply_expected(call) {
            return 0;
        }

        let text = text.replace('\0', " ");
        let built = Message::error(&call.header(), name).and_then(|b| b.build(&(text,)));
        match built {
            Ok(msg) => self.inner.borrow_mut().send(&msg),
            Err(e) => {
                error!("dbus: cannot build error reply '{name}': {e}");
                0
            }
        }
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
    pub fn emit<S: DbusSignal>(&self, path: &str, body: &S::Args) -> u32 {
        let built = Message::signal(path, S::INTERFACE, S::MEMBER).and_then(|b| b.build(body));
        match built {
            Ok(msg) => self.inner.borrow_mut().send(&msg),
            Err(e) => {
                error!("dbus: cannot emit {}.{}: {e}", S::INTERFACE, S::MEMBER);
                0
            }
        }
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

    /// Our unique bus name once the Hello reply has arrived.
    pub fn unique_name(&self) -> Option<String> {
        self.inner.borrow().unique_name.clone()
    }

    /// Re-connect a dead connection, call it after observing `DbusMessage::Disconnected`
    pub fn reconnect(&self) -> Result<(), ConnectError> {
        let mut inner = self.inner.borrow_mut();

        // The kernel may still hold pointers into read/write buffers for ops
        // on the old fd; reusing them for a new connection would race. A dead
        // socket's pending ops complete quickly, so callers just retry.
        if inner.read_token.is_some() || inner.write_token.is_some() {
            return Err(ConnectError::Busy);
        }

        let (fd, unix_fd) = connect_socket::<B>()?;

        // Swap the socket and reset every piece of per-connection state. The
        // old fd closes here; queued outgoing frames and unclaimed incoming
        // fds belonged to the dead connection and are dropped (OwnedFd drop
        // closes our dup'd copies).
        unsafe { libc::close(inner.fd) };
        inner.fd = fd;
        inner.unix_fd = unix_fd;
        inner.read_acc.clear();
        inner.in_fds.clear();
        inner.out.clear();
        inner.in_flight = None;
        inner.unique_name = None;
        inner.reconnecting = true;

        let hello =
            method_message::<fdo::Hello>(fdo::Hello::PATH, &()).map_err(ConnectError::Build)?;
        inner.hello_serial = Some(inner.send(&hello));
        inner.submit_read();
        Ok(())
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
    pub fn new(ring: RingProxy) -> Self {
        Self::try_new(ring).unwrap_or_else(|e| panic!("dbus [{}]: {e}", B::NAME))
    }

    /// Connect to bus `B`, perform the SASL handshake, send `Hello`, and arm the
    /// first read — all synchronously, up front. After this the connection is
    /// purely event-driven through the ring.
    pub fn try_new(ring: RingProxy) -> Result<Self, ConnectError> {
        let (fd, unix_fd) = connect_socket::<B>()?;

        let inner = DbusInner {
            fd,
            ring,
            read_scratch: Box::new([0u8; READ_CHUNK]),
            read_buf: MsgBuffers::zeroed(),
            read_acc: Vec::with_capacity(READ_CHUNK),
            in_fds: VecDeque::new(),
            read_token: None,
            out: VecDeque::new(),
            in_flight: None,
            write_buf: MsgBuffers::zeroed(),
            write_token: None,
            unix_fd,
            reconnecting: false,
            hello_serial: None,
            unique_name: None,
        };
        let data = Rc::new(RefCell::new(inner));

        // Mandatory first message: Hello() to obtain our unique name. Remember
        // its serial so we can recognise the reply, then arm reads.
        {
            let mut i = data.borrow_mut();
            let hello =
                method_message::<fdo::Hello>(fdo::Hello::PATH, &()).map_err(ConnectError::Build)?;
            i.hello_serial = Some(i.send(&hello));
            i.submit_read();
        }

        Ok(Self {
            data,
            _bus: PhantomData,
        })
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
            let inner = &mut *conn.data.borrow_mut();

            // Token isolation: act only on tokens submmitted for dbus. Any other
            // token (another bus, or Wayland) falls through to `else`.
            if Some(*token) == inner.read_token {
                inner.read_token = None;
                let n = *result;
                if n <= 0 {
                    warn!("dbus [{}] socket closed/error: {n}", B::NAME);
                    vec![DbusEvent::new(DbusMessage::Disconnected)]
                } else {
                    // Harvest any passed fds from the control buffer first.
                    if (inner.read_buf.hdr.msg_flags & libc::MSG_CTRUNC) != 0 {
                        error!(
                            "dbus [{}]: SCM_RIGHTS truncated (MSG_CTRUNC) - fds lost",
                            B::NAME
                        );
                    }
                    // SAFETY: read_msghdr was populated by the completed RecvMsg.
                    let got = unsafe { parse_scm_rights(&inner.read_buf.hdr) };
                    for fd in got {
                        if inner.in_fds.len() >= MAX_QUEUED_FDS {
                            // Drop fds than accumulate
                            warn!(
                                "dbus [{}]: unclaimed-fd queue full; closing excess fd",
                                B::NAME
                            );
                            drop(fd);
                        } else {
                            inner.in_fds.push_back(fd);
                        }
                    }

                    let n = n as usize;
                    let (scratch, acc) = (&inner.read_scratch, &mut inner.read_acc);
                    acc.extend_from_slice(&scratch[..n]);

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
                                    // A Hello reply after `reconnect` means the
                                    // new connection is fully established.
                                    if inner.reconnecting {
                                        inner.reconnecting = false;
                                        events.push(DbusEvent::new(DbusMessage::Reconnected));
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
                let n = *result;
                if n < 0 {
                    // A failed send would desync the stream if we kept going.
                    error!("dbus [{}] write error: {n}", B::NAME);
                    inner.in_flight = None;
                    inner.out.clear();
                } else {
                    let n = n as usize;
                    let done = inner
                        .in_flight
                        .as_ref()
                        .map(|f| n >= f.bytes.len())
                        .unwrap_or(true);
                    if done {
                        // Dropping the frame closes our dup'd fd copies (the
                        // kernel installed its own on send).
                        inner.in_flight = None;
                    } else if let Some(f) = inner.in_flight.as_mut() {
                        // Short write: keep the unsent tail for retry. The fds
                        // travelled with the first byte — clear them so the
                        // retry sends no control data (and closes our dups).
                        f.bytes.drain(..n);
                        f.fds.clear();
                    }
                }
                inner.submit_write(); // retry remainder or start next frame

                Vec::new()
            } else {
                Vec::new() // it is not dbus token
            }
        };
        Many(events.into_iter())
    })
}
