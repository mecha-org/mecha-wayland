//! A scripted bus daemon on the other end of a socketpair, for tests
//! that need no real one. Single-threaded, like `wayland::fake`.
//!
//! - The SASL answers, the `Hello` reply (the app's unique name is
//!   [`UNIQUE`]) and the `NameAcquired` after it are written before
//!   install, so install's blocking handshake finds them waiting.
//! - With `auto_daemon` (the default) the fake answers calls to
//!   `org.freedesktop.DBus` as a daemon would: `AddMatch`, `RemoveMatch`,
//!   `GetNameOwner` (from `owners`), `NameHasOwner`, `ListNames`,
//!   `RequestName`, `ReleaseName`. They still appear in `received`.
//! - The fake reads and writes its end with its own `Ring`, so fds pass
//!   in both directions. Its end is nonblocking; a read with nothing to
//!   read completes with `-EAGAIN`.
//! - What the app sends at once must fit in the socket buffer (about
//!   200 KiB): the fake only reads between the app's turns.

use std::collections::{HashMap, VecDeque};
use std::io::{ErrorKind, Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use app::prelude::*;
use ring::prelude::*;

use crate::bus::{BusId, DBus, DBusModule};
use crate::message::{Message, MessageKind};
use crate::names::BusName;
use crate::types::Body;

/// The app's unique name on the fake bus.
pub const UNIQUE: &str = ":1.1";
const DAEMON: &str = "org.freedesktop.DBus";
const DAEMON_PATH: &str = "/org/freedesktop/DBus";

pub struct FakeBus {
    pub app: App,
    peer: UnixStream,
    ring: Ring,
    reading: Option<Token>,
    inbox: Vec<u8>,
    in_fds: VecDeque<OwnedFd>,
    authed: bool,
    auth: String,
    outbox: Vec<u8>,
    wrote: bool,
    received: Vec<Message>,
    daemon_calls: Vec<Message>,
    next_serial: u32,
    /// Well-known name to unique owner, for `GetNameOwner` and for the
    /// sender of replies to calls addressed to the well-known name.
    pub owners: HashMap<String, String>,
    /// Answer `AddMatch` with `org.freedesktop.DBus.Error.MatchRuleInvalid`.
    pub reject_add_match: bool,
    /// Answer calls to `org.freedesktop.DBus` automatically.
    pub auto_daemon: bool,
}

impl FakeBus {
    pub fn new() -> FakeBus {
        FakeBus::build(true)
    }

    /// A bus that refuses fd passing.
    pub fn without_fds() -> FakeBus {
        FakeBus::build(false)
    }

    fn build(fds: bool) -> FakeBus {
        let (ours, theirs) = UnixStream::pair().expect("socketpair");
        theirs.set_nonblocking(true).expect("nonblocking peer");
        let mut app = App::new();
        app.add_module(RingModule::default());
        let mut f = FakeBus {
            app,
            peer: theirs,
            ring: Ring::new(8),
            reading: None,
            inbox: Vec::new(),
            in_fds: VecDeque::new(),
            authed: false,
            auth: String::new(),
            outbox: Vec::new(),
            wrote: false,
            received: Vec::new(),
            daemon_calls: Vec::new(),
            next_serial: 1,
            owners: HashMap::new(),
            reject_add_match: false,
            auto_daemon: true,
        };
        f.outbox
            .extend_from_slice(b"OK 0123456789abcdef0123456789abcdef\r\n");
        let fd_answer: &[u8] = if fds {
            b"AGREE_UNIX_FD\r\n"
        } else {
            b"ERROR fds are off\r\n"
        };
        f.outbox.extend_from_slice(fd_answer);
        // The app's first message is Hello, serial 1.
        f.send(
            Message::method_return(1)
                .with_destination(UNIQUE)
                .with_sender(DAEMON)
                .with_body(&(UNIQUE,)),
        );
        f.send(name_signal("NameAcquired", UNIQUE));
        f.flush_outbox();
        f.wrote = false;
        f.app
            .add_module(DBusModule::new().over(BusId::Session, ours));
        f.turn();
        f
    }

    /// Assign the next serial and encode, for a test writing bytes by hand.
    pub fn encode(&mut self, msg: &Message) -> Vec<u8> {
        let serial = self.next_serial;
        self.next_serial += 1;
        let (bytes, fds) = msg.encode(serial);
        assert!(
            fds.is_empty(),
            "fake: use send_with_fds for a message with fds"
        );
        bytes
    }

    /// Queue a message to the app; it goes out on the next [`turn`](Self::turn).
    pub fn send(&mut self, msg: Message) -> u32 {
        let serial = self.next_serial;
        let bytes = self.encode(&msg);
        self.outbox.extend_from_slice(&bytes);
        self.wrote = true;
        serial
    }

    /// Send a message carrying fds now, through the fake's own ring.
    pub fn send_with_fds(&mut self, msg: Message) -> u32 {
        self.flush_outbox();
        let serial = self.next_serial;
        self.next_serial += 1;
        let (bytes, fds) = msg.encode(serial);
        let fds = fds
            .iter()
            .map(|f| f.try_clone_owned().expect("fake: dup an fd"))
            .collect();
        let t = self.ring.sendmsg(self.peer.as_fd(), bytes, fds);
        // The read parked by `read_peer` may complete first.
        let mut events = self.ring.wait();
        while !events.iter().any(|e| e.token == t) {
            events = self.ring.wait();
        }
        assert!(
            events.iter().any(|e| e.token == t && e.result > 0),
            "fake: sendmsg failed: {events:?}"
        );
        self.ring.finish(t);
        self.wrote = true;
        serial
    }

    /// Write raw bytes now: half a message, or garbage.
    pub fn write_raw(&mut self, bytes: &[u8]) {
        self.flush_outbox();
        self.write_pumping(bytes);
        self.wrote = true;
    }

    /// A method return for `call`, from whoever owns its destination.
    pub fn reply<B: Body>(&mut self, call: &Message, body: &B) -> u32 {
        let m = Message::method_return(call.serial())
            .with_destination(UNIQUE)
            .with_sender(self.sender_for(call))
            .with_body(body);
        self.send(m)
    }

    /// An error reply for `call`.
    pub fn error(&mut self, call: &Message, name: &'static str, text: &str) -> u32 {
        let m = Message::error(call.serial(), name, text)
            .with_destination(UNIQUE)
            .with_sender(self.sender_for(call));
        self.send(m)
    }

    fn sender_for(&self, call: &Message) -> BusName {
        let dest = call
            .destination()
            .map(|d| d.as_str().to_owned())
            .unwrap_or_else(|| DAEMON.to_owned());
        let sender = self.owners.get(&dest).cloned().unwrap_or(dest);
        BusName::new(&sender).expect("fake: a valid sender")
    }

    /// Run both sides until quiet. Each round writes what is queued,
    /// ticks the app (which raises the replies of calls that failed
    /// before being sent) and raises its `BeforeWait` (which queues
    /// `NoReply`s and `PropertiesChanged`). If either side then has
    /// something in flight, one `Ring::turn` and another round; if the
    /// app has work left, another round; otherwise read what the app
    /// sent, answer daemon calls, and stop once there were none. A
    /// `Ring::turn` is only made when a completion is certain, so this
    /// never blocks forever.
    pub fn turn(&mut self) {
        let mut rounds = 0;
        loop {
            rounds += 1;
            assert!(rounds < 64, "fake: the fake and the app keep talking");
            self.flush_outbox();
            self.app.tick();
            self.app.signal(BeforeWait);
            self.app.flush();
            if self.wrote || self.app.resource::<DBus>().has_pending_output() {
                self.wrote = false;
                Ring::turn(&mut self.app);
                continue;
            }
            if self.app.has_pending_work() {
                continue;
            }
            self.read_peer();
            if !(self.auto_daemon && self.answer_daemon()) {
                break;
            }
        }
    }

    /// Every message the app sent since the last call, in order.
    pub fn received(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.received)
    }

    /// The first method call named `member`, removed.
    pub fn expect_call(&mut self, member: &str) -> Message {
        let at = self
            .received
            .iter()
            .position(|m| {
                m.kind() == MessageKind::MethodCall
                    && m.member().is_some_and(|x| x.as_str() == member)
            })
            .unwrap_or_else(|| panic!("fake: no call {member} among {:?}", self.summary()));
        self.received.remove(at)
    }

    /// The first message of `kind`, removed.
    pub fn expect(&mut self, kind: MessageKind) -> Message {
        let at = self
            .received
            .iter()
            .position(|m| m.kind() == kind)
            .unwrap_or_else(|| panic!("fake: no {kind:?} among {:?}", self.summary()));
        self.received.remove(at)
    }

    /// The SASL text the app sent.
    pub fn auth(&self) -> &str {
        &self.auth
    }

    /// Whether the app closed its end: a nonblocking read finds EOF.
    pub fn peer_closed(&mut self) -> bool {
        let mut buf = [0u8; 1];
        matches!((&self.peer).read(&mut buf), Ok(0))
    }

    /// Close the fake's end: the app's next read returns zero.
    pub fn hang_up(self) -> App {
        let FakeBus { app, peer, .. } = self;
        drop(peer);
        app
    }

    fn summary(&self) -> Vec<String> {
        self.received
            .iter()
            .map(|m| format!("{:?} {:?}", m.kind(), m.member().map(|x| x.as_str())))
            .collect()
    }

    fn flush_outbox(&mut self) {
        if self.outbox.is_empty() {
            return;
        }
        let buf = std::mem::take(&mut self.outbox);
        self.write_pumping(&buf);
    }

    /// Write all of `bytes`, turning the app whenever the socket is full.
    fn write_pumping(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            match (&self.peer).write(bytes) {
                Ok(n) => bytes = &bytes[n..],
                Err(e) if e.kind() == ErrorKind::WouldBlock => Ring::turn(&mut self.app),
                Err(e) => panic!("fake: write to the app: {e}"),
            }
        }
    }

    fn read_peer(&mut self) {
        // io_uring parks a read with nothing to read instead of
        // completing it with -EAGAIN, so the read stays armed across
        // calls and a zero timeout makes the wait return regardless:
        // a read with data ready completes at submission, before it.
        loop {
            let t = *self.reading.get_or_insert_with(|| {
                self.ring
                    .recvmsg(self.peer.as_fd(), Vec::with_capacity(64 * 1024), 253)
            });
            let timer = self.ring.timeout(Duration::ZERO);
            while self.ring.finish(timer).is_none() {
                self.ring.wait();
            }
            let Some(done) = self.ring.finish(t) else {
                break; // nothing to read yet
            };
            self.reading = None;
            if done.buf.is_empty() {
                break; // the app hung up
            }
            self.inbox.extend_from_slice(&done.buf);
            self.in_fds.extend(done.fds);
        }
        if !self.authed {
            let Some(at) = self.inbox.windows(7).position(|w| w == b"BEGIN\r\n") else {
                return;
            };
            let end = at + 7;
            self.auth = String::from_utf8_lossy(&self.inbox[..end]).into_owned();
            self.inbox.drain(..end);
            self.authed = true;
        }
        let mut at = 0;
        while let Ok(Some(n)) = Message::frame_len(&self.inbox[at..]) {
            let msg = Message::parse(&self.inbox[at..at + n], &mut self.in_fds)
                .expect("fake: the app sent a malformed message");
            if msg.kind() == MessageKind::MethodCall
                && msg.destination().is_some_and(|d| d.as_str() == DAEMON)
            {
                self.daemon_calls.push(msg.clone());
            }
            self.received.push(msg);
            at += n;
        }
        self.inbox.drain(..at);
    }

    /// Answer queued daemon calls; whether there were any.
    fn answer_daemon(&mut self) -> bool {
        let calls = std::mem::take(&mut self.daemon_calls);
        let any = !calls.is_empty();
        for call in calls {
            let member = call
                .member()
                .map(|m| m.as_str().to_owned())
                .unwrap_or_default();
            match member.as_str() {
                "Hello" => {} // answered before install
                "AddMatch" if self.reject_add_match => {
                    self.error(
                        &call,
                        "org.freedesktop.DBus.Error.MatchRuleInvalid",
                        "rejected by the fake",
                    );
                }
                "AddMatch" | "RemoveMatch" => {
                    self.reply(&call, &());
                }
                "GetNameOwner" => {
                    let (name,): (String,) = call.body().expect("fake: GetNameOwner(s)");
                    match self.owners.get(&name).cloned() {
                        Some(owner) => {
                            self.reply(&call, &(owner,));
                        }
                        None => {
                            self.error(
                                &call,
                                "org.freedesktop.DBus.Error.NameHasNoOwner",
                                "no owner",
                            );
                        }
                    }
                }
                "NameHasOwner" => {
                    let (name,): (String,) = call.body().expect("fake: NameHasOwner(s)");
                    let has = self.owners.contains_key(&name);
                    self.reply(&call, &(has,));
                }
                "ListNames" => {
                    let mut names: Vec<String> = self.owners.keys().cloned().collect();
                    names.sort();
                    names.push(DAEMON.to_owned());
                    names.push(UNIQUE.to_owned());
                    self.reply(&call, &(names,));
                }
                "RequestName" => {
                    let (name, _flags): (String, u32) = call.body().expect("fake: RequestName(su)");
                    self.owners.insert(name.clone(), UNIQUE.to_owned());
                    self.reply(&call, &(1u32,));
                    self.send(name_signal("NameAcquired", &name));
                }
                "ReleaseName" => {
                    let (name,): (String,) = call.body().expect("fake: ReleaseName(s)");
                    self.owners.remove(&name);
                    self.reply(&call, &(1u32,));
                    self.send(name_signal("NameLost", &name));
                }
                _ => {
                    self.error(
                        &call,
                        "org.freedesktop.DBus.Error.UnknownMethod",
                        "the fake does not know it",
                    );
                }
            }
        }
        any
    }
}

fn name_signal(member: &'static str, name: &str) -> Message {
    Message::signal(DAEMON_PATH, DAEMON, member)
        .with_destination(UNIQUE)
        .with_sender(DAEMON)
        .with_body(&(name,))
}
