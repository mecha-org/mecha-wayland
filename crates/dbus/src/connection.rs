use std::{
    cell::RefCell,
    collections::VecDeque,
    env,
    marker::PhantomData,
    os::{
        fd::{IntoRawFd, RawFd},
        unix::net::UnixStream,
    },
    rc::Rc,
};

use app::{Event, Many, Module, RegisteredModule, prelude::State};
use io_ring::{IoEvent, IoToken, RingProxy};
use io_uring::{opcode, types};
use zbus::message::{Message, Type as MessageType};
use zbus::zvariant::LE;
use zbus::zvariant::serialized::{Context, Data};

use crate::{
    dbus::{DbusMethod, DbusSignal, MatchRule, Subscription, fdo},
    util::{dbus_message_len, parse_unix_path, sasl_handshake},
};

const READ_CHUNK: usize = 8192;

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
    // accumulator for full message
    read_acc: Vec<u8>,
    read_token: Option<IoToken>,

    // outgoing bytes
    out: VecDeque<u8>,
    // in flight bytes
    in_flight: Vec<u8>,
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
        // SAFETY: `read_scratch` lives behind `Rc<RefCell<DbusInner>>` at a
        // stable heap address and is never reallocated; its pointer stays valid
        // until the matching CQE clears `read_token`.
        let sqe = opcode::Recv::new(
            types::Fd(self.fd),
            self.read_scratch.as_mut_ptr(),
            READ_CHUNK as u32,
        )
        .build();
        self.read_token = Some(self.ring.push(sqe));
    }

    /// Arm a `Send` for queued outgoing bytes, if any and none in flight.
    fn submit_write(&mut self) {
        if self.write_token.is_some() || !self.in_flight.is_empty() {
            return;
        }
        if self.out.is_empty() {
            return;
        }
        self.in_flight = self.out.drain(..).collect();

        // SAFETY: `in_flight` is owned by `self` (stable heap address behind
        // Rc<RefCell>) and left untouched until the CQE clears `write_token`.
        let sqe = opcode::Send::new(
            types::Fd(self.fd),
            self.in_flight.as_ptr(),
            self.in_flight.len() as u32,
        )
        .build();
        self.write_token = Some(self.ring.push(sqe));
    }

    /// Queue a built message and return its zbus-assigned serial.
    fn send(&mut self, msg: &Message) -> u32 {
        let serial = u32::from(msg.primary_header().serial_num());
        self.out.extend(msg.data().iter().copied());
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
            let data = Data::new(frame, Context::new_dbus(LE, 0));
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
            read_acc: Vec::with_capacity(READ_CHUNK),
            read_token: None,
            out: VecDeque::new(),
            in_flight: Vec::new(),
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
                            _ => {} // ignore MethodCall (as we are a client), in future this implementation can be extended to make a service too
                        }
                    }
                    events
                }
            } else if Some(*token) == inner.write_token {
                inner.write_token = None;
                inner.in_flight.clear();
                inner.submit_write(); // kick off next queued batch, if any
                Vec::new()
            } else {
                Vec::new() // it is not dbus token
            }
        };
        Many(events.into_iter())
    })
}
