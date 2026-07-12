use std::{collections::HashMap, fmt, marker::PhantomData, rc::Rc};
use zbus::export::serde::{Serialize, de::DeserializeOwned};
use zbus::{
    Message,
    message::Type as MessageType,
    zvariant::{DynamicType, Type},
};

use crate::{Bus, DbusProxy, connection::DbusMessage};

/// A D-Bus signal, described at the type level.
pub trait DbusSignal {
    const INTERFACE: &'static str;
    const MEMBER: &'static str;
    type Args: Type + Serialize + DeserializeOwned;

    fn match_rule() -> MatchRule
    where
        Self: Sized,
    {
        MatchRule::signal(Self::INTERFACE, Self::MEMBER)
    }
}

/// A handle to an installed match rule. Keep it to `unsubscribe` later
#[derive(Clone, Debug)]
pub struct Subscription {
    pub rule: String,
    pub serial: u32,
}

pub struct MatchRule {
    interface: &'static str,
    member: &'static str,
    sender: Option<String>,
    path: Option<String>,
}

impl MatchRule {
    pub(crate) fn signal(interface: &'static str, member: &'static str) -> Self {
        Self {
            interface,
            member,
            sender: None,
            path: None,
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

impl fmt::Display for MatchRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "type='signal',interface='{}',member='{}'",
            self.interface, self.member
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

/// A D-Bus method, described at the type level.
pub trait DbusMethod {
    const DESTINATION: &'static str;
    const PATH: &'static str;
    const INTERFACE: &'static str;
    const MEMBER: &'static str;
    type Args: Serialize + DynamicType;
    type Reply: Type + DeserializeOwned;
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
        if serial != 0 {
            self.map.insert(serial, ctx);
        }
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
        if serial != 0 {
            self.map.insert(serial, ctx);
        }
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

/// A D-Bus method you *implement* (serve), described at the type level. The
/// mirror of `DbusMethod`: `Args` is what callers send you, `Ret` is what you
/// return.
pub trait DbusHandler {
    const INTERFACE: &'static str;
    const MEMBER: &'static str;
    type Args: Type + DeserializeOwned;
    type Ret: Serialize + DynamicType;
}

impl<M: DbusMethod> DbusHandler for M
where
    M::Args: Type + DeserializeOwned,
    M::Reply: Serialize + DynamicType,
{
    const INTERFACE: &'static str = M::INTERFACE;
    const MEMBER: &'static str = M::MEMBER;
    type Args = M::Args;
    type Ret = M::Reply;
}

/// A decoded incoming method call for handler `M`, carrying the raw message so
/// you can reply. Obtain one with `IncomingCall::<M>::try_from(&ev.msg)`.
pub struct IncomingCall<M: DbusHandler> {
    pub path: Option<String>,
    pub sender: Option<String>,
    pub args: M::Args,
    raw: Rc<Message>,
    _m: PhantomData<M>,
}

impl<M: DbusHandler> IncomingCall<M> {
    /// `Some(Ok(_))` if `msg` is a call to `M`'s interface+member (and decodes);
    /// `Some(Err(_))` if it matches but the body is malformed; `None` otherwise.
    pub fn try_from(msg: &DbusMessage) -> Option<Result<Self, CallError>> {
        let DbusMessage::Call(message) = msg else {
            return None;
        };
        let hdr = message.header();
        if hdr.interface().map(|i| i.as_str()) != Some(M::INTERFACE)
            || hdr.member().map(|m| m.as_str()) != Some(M::MEMBER)
        {
            return None;
        }
        let path = hdr.path().map(|p| p.as_str().to_string());
        let sender = hdr.sender().map(|s| s.as_str().to_string());
        let raw = message.clone();
        let decoded = message
            .body()
            .deserialize::<M::Args>()
            .map(|args| IncomingCall {
                path,
                sender,
                args,
                raw,
                _m: PhantomData,
            })
            .map_err(CallError::Deserialize);
        Some(decoded)
    }

    /// Send the successful reply for this call.
    pub fn respond<B: Bus>(&self, proxy: &DbusProxy<B>, ret: &M::Ret) -> u32 {
        proxy.reply(&self.raw, ret)
    }

    /// Send an error reply for this call.
    pub fn error<B: Bus>(&self, proxy: &DbusProxy<B>, name: &str, text: &str) -> u32 {
        proxy.reply_error(&self.raw, name, text)
    }
}
