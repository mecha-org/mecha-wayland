use std::{collections::HashMap, fmt, marker::PhantomData};

use serde::{Serialize, de::DeserializeOwned};
use zbus::{
    Message,
    message::Type as MessageType,
    zvariant::{DynamicType, Endian, Type},
};

use crate::{Bus, DbusProxy, connection::DbusMessage};

/// A D-Bus method, described at the type level.
pub trait DbusMethod {
    const DESTINATION: &'static str;
    const PATH: &'static str;
    const INTERFACE: &'static str;
    const MEMBER: &'static str;
    type Args: Serialize + DynamicType;
    type Reply: Type + DeserializeOwned;
}

/// A D-Bus signal, described at the type level.
pub trait DbusSignal {
    const INTERFACE: &'static str;
    const MEMBER: &'static str;
    type Args: Type + DeserializeOwned;

    fn match_rule() -> MatchRule<Self>
    where
        Self: Sized,
    {
        MatchRule::new()
    }
}

/// A handle to an installed match rule. Keep it to `unsubscribe` later — the bus
/// removes rules by their exact string, so the token carries that string for you.
#[derive(Clone, Debug)]
pub struct Subscription {
    pub rule: String,
}

impl Subscription {
    /// The exact match-rule string that was installed.
    pub fn rule(&self) -> &str {
        &self.rule
    }
}

pub struct MatchRule<S: DbusSignal> {
    sender: Option<String>,
    path: Option<String>,
    _s: PhantomData<S>,
}

impl<S: DbusSignal> MatchRule<S> {
    fn new() -> Self {
        Self {
            sender: None,
            path: None,
            _s: PhantomData,
        }
    }
    pub fn sender(mut self, s: impl Into<String>) -> Self {
        self.sender = Some(s.into());
        self
    }
    pub fn path(mut self, p: impl Into<String>) -> Self {
        self.path = Some(p.into());
        self
    }
}

impl<S: DbusSignal> fmt::Display for MatchRule<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "type='signal',interface='{}',member='{}'",
            S::INTERFACE,
            S::MEMBER
        )?;
        if let Some(s) = &self.sender {
            write!(f, ",sender='{s}'")?;
        }
        if let Some(p) = &self.path {
            write!(f, ",path='{p}'")?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum CallError {
    Bus {
        name: Option<String>,
        text: Option<String>,
    },
    Deserialize(zbus::Error),
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CallError::Bus { name, text } => write!(
                f,
                "dbus error {}: {}",
                name.as_deref().unwrap_or("?"),
                text.as_deref().unwrap_or("")
            ),
            CallError::Deserialize(e) => write!(f, "reply decode error: {e}"),
        }
    }
}
impl std::error::Error for CallError {}

fn decode_reply<M: DbusMethod>(message: &Message) -> Result<M::Reply, CallError> {
    if message.message_type() == MessageType::Error {
        let name = message
            .header()
            .error_name()
            .map(|n| n.as_str().to_string());
        let text = message.body().deserialize::<String>().ok();
        return Err(CallError::Bus { name, text });
    }
    message
        .body()
        .deserialize::<M::Reply>()
        .map_err(CallError::Deserialize)
}

/// Outstanding calls of method `M`, each tagged with context `C`.
pub struct Pending<M: DbusMethod, C = ()> {
    map: HashMap<u32, C>,
    _m: PhantomData<M>,
}

impl<M: DbusMethod, C> Default for Pending<M, C> {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
            _m: PhantomData,
        }
    }
}

impl<M: DbusMethod, C> Pending<M, C> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Send `M` at its declared path and record `ctx` against the reply.
    pub fn call<B: Bus>(&mut self, proxy: &DbusProxy<B>, args: &M::Args, ctx: C) -> u32 {
        let serial = proxy.call::<M>(args);
        self.map.insert(serial, ctx);
        serial
    }

    /// Send `M` at a runtime path (e.g. a per-object path) and record `ctx`.
    pub fn call_at<B: Bus>(
        &mut self,
        proxy: &DbusProxy<B>,
        path: &str,
        args: &M::Args,
        ctx: C,
    ) -> u32 {
        let serial = proxy.call_at::<M>(path, args);
        self.map.insert(serial, ctx);
        serial
    }

    /// If `msg` answers one of these calls, remove it and hand back the context
    /// together with the decoded reply.
    pub fn resolve(&mut self, msg: &DbusMessage) -> Option<(C, Result<M::Reply, CallError>)> {
        if let DbusMessage::Reply { serial, message } = msg {
            if let Some(ctx) = self.map.remove(serial) {
                return Some((ctx, decode_reply::<M>(message)));
            }
        }
        None
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// A typed, decoded signal plus the path/sender it came from.
pub struct SignalMatch<S: DbusSignal> {
    pub path: Option<String>,
    pub sender: Option<String>,
    pub args: S::Args,
    _s: PhantomData<S>,
}

impl<S: DbusSignal> SignalMatch<S> {
    pub fn try_from(msg: &DbusMessage) -> Option<Result<Self, CallError>> {
        let DbusMessage::Signal(message) = msg else {
            return None;
        };
        let hdr = message.header();
        if hdr.interface().map(|i| i.as_str()) != Some(S::INTERFACE)
            || hdr.member().map(|m| m.as_str()) != Some(S::MEMBER)
        {
            return None;
        }
        let path = hdr.path().map(|p| p.as_str().to_string());
        let sender = hdr.sender().map(|s| s.as_str().to_string());
        let decoded = message
            .body()
            .deserialize::<S::Args>()
            .map(|args| SignalMatch {
                path,
                sender,
                args,
                _s: PhantomData,
            })
            .map_err(CallError::Deserialize);
        Some(decoded)
    }
}

pub mod fdo {
    use crate::dbus_method;

    dbus_method!(pub Hello {
        dest: "org.freedesktop.DBus",
        path: "/org/freedesktop/DBus",
        iface: "org.freedesktop.DBus",
        member: "Hello",
        args: (), reply: String,
    });
    dbus_method!(pub AddMatch {
        dest: "org.freedesktop.DBus",
        path: "/org/freedesktop/DBus",
        iface: "org.freedesktop.DBus",
        member: "AddMatch",
        args: (String,), reply: (),
    });
    dbus_method!(pub RemoveMatch {
        dest: "org.freedesktop.DBus",
        path: "/org/freedesktop/DBus",
        iface: "org.freedesktop.DBus",
        member: "RemoveMatch",
        args: (String,), reply: (),
    });
}
