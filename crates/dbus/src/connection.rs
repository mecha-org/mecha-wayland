//! One bus connection: the socket, what is waiting to go out, what came
//! in and is not yet a whole message, and every call still owed a reply.
//! Methods return the signals to raise as [`Out`]s; they never touch the
//! `App`.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap, VecDeque};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::time::Instant;

use ring::{Completed, IoEvent, Ring, Token};

use crate::bus::BusId;
use crate::client::{CallError, CallId, MethodCall, PROPERTIES};
use crate::matching::{Entry, MatchId, MatchRule, Matches, Owner};
use crate::message::{Flags, Message, MessageKind};
use crate::names::{BusName, ErrorName, InterfaceName, MemberName, ObjectPath};
use crate::server::{
    Access, Export, ExportError, ExportId, INTROSPECTABLE, IncomingCall, InterfaceSpec, PEER,
    PropertyError, STANDARD, Server, errors,
};
use crate::signals::{
    BusDisconnected, MatchFailed, MethodCallReceived, MethodReply, NameAcquired, NameLost, Out,
    PropertySetRequested, SignalReceived, out,
};
use crate::standard;
use crate::types::Body;
use crate::value::Value;

pub(crate) const DAEMON: &str = "org.freedesktop.DBus";
pub(crate) const DAEMON_PATH: &str = "/org/freedesktop/DBus";
/// The one receive's buffer.
pub(crate) const READ_CAPACITY: usize = 64 * 1024;
/// `SCM_MAX_FD`: room for every fd one `sendmsg` can carry.
pub(crate) const MAX_FDS: usize = 253;

/// A call this connection made and has not had answered.
pub(crate) enum Pending {
    /// The connection's own bookkeeping; answered internally.
    Internal(Internal),
    /// A caller's call; its answer is a `MethodReply`. Its deadline lives in the `deadlines` heap.
    User,
}

pub(crate) enum Internal {
    Hello,
    /// `AddMatch` for the rule with this string.
    AddMatch(String),
    /// `AddMatch` for an owner-following rule; failures are ignored.
    OwnerMatch,
    GetNameOwner(BusName),
}

pub(crate) struct Connection {
    pub(crate) id: BusId,
    stream: UnixStream,
    pub(crate) fds_ok: bool,
    pub(crate) unique_name: Option<BusName>,
    pub(crate) dead: bool,
    pub(crate) reading: Option<Token>,
    pub(crate) sending: Option<Token>,
    inbox: Vec<u8>,
    in_fds: VecDeque<OwnedFd>,
    /// Encoded messages and their fds, oldest first.
    outbox: VecDeque<(Vec<u8>, Vec<OwnedFd>)>,
    next_serial: u32,
    pending: HashMap<u32, Pending>,
    /// Every `User` call's deadline, earliest first. Entries for calls
    /// already answered are skipped when they surface.
    deadlines: BinaryHeap<Reverse<(Instant, u32)>>,
    /// Ring timeouts in flight, and the deadline each was armed for.
    timers: Vec<(Token, Instant)>,
    /// Calls that failed before being sent; answered in the next tick.
    deferred: Vec<(u32, CallError)>,
    pub(crate) matches: Matches,
    pub(crate) server: Server,
}

/// Members a call without an interface may mean on any path.
const STANDARD_MEMBERS: [&str; 6] = ["Introspect", "Ping", "GetMachineId", "Get", "GetAll", "Set"];
/// Where an incoming call goes.
enum Route {
    Standard,
    Export(u64),
    Error(&'static str, String),
}

impl Connection {
    pub(crate) fn new(id: BusId, stream: UnixStream, fds_ok: bool) -> Self {
        Connection {
            id,
            stream,
            fds_ok,
            unique_name: None,
            dead: false,
            reading: None,
            sending: None,
            inbox: Vec::new(),
            in_fds: VecDeque::new(),
            outbox: VecDeque::new(),
            next_serial: 1,
            pending: HashMap::new(),
            deadlines: BinaryHeap::new(),
            timers: Vec::new(),
            deferred: Vec::new(),
            matches: Matches::default(),
            server: Server::default(),
        }
    }

    pub(crate) fn arm_read(&mut self, ring: &mut Ring, buf: Vec<u8>) {
        self.reading = Some(ring.recvmsg(self.stream.as_fd(), buf, MAX_FDS));
    }

    /// The next serial: never zero, wrapping past `u32::MAX` to 1.
    pub(crate) fn serial(&mut self) -> u32 {
        let s = self.next_serial;
        self.next_serial = if s == u32::MAX { 1 } else { s + 1 };
        s
    }

    /// Queue `msg` under a fresh serial and return it. A dead connection
    /// spends the serial and sends nothing.
    pub(crate) fn queue(&mut self, msg: &Message) -> u32 {
        let serial = self.serial();
        if self.dead {
            return serial;
        }
        let (bytes, fds) = msg.encode(serial);
        let fds = fds
            .iter()
            .map(|f| f.try_clone_owned().expect("dbus: dup an fd to send"))
            .collect();
        self.outbox.push_back((bytes, fds));
        serial
    }

    /// Whether a send is queued or in flight.
    #[cfg(feature = "fake")]
    pub(crate) fn has_pending_output(&self) -> bool {
        !self.dead && (!self.outbox.is_empty() || self.sending.is_some())
    }

    /// Whether work is left: for the next `BeforeWait`, a send to start,
    /// a dropped call to answer or a property change to signal; for the
    /// next tick, a deferred reply to raise.
    pub(crate) fn has_work(&self) -> bool {
        (!self.dead && !self.outbox.is_empty() && self.sending.is_none())
            || !self.server.unanswered.borrow().is_empty()
            || self.server.exports.values().any(|e| !e.changed.is_empty())
            || !self.deferred.is_empty()
    }

    /// Send what is queued in one `sendmsg`, if none is in flight, cut
    /// at a message boundary so no send carries more than [`MAX_FDS`].
    pub(crate) fn flush(&mut self, ring: &mut Ring) {
        if self.dead || self.sending.is_some() || self.outbox.is_empty() {
            return;
        }
        let mut buf = Vec::new();
        let mut fds = Vec::new();
        while let Some((_, f)) = self.outbox.front() {
            if !buf.is_empty() && fds.len() + f.len() > MAX_FDS {
                break;
            }
            let (b, f) = self.outbox.pop_front().unwrap();
            buf.extend_from_slice(&b);
            fds.extend(f);
        }
        self.sending = Some(ring.sendmsg(self.stream.as_fd(), buf, fds));
    }

    pub(crate) fn owns(&self, t: Token) -> bool {
        self.reading == Some(t)
            || self.sending == Some(t)
            || self.timers.iter().any(|&(tt, _)| tt == t)
    }

    /// The replies of calls that failed before being sent.
    pub(crate) fn take_deferred(&mut self) -> Vec<Out> {
        let bus = self.id;
        self.deferred
            .drain(..)
            .map(|(serial, e)| {
                out(MethodReply {
                    call: CallId { bus, serial },
                    result: Err(e),
                })
            })
            .collect()
    }

    pub(crate) fn before_wait(&mut self, ring: &mut Ring) {
        let dropped: Vec<_> = self.server.unanswered.borrow_mut().drain(..).collect();
        for (sender, serial) in dropped {
            let m = Message::error(
                serial,
                errors::NO_REPLY,
                "The call was dropped without a reply",
            )
            .with_destination_opt(sender);
            self.queue(&m);
        }
        let mut changes = Vec::new();
        for e in self.server.exports.values_mut() {
            if e.changed.is_empty() {
                continue;
            }
            let names = std::mem::take(&mut e.changed);
            let changed: BTreeMap<String, Value> = names
                .into_iter()
                .map(|n| {
                    let v = e.values[&n].clone();
                    (n, v)
                })
                .collect();
            changes.push(
                Message::signal(e.path.clone(), PROPERTIES, "PropertiesChanged").with_body(&(
                    e.spec.name.as_str(),
                    changed,
                    Vec::<String>::new(),
                )),
            );
        }
        for m in changes {
            self.queue(&m);
        }
        self.flush(ring);
        self.arm_timer(ring);
    }

    pub(crate) fn export(
        &mut self,
        path: ObjectPath,
        spec: InterfaceSpec,
    ) -> Result<ExportId, ExportError> {
        if STANDARD.contains(&spec.name.as_str()) {
            return Err(ExportError::Reserved);
        }
        if self.server.export_at(&path, spec.name.as_str()).is_some() {
            return Err(ExportError::AlreadyExported);
        }
        let n = self.server.next;
        self.server.next += 1;
        self.server.exports.insert(n, Export::new(path, spec));
        Ok(ExportId { bus: self.id, n })
    }

    pub(crate) fn unexport(&mut self, id: ExportId) {
        if id.bus == self.id {
            self.server.exports.remove(&id.n);
        }
    }

    /// Answer `call` with a return, unless it was answered already or
    /// asked for no reply.
    pub(crate) fn answer<B: Body>(&mut self, call: &IncomingCall, body: &B) {
        if !call.take_answer() || call.no_reply() {
            return;
        }
        let m = Message::method_return(call.message().serial())
            .with_destination_opt(call.sender().cloned())
            .with_body(body);
        self.queue(&m);
    }

    pub(crate) fn answer_error(
        &mut self,
        call: &IncomingCall,
        name: impl Into<ErrorName>,
        text: &str,
    ) {
        if !call.take_answer() || call.no_reply() {
            return;
        }
        let m = Message::error(call.message().serial(), name, text)
            .with_destination_opt(call.sender().cloned());
        self.queue(&m);
    }

    fn incoming(&mut self, msg: Message) -> Vec<Out> {
        let call = IncomingCall::new(self.id, msg, &self.server.unanswered);
        match self.route(&call) {
            Route::Export(n) => vec![out(MethodCallReceived {
                export: ExportId { bus: self.id, n },
                call,
            })],
            Route::Standard => self.standard(call),
            Route::Error(name, text) => {
                self.answer_error(&call, name, &text);
                Vec::new()
            }
        }
    }

    /// Introspectable, Peer and Properties, on any path.
    fn standard(&mut self, call: IncomingCall) -> Vec<Out> {
        let msg = call.message();
        let member = msg
            .member()
            .expect("dbus: a call has a member")
            .as_str()
            .to_owned();
        let iface = match msg.interface() {
            Some(i) => i.as_str().to_owned(),
            None => match member.as_str() {
                "Introspect" => INTROSPECTABLE,
                "Ping" | "GetMachineId" => PEER,
                _ => PROPERTIES,
            }
            .to_owned(),
        };
        let sig = msg.signature().as_str().to_owned();
        let path = msg.path().expect("dbus: a call has a path").clone();
        let want = match (iface.as_str(), member.as_str()) {
            (INTROSPECTABLE, "Introspect") | (PEER, "Ping") | (PEER, "GetMachineId") => "",
            (PROPERTIES, "Get") => "ss",
            (PROPERTIES, "GetAll") => "s",
            (PROPERTIES, "Set") => "ssv",
            _ => {
                self.answer_error(
                    &call,
                    errors::UNKNOWN_METHOD,
                    &format!("{iface} has no method {member}"),
                );
                return Vec::new();
            }
        };
        if sig != want {
            self.answer_error(
                &call,
                errors::INVALID_ARGS,
                &format!("{member} takes ({want}), not ({sig})"),
            );
            return Vec::new();
        }
        match member.as_str() {
            "Introspect" => {
                let xml = standard::introspect(&path, self.server.exports.values());
                self.answer(&call, &(xml,));
            }
            "Ping" => self.answer(&call, &()),
            "GetMachineId" => match standard::machine_id() {
                Ok(id) => self.answer(&call, &(id,)),
                Err(e) => self.answer_error(&call, errors::FAILED, &e.to_string()),
            },
            "Get" => {
                let Ok((i, name)) = call.args::<(String, String)>() else {
                    self.answer_error(&call, errors::INVALID_ARGS, "malformed arguments");
                    return Vec::new();
                };
                match self.property(&path, &i, &name) {
                    Ok((_, _, value)) => self.answer(&call, &(value,)),
                    Err((err, text)) => self.answer_error(&call, err, &text),
                }
            }
            "GetAll" => {
                let Ok((i,)) = call.args::<(String,)>() else {
                    self.answer_error(&call, errors::INVALID_ARGS, "malformed arguments");
                    return Vec::new();
                };
                match self.server.export_at(&path, &i) {
                    Some((_, e)) => {
                        let all: BTreeMap<String, Value> = e.values.clone();
                        self.answer(&call, &(all,));
                    }
                    None => self.answer_error(
                        &call,
                        errors::UNKNOWN_INTERFACE,
                        &format!("No interface {i} at {path}"),
                    ),
                }
            }
            "Set" => return self.property_set(call, &path),
            _ => unreachable!("dbus: every standard member is listed above"),
        }
        Vec::new()
    }

    /// A property's export, access and current value.
    fn property(
        &self,
        path: &ObjectPath,
        iface: &str,
        name: &str,
    ) -> Result<(u64, Access, Value), (&'static str, String)> {
        let (n, e) = self.server.export_at(path, iface).ok_or((
            errors::UNKNOWN_INTERFACE,
            format!("No interface {iface} at {path}"),
        ))?;
        let spec = e
            .spec
            .properties
            .iter()
            .find(|p| p.name.as_str() == name)
            .ok_or((
                errors::UNKNOWN_PROPERTY,
                format!("No property {name} on {iface}"),
            ))?;
        Ok((n, spec.access, e.values[name].clone()))
    }

    fn property_set(&mut self, call: IncomingCall, path: &ObjectPath) -> Vec<Out> {
        let Ok((i, name, value)) = call.args::<(String, String, Value)>() else {
            self.answer_error(&call, errors::INVALID_ARGS, "malformed arguments");
            return Vec::new();
        };
        match self.property(path, &i, &name) {
            Err((err, text)) => {
                self.answer_error(&call, err, &text);
                Vec::new()
            }
            Ok((_, Access::Read, _)) => {
                self.answer_error(
                    &call,
                    errors::PROPERTY_READ_ONLY,
                    &format!("{name} is read-only"),
                );
                Vec::new()
            }
            Ok((_, Access::ReadWrite, current)) if value.signature() != current.signature() => {
                let text = format!(
                    "{name} is a {}, not a {}",
                    current.signature(),
                    value.signature()
                );
                self.answer_error(&call, errors::INVALID_ARGS, &text);
                Vec::new()
            }
            Ok((n, Access::ReadWrite, _)) => vec![out(PropertySetRequested {
                export: ExportId { bus: self.id, n },
                name,
                value,
                call,
            })],
        }
    }

    pub(crate) fn set_property_value(
        &mut self,
        export: ExportId,
        name: &str,
        value: Value,
    ) -> Result<(), PropertyError> {
        let bus = self.id;
        let e = self
            .server
            .exports
            .get_mut(&export.n)
            .filter(|_| export.bus == bus)
            .ok_or(PropertyError::UnknownExport)?;
        let spec = e
            .spec
            .properties
            .iter()
            .find(|p| p.name.as_str() == name)
            .ok_or(PropertyError::UnknownProperty)?;
        let found = value.signature();
        if found != spec.sig {
            return Err(PropertyError::Signature {
                expected: spec.sig.clone(),
                found,
            });
        }
        if e.values.get(name) == Some(&value) {
            return Ok(());
        }
        e.values.insert(name.to_owned(), value);
        e.changed.insert(name.to_owned());
        Ok(())
    }

    /// The export a call is for, or the error that answers it.
    fn route(&self, call: &IncomingCall) -> Route {
        let msg = call.message();
        let path = msg.path().expect("dbus: parse requires a call's path");
        let member = msg.member().expect("dbus: parse requires a call's member");
        if msg
            .interface()
            .is_some_and(|i| STANDARD.contains(&i.as_str()))
        {
            return Route::Standard;
        }
        let here: Vec<(u64, &Export)> = self.server.at_path(path).collect();
        let candidates: Vec<(u64, &Export)> = match msg.interface() {
            Some(iface) => {
                if here.is_empty() {
                    return Route::Error(errors::UNKNOWN_OBJECT, format!("No object at {path}"));
                }
                match here.iter().find(|(_, e)| e.spec.name == *iface) {
                    Some(&c) => vec![c],
                    None => {
                        return Route::Error(
                            errors::UNKNOWN_INTERFACE,
                            format!("No interface {iface} at {path}"),
                        );
                    }
                }
            }
            None => here
                .iter()
                .copied()
                .filter(|(_, e)| e.spec.methods.iter().any(|m| m.name == *member))
                .collect(),
        };
        match candidates.as_slice() {
            [(n, e)] => match e.spec.methods.iter().find(|m| m.name == *member) {
                None => Route::Error(
                    errors::UNKNOWN_METHOD,
                    format!("No method {member} on {}", e.spec.name),
                ),
                Some(m) if m.input != *msg.signature() => Route::Error(
                    errors::INVALID_ARGS,
                    format!("{member} takes ({}), not ({})", m.input, msg.signature()),
                ),
                Some(_) => Route::Export(*n),
            },
            [] if msg.interface().is_none() && STANDARD_MEMBERS.contains(&member.as_str()) => {
                Route::Standard
            }
            [] if here.is_empty() => {
                Route::Error(errors::UNKNOWN_OBJECT, format!("No object at {path}"))
            }
            _ => Route::Error(
                errors::UNKNOWN_METHOD,
                format!("No single method {member} at {path}"),
            ),
        }
    }

    pub(crate) fn call(&mut self, call: MethodCall) -> CallId {
        let fds_refused = !call.msg.fds().is_empty() && !self.fds_ok;
        let serial = if fds_refused {
            self.serial()
        } else {
            self.queue(&call.msg)
        };
        if fds_refused {
            self.deferred.push((serial, CallError::FdsUnsupported));
        } else if self.dead {
            self.deferred.push((serial, CallError::Disconnected));
        } else {
            self.pending.insert(serial, Pending::User);
            // A timeout too large to represent never expires.
            if let Some(deadline) = Instant::now().checked_add(call.timeout) {
                self.deadlines.push(Reverse((deadline, serial)));
            }
        }
        CallId {
            bus: self.id,
            serial,
        }
    }

    pub(crate) fn call_no_reply(&mut self, call: MethodCall) {
        if !call.msg.fds().is_empty() && !self.fds_ok {
            return;
        }
        let flags = call.msg.flags() | Flags::NO_REPLY_EXPECTED;
        let msg = call.msg.with_flags(flags);
        self.queue(&msg);
    }

    /// Forget a call: no `MethodReply` will be raised for it.
    pub(crate) fn cancel(&mut self, serial: u32) {
        if matches!(self.pending.get(&serial), Some(Pending::User)) {
            self.pending.remove(&serial);
        }
        self.deferred.retain(|(s, _)| *s != serial);
    }

    /// Arm a ring timeout for the earliest pending deadline unless one
    /// armed already fires by then.
    fn arm_timer(&mut self, ring: &mut Ring) {
        while let Some(Reverse((_, s))) = self.deadlines.peek() {
            if matches!(self.pending.get(s), Some(Pending::User)) {
                break;
            }
            self.deadlines.pop();
        }
        let Some(&Reverse((at, _))) = self.deadlines.peek() else {
            return;
        };
        if self.timers.iter().any(|&(_, t)| t <= at) {
            return;
        }
        let token = ring.timeout(at.saturating_duration_since(Instant::now()));
        self.timers.push((token, at));
    }

    /// Fail every call whose deadline has passed.
    fn expire(&mut self) -> Vec<Out> {
        let now = Instant::now();
        let mut outs = Vec::new();
        while let Some(&Reverse((at, serial))) = self.deadlines.peek() {
            if at > now {
                break;
            }
            self.deadlines.pop();
            if let Some(Pending::User) = self.pending.get(&serial) {
                self.pending.remove(&serial);
                outs.push(out(MethodReply {
                    call: CallId {
                        bus: self.id,
                        serial,
                    },
                    result: Err(CallError::Timeout),
                }));
            }
        }
        outs
    }

    pub(crate) fn on_io(&mut self, e: &IoEvent, done: Completed, ring: &mut Ring) -> Vec<Out> {
        if let Some(i) = self.timers.iter().position(|&(t, _)| t == e.token) {
            self.timers.swap_remove(i);
            return self.expire();
        }
        if self.reading == Some(e.token) {
            self.reading = None;
            if self.dead {
                return Vec::new();
            }
            if e.result <= 0 {
                return self.die();
            }
            self.inbox.extend_from_slice(&done.buf);
            self.in_fds.extend(done.fds);
            let outs = self.read_messages();
            // A connection that died while framing stops reading.
            if !self.dead {
                self.arm_read(ring, done.buf);
            }
            outs
        } else if self.sending == Some(e.token) {
            self.sending = None;
            if self.dead {
                return Vec::new();
            }
            if e.result < 0 {
                return self.die();
            }
            let sent = e.result as usize;
            if sent < done.buf.len() {
                // The fds went with the first byte; the tail carries none.
                self.outbox
                    .push_front((done.buf[sent..].to_vec(), Vec::new()));
            }
            Vec::new()
        } else {
            Vec::new()
        }
    }

    /// Handle every whole message in the inbox. Anything unframeable is a
    /// protocol violation: the specification says to drop the connection.
    fn read_messages(&mut self) -> Vec<Out> {
        let mut outs = Vec::new();
        let mut at = 0;
        loop {
            match Message::frame_len(&self.inbox[at..]) {
                Ok(None) => break,
                // The specification says to ignore unknown message types.
                Ok(Some(n)) if !matches!(self.inbox[at + 1], 1..=4) => at += n,
                Ok(Some(n)) => match Message::parse(&self.inbox[at..at + n], &mut self.in_fds) {
                    Ok(msg) => {
                        at += n;
                        outs.extend(self.handle(msg));
                        if self.dead {
                            return outs;
                        }
                    }
                    Err(_) => {
                        outs.extend(self.die());
                        return outs;
                    }
                },
                Err(_) => {
                    outs.extend(self.die());
                    return outs;
                }
            }
        }
        self.inbox.drain(..at);
        outs
    }

    fn handle(&mut self, msg: Message) -> Vec<Out> {
        match msg.kind() {
            MessageKind::MethodReturn | MessageKind::Error => {
                let Some(serial) = msg.reply_serial() else {
                    return Vec::new();
                };
                match self.pending.remove(&serial) {
                    Some(Pending::Internal(i)) => self.internal_reply(i, msg),
                    Some(Pending::User) => {
                        let call = CallId {
                            bus: self.id,
                            serial,
                        };
                        let result = if msg.kind() == MessageKind::Error {
                            Err(CallError::Remote {
                                name: msg
                                    .error_name()
                                    .cloned()
                                    .expect("dbus: parse requires an error name"),
                                message: msg.error_text().unwrap_or_default(),
                            })
                        } else {
                            Ok(msg)
                        };
                        vec![out(MethodReply { call, result })]
                    }
                    None => Vec::new(),
                }
            }
            MessageKind::Signal => {
                let mut outs = self.daemon_signal(&msg);
                let matches = self.matches.matching(&msg, self.id);
                if !matches.is_empty() {
                    outs.push(out(SignalReceived {
                        bus: self.id,
                        matches,
                        msg,
                    }));
                }
                outs
            }
            MessageKind::MethodCall => self.incoming(msg),
        }
    }

    fn internal_reply(&mut self, i: Internal, msg: Message) -> Vec<Out> {
        match i {
            Internal::Hello => {
                if msg.kind() == MessageKind::MethodReturn
                    && let Ok((name,)) = msg.body::<(BusName,)>()
                {
                    self.unique_name = Some(name);
                    return Vec::new();
                }
                // Refused, or no name: install would wait forever.
                self.die()
            }
            Internal::AddMatch(key) => {
                if msg.kind() != MessageKind::Error {
                    return Vec::new();
                }
                let error = CallError::Remote {
                    name: msg
                        .error_name()
                        .cloned()
                        .expect("dbus: parse requires an error name"),
                    message: msg.error_text().unwrap_or_default(),
                };
                let bus = self.id;
                self.matches
                    .rules
                    .get(&key)
                    .map(|e| {
                        e.ids
                            .iter()
                            .map(|&n| {
                                out(MatchFailed {
                                    id: MatchId { bus, n },
                                    error: error.clone(),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            }
            Internal::OwnerMatch => Vec::new(),
            Internal::GetNameOwner(name) => {
                let unique = msg.body::<(BusName,)>().ok().map(|(u,)| u);
                if let Some(owner) = self.matches.owners.get_mut(&name) {
                    owner.unique = unique;
                }
                Vec::new()
            }
        }
    }

    /// A call to the daemon whose answer the connection handles itself.
    fn daemon_call<B: Body>(&mut self, member: &'static str, body: &B, why: Internal) {
        let msg = Message::method_call(DAEMON_PATH, member)
            .with_destination(DAEMON)
            .with_interface(DAEMON)
            .with_body(body);
        let serial = self.queue(&msg);
        self.pending.insert(serial, Pending::Internal(why));
    }

    fn daemon_call_no_reply<B: Body>(&mut self, member: &'static str, body: &B) {
        let msg = Message::method_call(DAEMON_PATH, member)
            .with_destination(DAEMON)
            .with_interface(DAEMON)
            .with_flags(Flags::NO_REPLY_EXPECTED)
            .with_body(body);
        self.queue(&msg);
    }

    pub(crate) fn add_match(&mut self, rule: MatchRule) -> MatchId {
        let n = self.matches.next;
        self.matches.next += 1;
        let key = rule.to_rule_string();
        self.matches.ids.insert(n, key.clone());
        let fresh = !self.matches.rules.contains_key(&key);
        let sender = rule.sender_name().cloned();
        self.matches
            .rules
            .entry(key.clone())
            .or_insert(Entry {
                rule,
                ids: Vec::new(),
            })
            .ids
            .push(n);
        if fresh {
            self.daemon_call(
                "AddMatch",
                &(key.as_str(),),
                Internal::AddMatch(key.clone()),
            );
            if let Some(s) = sender.filter(|s| !s.is_unique() && s.as_str() != DAEMON) {
                self.track_owner(s);
            }
        }
        MatchId { bus: self.id, n }
    }

    pub(crate) fn remove_match(&mut self, id: MatchId) {
        let Some(key) = self.matches.ids.remove(&id.n) else {
            return;
        };
        let entry = self
            .matches
            .rules
            .get_mut(&key)
            .expect("dbus: an id's rule exists");
        entry.ids.retain(|&x| x != id.n);
        if !entry.ids.is_empty() {
            return;
        }
        let entry = self.matches.rules.remove(&key).unwrap();
        self.daemon_call_no_reply("RemoveMatch", &(key.as_str(),));
        if let Some(s) = entry
            .rule
            .sender_name()
            .filter(|s| !s.is_unique() && s.as_str() != DAEMON)
        {
            self.untrack_owner(s.clone());
        }
    }

    /// Follow `name`'s owner: a `NameOwnerChanged` rule first, then
    /// `GetNameOwner`, so the daemon answers both before anything sent
    /// after them and no later signal can beat the owner.
    fn track_owner(&mut self, name: BusName) {
        let owner = self.matches.owners.entry(name.clone()).or_insert(Owner {
            refs: 0,
            unique: None,
        });
        owner.refs += 1;
        let first = owner.refs == 1;
        if first {
            let rule = owner_rule(&name);
            self.daemon_call("AddMatch", &(rule.as_str(),), Internal::OwnerMatch);
            self.daemon_call(
                "GetNameOwner",
                &(name.as_str(),),
                Internal::GetNameOwner(name.clone()),
            );
        }
    }

    fn untrack_owner(&mut self, name: BusName) {
        let Some(owner) = self.matches.owners.get_mut(&name) else {
            return;
        };
        owner.refs -= 1;
        if owner.refs == 0 {
            self.matches.owners.remove(&name);
            let rule = owner_rule(&name);
            self.daemon_call_no_reply("RemoveMatch", &(rule.as_str(),));
        }
    }

    /// The daemon's own signals: owner changes for followed names, and
    /// names this connection gained or lost.
    fn daemon_signal(&mut self, msg: &Message) -> Vec<Out> {
        if msg.sender().map(BusName::as_str) != Some(DAEMON)
            || msg.interface().map(InterfaceName::as_str) != Some(DAEMON)
        {
            return Vec::new();
        }
        let bus = self.id;
        match msg.member().map(MemberName::as_str) {
            Some("NameOwnerChanged") => {
                if let Ok((name, _, new)) = msg.body::<(String, String, String)>() {
                    let followed = BusName::new(&name)
                        .ok()
                        .and_then(|n| self.matches.owners.get_mut(&n));
                    if let Some(owner) = followed {
                        owner.unique = BusName::new(&new).ok();
                    }
                }
                Vec::new()
            }
            Some("NameAcquired") => msg
                .body::<(BusName,)>()
                .map(|(name,)| vec![out(NameAcquired { bus, name })])
                .unwrap_or_default(),
            Some("NameLost") => msg
                .body::<(BusName,)>()
                .map(|(name,)| vec![out(NameLost { bus, name })])
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    pub(crate) fn hello(&mut self) {
        let msg = Message::method_call(DAEMON_PATH, "Hello")
            .with_destination(DAEMON)
            .with_interface(DAEMON);
        let serial = self.queue(&msg);
        self.pending
            .insert(serial, Pending::Internal(Internal::Hello));
    }

    /// The bus is gone: fail every pending call, drop matches and
    /// exports, raise `BusDisconnected` once. The socket stays open until
    /// the connection drops, since an op in flight may still use it.
    pub(crate) fn die(&mut self) -> Vec<Out> {
        if self.dead {
            return Vec::new();
        }
        self.dead = true;
        // Close the socket so the daemon drops us; the fd stays valid
        // for operations still in flight.
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        self.outbox.clear();
        self.inbox.clear();
        self.in_fds.clear();
        let bus = self.id;
        let mut failed: Vec<u32> = self
            .pending
            .drain()
            .filter_map(|(serial, p)| matches!(p, Pending::User).then_some(serial))
            .collect();
        failed.sort_unstable();
        let mut outs: Vec<Out> = failed
            .into_iter()
            .map(|serial| {
                out(MethodReply {
                    call: CallId { bus, serial },
                    result: Err(CallError::Disconnected),
                })
            })
            .collect();
        self.deadlines.clear();
        self.matches = Matches::default();
        self.server.exports.clear();
        self.server.unanswered.borrow_mut().clear();
        outs.push(out(BusDisconnected { bus }));
        outs
    }
}

/// The rule that follows `name`'s owner.
fn owner_rule(name: &BusName) -> String {
    MatchRule::signal()
        .sender(DAEMON)
        .interface(DAEMON)
        .member("NameOwnerChanged")
        .arg(0, name.as_str())
        .to_rule_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> (Connection, UnixStream) {
        let (a, b) = UnixStream::pair().unwrap();
        (Connection::new(BusId::Session, a, true), b)
    }

    #[test]
    fn serials_skip_zero_when_they_wrap() {
        let (mut c, _peer) = conn();
        c.next_serial = u32::MAX;
        assert_eq!(c.serial(), u32::MAX);
        assert_eq!(c.serial(), 1);
    }

    #[test]
    fn a_partial_send_keeps_the_unsent_tail_first() {
        let (mut c, _peer) = conn();
        let mut ring = Ring::new(4);
        let t = Token::from_raw(7);
        c.sending = Some(t);
        c.outbox.push_back((b"next".to_vec(), Vec::new()));
        let outs = c.on_io(
            &IoEvent {
                token: t,
                result: 3,
            },
            Completed {
                buf: b"abcdef".to_vec(),
                fds: Vec::new(),
            },
            &mut ring,
        );
        assert!(outs.is_empty());
        assert!(c.sending.is_none());
        let queued: Vec<&[u8]> = c.outbox.iter().map(|(b, _)| b.as_slice()).collect();
        assert_eq!(queued, [b"def".as_slice(), b"next".as_slice()]);
    }

    #[test]
    fn a_read_completing_after_death_is_ignored() {
        let (mut c, _peer) = conn();
        let mut ring = Ring::new(4);
        c.die();
        let t = Token::from_raw(9);
        c.reading = Some(t);
        let bytes = Message::signal(
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameAcquired",
        )
        .with_sender("org.freedesktop.DBus")
        .with_body(&(":1.1",))
        .encode(5)
        .0;
        let outs = c.on_io(
            &IoEvent {
                token: t,
                result: bytes.len() as i32,
            },
            Completed {
                buf: bytes,
                fds: Vec::new(),
            },
            &mut ring,
        );
        assert!(outs.is_empty());
        assert!(c.reading.is_none());
        assert!(c.inbox.is_empty());
    }

    #[test]
    fn a_refused_hello_kills_the_connection() {
        let (mut c, _peer) = conn();
        c.hello();
        let reply = Message::error(1, "org.freedesktop.DBus.Error.AccessDenied", "no");
        c.handle(reply);
        assert!(c.dead);
        assert!(c.unique_name.is_none());
    }

    #[test]
    fn a_hello_reply_without_a_name_kills_the_connection() {
        let (mut c, _peer) = conn();
        c.hello();
        c.handle(Message::method_return(1).with_body(&(1u32,)));
        assert!(c.dead);
        assert!(c.unique_name.is_none());
    }

    #[test]
    fn an_unknown_message_type_is_skipped() {
        let (mut c, _peer) = conn();
        let mut bytes = Message::signal("/a", "a.b", "C")
            .with_sender(":1.9")
            .encode(5)
            .0;
        bytes[1] = 9;
        c.inbox.extend_from_slice(&bytes);
        let outs = c.read_messages();
        assert!(outs.is_empty());
        assert!(!c.dead);
        assert!(c.inbox.is_empty());
    }

    #[test]
    fn a_send_completing_after_death_queues_no_tail() {
        let (mut c, _peer) = conn();
        let mut ring = Ring::new(4);
        c.die();
        let t = Token::from_raw(7);
        c.sending = Some(t);
        c.on_io(
            &IoEvent {
                token: t,
                result: 1,
            },
            Completed {
                buf: b"abc".to_vec(),
                fds: Vec::new(),
            },
            &mut ring,
        );
        assert!(c.outbox.is_empty());
        assert!(c.sending.is_none());
    }
}
