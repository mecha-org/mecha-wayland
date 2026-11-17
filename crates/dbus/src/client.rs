//! The calling side: [`MethodCall`], [`ObjectRef`], and what comes back.

use std::time::Duration;

use crate::bus::BusId;
use crate::connection::Connection;
use crate::message::{Flags, Message};
use crate::names::{BusName, ErrorName, InterfaceName, MemberName, ObjectPath};
use crate::types::Body;
use crate::value::Value;

/// libdbus's default: how long a call waits for its reply.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(25);
pub(crate) const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// Names one call: its bus and its serial there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CallId {
    pub bus: BusId,
    pub serial: u32,
}

/// Why a call has no result.
#[derive(Debug, Clone, PartialEq)]
pub enum CallError {
    /// The callee answered with an error.
    Remote { name: ErrorName, message: String },
    /// No reply within the call's timeout.
    Timeout,
    /// The bus went away first.
    Disconnected,
    /// The call carries fds and this bus refused fd passing.
    FdsUnsupported,
}

impl CallError {
    /// Whether this is the remote error `name`.
    pub fn is(&self, name: &str) -> bool {
        matches!(self, CallError::Remote { name: n, .. } if n.as_str() == name)
    }
}

/// A method call with every option; most calls go through [`ObjectRef`].
#[derive(Debug, Clone)]
pub struct MethodCall {
    pub(crate) msg: Message,
    pub(crate) timeout: Duration,
}

impl MethodCall {
    pub fn new(
        destination: impl Into<BusName>,
        path: impl Into<ObjectPath>,
        interface: impl Into<InterfaceName>,
        member: impl Into<MemberName>,
    ) -> Self {
        MethodCall {
            msg: Message::method_call(path, member)
                .with_destination(destination)
                .with_interface(interface),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn body<B: Body>(mut self, body: &B) -> Self {
        self.msg = self.msg.with_body(body);
        self
    }

    /// A body of values known only at run time.
    pub fn values(mut self, values: &[Value]) -> Self {
        self.msg = self.msg.with_values(values);
        self
    }

    /// Let the callee ask the user (polkit) before answering.
    pub fn allow_interactive_auth(mut self) -> Self {
        let flags = self.msg.flags() | Flags::ALLOW_INTERACTIVE_AUTHORIZATION;
        self.msg = self.msg.with_flags(flags);
        self
    }

    /// Fail the call with [`CallError::Timeout`] after `after`. A timeout
    /// too large to represent (such as `Duration::MAX`) never expires.
    pub fn timeout(mut self, after: Duration) -> Self {
        self.timeout = after;
        self
    }
}

/// One remote object, a destination and a path on one bus.
pub struct ObjectRef<'a> {
    pub(crate) conn: &'a mut Connection,
    pub(crate) destination: BusName,
    pub(crate) path: ObjectPath,
}

impl ObjectRef<'_> {
    fn method(&self, interface: InterfaceName, member: MemberName) -> MethodCall {
        MethodCall::new(
            self.destination.clone(),
            self.path.clone(),
            interface,
            member,
        )
    }

    pub fn call<B: Body>(
        &mut self,
        interface: impl Into<InterfaceName>,
        member: impl Into<MemberName>,
        body: &B,
    ) -> CallId {
        let call = self.method(interface.into(), member.into()).body(body);
        self.conn.call(call)
    }

    /// Sent with `NO_REPLY_EXPECTED`; nothing comes back.
    pub fn call_no_reply<B: Body>(
        &mut self,
        interface: impl Into<InterfaceName>,
        member: impl Into<MemberName>,
        body: &B,
    ) {
        let call = self.method(interface.into(), member.into()).body(body);
        self.conn.call_no_reply(call);
    }

    /// The reply's body is `(Value,)`: the property inside its variant.
    pub fn get_property(&mut self, interface: impl Into<InterfaceName>, name: &str) -> CallId {
        let i = interface.into();
        self.call(PROPERTIES, "Get", &(i.as_str(), name))
    }

    /// The reply's body is `(PropMap,)`.
    pub fn get_all(&mut self, interface: impl Into<InterfaceName>) -> CallId {
        let i = interface.into();
        self.call(PROPERTIES, "GetAll", &(i.as_str(),))
    }

    pub fn set_property(
        &mut self,
        interface: impl Into<InterfaceName>,
        name: &str,
        value: Value,
    ) -> CallId {
        let i = interface.into();
        self.call(PROPERTIES, "Set", &(i.as_str(), name, value))
    }
}
