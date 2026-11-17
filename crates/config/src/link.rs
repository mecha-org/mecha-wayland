//! Connects a source to mxconfigd through the `dbus` crate.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;

use dbus::{
    BusId, CallError, CallId, DBus, MatchId, MatchRule, Message, MethodCall, MethodReply,
    SignalReceived, Signature, UnixFd, Value,
};
use mxconfig::runtime::wire::{Arg, RULES};
use mxconfig::runtime::{Drive, Event};
use mxconfig::{Bus, Source};

/// A connection to the daemon on one bus.
pub(crate) struct Link {
    pub(crate) bus: Bus,
    pub(crate) id: BusId,
    /// Request ids of the calls in flight.
    waiting: HashMap<CallId, u64>,
    rules: [MatchId; RULES.len()],
}

/// A link on each bus `DBusModule` connected.
pub(crate) fn connect(source: &Source, dbus: &mut DBus) -> Vec<Link> {
    let buses = [(BusId::Session, Bus::Session), (BusId::System, Bus::System)];
    let mut links = Vec::new();
    for (id, bus) in buses {
        let Some(mut conn) = dbus.bus(id) else {
            continue;
        };
        let rules = RULES.map(|rule| {
            let mut matched = MatchRule::signal()
                .sender(rule.sender)
                .interface(rule.interface)
                .member(rule.member);
            if let Some(arg0) = rule.arg0 {
                matched = matched.arg(0, arg0);
            }
            conn.add_match(matched)
        });
        source.reach(bus);
        links.push(Link {
            bus,
            id,
            waiting: HashMap::new(),
            rules,
        });
    }
    links
}

impl Link {
    pub(crate) fn matches(&self, e: &SignalReceived) -> bool {
        self.rules.iter().any(|id| e.matches.contains(id))
    }

    /// Send the source's queued calls. Returns whether any were sent.
    pub(crate) fn send(&mut self, source: &Source, dbus: &mut DBus) -> bool {
        // Without the connection, the calls wait for it.
        let Some(mut conn) = dbus.bus(self.id) else {
            return false;
        };
        let calls = source.take_calls(self.bus);
        let sent = !calls.is_empty();
        for call in calls {
            let args = match call.args.iter().map(value).collect::<io::Result<Vec<_>>>() {
                Ok(args) => args,
                Err(e) => {
                    source.deliver(self.bus, Event::unsent(call.id, e));
                    continue;
                }
            };
            let mut message =
                MethodCall::new(call.destination, call.path, call.interface, call.member)
                    .values(&args);
            if call.interactive {
                message = message.allow_interactive_auth();
            }
            let id = conn.send(message);
            self.waiting.insert(id, call.id);
        }
        sent
    }

    /// Deliver `e` if it answers one of this link's calls. Returns whether it did.
    pub(crate) fn reply(&mut self, source: &Source, e: &MethodReply) -> bool {
        let Some(id) = self.waiting.remove(&e.call) else {
            return false;
        };
        let event = match &e.result {
            Ok(message) => Event::Reply {
                id,
                args: args(message),
            },
            Err(CallError::Remote { name, message }) => Event::Failed {
                id,
                name: name.as_str().to_string(),
                message: message.clone(),
            },
            // Unreaching the bus settles it, a sent write as unconfirmed.
            Err(CallError::Disconnected) => return true,
            Err(CallError::Timeout) => Event::timed_out(id),
            Err(other) => Event::unsent(id, format!("{other:?}")),
        };
        source.deliver(self.bus, event);
        true
    }

    pub(crate) fn signal(&self, source: &Source, e: &SignalReceived) {
        let message = &e.msg;
        let (Some(sender), Some(interface), Some(member)) =
            (message.sender(), message.interface(), message.member())
        else {
            return;
        };
        let event = Event::Signal {
            sender: sender.as_str().to_string(),
            interface: interface.as_str().to_string(),
            member: member.as_str().to_string(),
            args: args(message),
        };
        source.deliver(self.bus, event);
    }
}

/// A message body's arguments, or none if it can't be read.
fn args(message: &Message) -> Vec<Arg> {
    let values = message.body_values().unwrap_or_default();
    values.into_iter().map(arg).collect()
}

/// `arg` as a dbus value. Fails only if an fd can't be duplicated.
fn value(arg: &Arg) -> io::Result<Value> {
    let values = |args: &[Arg]| args.iter().map(value).collect::<io::Result<Vec<_>>>();
    Ok(match arg {
        Arg::Str(text) => Value::Str(text.clone()),
        Arg::U64(n) => Value::U64(*n),
        Arg::Bool(b) => Value::Bool(*b),
        Arg::Fd(fd) => Value::Fd(UnixFd::new(fd.try_clone()?)),
        Arg::Array(signature, items) => Value::Array {
            elem: Signature::new(signature).expect("a valid signature"),
            items: values(items)?,
        },
        Arg::Struct(fields) => Value::Struct(values(fields)?),
        Arg::Variant(inner) => Value::Variant(Box::new(value(inner)?)),
        Arg::Dict(_) | Arg::Kept(_) | Arg::Unreadable => {
            unreachable!("mxconfig sends none of these")
        }
    })
}

/// A dbus value as an [`Arg`]. Others mxconfig never sends.
fn arg(value: Value) -> Arg {
    match value {
        Value::Str(text) => Arg::Str(text),
        Value::U64(n) => Arg::U64(n),
        Value::Bool(b) => Arg::Bool(b),
        Value::Fd(fd) => fd
            .try_clone_owned()
            .map_or(Arg::Unreadable, |fd| Arg::Fd(Arc::new(fd))),
        Value::Array { elem, items } => Arg::Array(
            elem.as_str().to_string(),
            items.into_iter().map(arg).collect(),
        ),
        Value::Struct(fields) => Arg::Struct(fields.into_iter().map(arg).collect()),
        Value::Dict { entries, .. } => {
            Arg::Dict(entries.into_iter().map(|(k, v)| (arg(k), arg(v))).collect())
        }
        Value::Variant(inner) => Arg::Variant(Box::new(arg(*inner))),
        _ => Arg::Unreadable,
    }
}
