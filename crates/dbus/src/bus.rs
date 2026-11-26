//! The `DBus` resource, the `Bus` view over one connection, the module
//! that installs them, and the systems that drive the I/O.

use std::ops::BitOr;
use std::os::unix::net::UnixStream;

use app::prelude::*;
use ring::prelude::*;

use crate::address;
use crate::auth;
use crate::client::{CallId, MethodCall, ObjectRef};
use crate::connection::{Connection, DAEMON, DAEMON_PATH, READ_CAPACITY};
use crate::matching::{MatchId, MatchRule};
use crate::message::Message;
use crate::names::{BusName, ErrorName, InterfaceName, MemberName, ObjectPath};
use crate::server::{ExportError, ExportId, IncomingCall, InterfaceSpec, PropertyError};
use crate::signals::{Out, emit};
use crate::types::Body;
use crate::value::Value;

/// Which bus. `Custom` is any other address: a private bus, a test's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BusId {
    System,
    Session,
    Custom(u8),
}

/// `RequestName`'s flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NameFlags(pub u32);

impl NameFlags {
    pub const ALLOW_REPLACEMENT: NameFlags = NameFlags(1);
    pub const REPLACE_EXISTING: NameFlags = NameFlags(2);
    pub const DO_NOT_QUEUE: NameFlags = NameFlags(4);
}

impl BitOr for NameFlags {
    type Output = NameFlags;
    fn bitor(self, rhs: NameFlags) -> NameFlags {
        NameFlags(self.0 | rhs.0)
    }
}

/// Every connection, one per bus requested from [`DBusModule`].
pub struct DBus {
    conns: Vec<Connection>,
}

impl Resource for DBus {}

impl DBus {
    /// # Panics
    ///
    /// If [`DBusModule::system`] was not requested: a setup bug.
    pub fn system(&mut self) -> Bus<'_> {
        self.bus(BusId::System)
            .expect("dbus: the system bus was not requested from DBusModule")
    }

    /// # Panics
    ///
    /// If [`DBusModule::session`] was not requested: a setup bug.
    pub fn session(&mut self) -> Bus<'_> {
        self.bus(BusId::Session)
            .expect("dbus: the session bus was not requested from DBusModule")
    }

    pub fn bus(&mut self, id: BusId) -> Option<Bus<'_>> {
        self.conns
            .iter_mut()
            .find(|c| c.id == id)
            .map(|conn| Bus { conn })
    }

    /// Answer `call` with `body`. A second answer is ignored.
    pub fn reply<B: Body>(&mut self, call: &IncomingCall, body: &B) {
        if let Some(bus) = self.bus(call.bus()) {
            bus.conn.answer(call, body);
        }
    }

    /// Answer `call` with an error. A second answer is ignored.
    pub fn reply_error(&mut self, call: &IncomingCall, name: impl Into<ErrorName>, text: &str) {
        if let Some(bus) = self.bus(call.bus()) {
            bus.conn.answer_error(call, name, text);
        }
    }

    pub(crate) fn conn_for(&self, t: Token) -> Option<BusId> {
        self.conns.iter().find(|c| c.owns(t)).map(|c| c.id)
    }

    pub(crate) fn conn_mut(&mut self, id: BusId) -> &mut Connection {
        self.conns
            .iter_mut()
            .find(|c| c.id == id)
            .expect("dbus: a bus this DBus owns")
    }

    #[cfg(feature = "fake")]
    pub(crate) fn has_pending_output(&self) -> bool {
        self.conns.iter().any(|c| c.has_pending_output())
    }

    /// Whether work is left on any bus, for the next tick or `BeforeWait`.
    pub(crate) fn has_work(&self) -> bool {
        self.conns.iter().any(|c| c.has_work())
    }

    /// The replies of calls that failed before being sent, on any bus.
    pub(crate) fn take_deferred(&mut self) -> Vec<Out> {
        self.conns
            .iter_mut()
            .flat_map(|c| c.take_deferred())
            .collect()
    }

    pub(crate) fn before_wait(&mut self, ring: &mut Ring) {
        for c in &mut self.conns {
            c.before_wait(ring);
        }
    }
}

/// One bus, borrowed from [`DBus`]: every client and server call is a
/// method here.
pub struct Bus<'a> {
    pub(crate) conn: &'a mut Connection,
}

impl<'a> Bus<'a> {
    pub fn id(&self) -> BusId {
        self.conn.id
    }

    /// The name the daemon gave this connection (`:1.42`).
    pub fn unique_name(&self) -> &BusName {
        self.conn
            .unique_name
            .as_ref()
            .expect("dbus: the unique name is known from install")
    }

    pub fn fds_supported(&self) -> bool {
        self.conn.fds_ok
    }

    /// False once the bus went away; see `BusDisconnected`.
    pub fn is_connected(&self) -> bool {
        !self.conn.dead
    }

    /// One remote object on this bus.
    pub fn object(
        self,
        destination: impl Into<BusName>,
        path: impl Into<ObjectPath>,
    ) -> ObjectRef<'a> {
        ObjectRef {
            conn: self.conn,
            destination: destination.into(),
            path: path.into(),
        }
    }

    /// Serve `spec` at `path`. Calls arrive as `MethodCallReceived`.
    pub fn export(
        &mut self,
        path: impl Into<ObjectPath>,
        spec: InterfaceSpec,
    ) -> Result<ExportId, ExportError> {
        self.conn.export(path.into(), spec)
    }

    pub fn unexport(&mut self, id: ExportId) {
        self.conn.unexport(id);
    }

    /// Update an exported property. Changes made in one turn go out as
    /// one `PropertiesChanged` per object and interface at `BeforeWait`;
    /// a value equal to the stored one sends nothing.
    pub fn set_property_value(
        &mut self,
        export: ExportId,
        name: &str,
        value: Value,
    ) -> Result<(), PropertyError> {
        self.conn.set_property_value(export, name, value)
    }

    /// Broadcast a signal.
    pub fn emit_signal<B: Body>(
        &mut self,
        path: impl Into<ObjectPath>,
        interface: impl Into<InterfaceName>,
        member: impl Into<MemberName>,
        body: &B,
    ) {
        let m = Message::signal(path, interface, member).with_body(body);
        self.conn.queue(&m);
    }

    /// Send a signal to one destination.
    pub fn emit_signal_to<B: Body>(
        &mut self,
        destination: impl Into<BusName>,
        path: impl Into<ObjectPath>,
        interface: impl Into<InterfaceName>,
        member: impl Into<MemberName>,
        body: &B,
    ) {
        let m = Message::signal(path, interface, member)
            .with_destination(destination)
            .with_body(body);
        self.conn.queue(&m);
    }

    /// Send `call`; its answer is a `MethodReply` with the returned id.
    pub fn send(&mut self, call: MethodCall) -> CallId {
        self.conn.call(call)
    }

    pub fn send_no_reply(&mut self, call: MethodCall) {
        self.conn.call_no_reply(call);
    }

    /// No `MethodReply` will be raised for `call`.
    pub fn cancel(&mut self, call: CallId) {
        if call.bus == self.conn.id {
            self.conn.cancel(call.serial);
        }
    }

    /// Ask the daemon to route matching signals here; each arrives as a
    /// `SignalReceived` listing the id returned.
    pub fn add_match(&mut self, rule: MatchRule) -> MatchId {
        self.conn.add_match(rule)
    }

    pub fn remove_match(&mut self, id: MatchId) {
        if id.bus == self.conn.id {
            self.conn.remove_match(id);
        }
    }

    /// `PropertiesChanged` for one interface of one object.
    pub fn watch_properties(
        &mut self,
        destination: impl Into<BusName>,
        path: impl Into<ObjectPath>,
        interface: impl Into<InterfaceName>,
    ) -> MatchId {
        self.add_match(MatchRule::properties_changed(destination, path, interface))
    }

    fn daemon<B: Body>(&mut self, member: &'static str, body: &B) -> CallId {
        self.conn
            .call(MethodCall::new(DAEMON, DAEMON_PATH, DAEMON, member).body(body))
    }

    /// The reply's body is `(u32,)`, the daemon's result code; ownership
    /// arrives as `NameAcquired`.
    pub fn request_name(&mut self, name: impl Into<BusName>, flags: NameFlags) -> CallId {
        let name = name.into();
        self.daemon("RequestName", &(name.as_str(), flags.0))
    }

    pub fn release_name(&mut self, name: impl Into<BusName>) -> CallId {
        let name = name.into();
        self.daemon("ReleaseName", &(name.as_str(),))
    }

    /// The reply's body is `(String,)`, the unique owner.
    pub fn get_name_owner(&mut self, name: impl Into<BusName>) -> CallId {
        let name = name.into();
        self.daemon("GetNameOwner", &(name.as_str(),))
    }

    /// The reply's body is `(bool,)`.
    pub fn name_has_owner(&mut self, name: impl Into<BusName>) -> CallId {
        let name = name.into();
        self.daemon("NameHasOwner", &(name.as_str(),))
    }

    /// The reply's body is `(Vec<String>,)`.
    pub fn list_names(&mut self) -> CallId {
        self.daemon("ListNames", &())
    }
}

enum Source {
    Address(String),
    Stream(UnixStream),
}

/// Connects and authenticates every requested bus at install, says
/// `Hello`, and turns the ring until each has its unique name. Installs
/// after `RingModule`.
#[derive(Default)]
pub struct DBusModule {
    buses: Vec<(BusId, Source)>,
}

impl DBusModule {
    pub fn new() -> Self {
        Self::default()
    }

    /// The system bus: `DBUS_SYSTEM_BUS_ADDRESS` or the standard socket.
    pub fn system(self) -> Self {
        self.address(BusId::System, &address::system())
    }

    /// The session bus: `DBUS_SESSION_BUS_ADDRESS` or `$XDG_RUNTIME_DIR/bus`.
    pub fn session(self) -> Self {
        self.address(BusId::Session, &address::session())
    }

    /// Any bus, by address: `unix:path=/run/user/1000/bus`.
    pub fn address(mut self, id: BusId, address: &str) -> Self {
        self.buses.push((id, Source::Address(address.to_owned())));
        self
    }

    /// Any bus, over a connected stream not yet authenticated.
    pub fn over(mut self, id: BusId, stream: UnixStream) -> Self {
        self.buses.push((id, Source::Stream(stream)));
        self
    }
}

impl Module for DBusModule {
    fn install(self, app: &mut App) {
        assert!(
            app.has_resource::<Ring>(),
            "dbus: DBusModule installs after RingModule"
        );
        app.system(on_tick)
            .system(on_before_wait)
            .system(on_io)
            .pending_check(|app| app.resource::<DBus>().has_work());
        let uid = auth::effective_uid().expect("dbus: read the effective uid");
        let mut conns: Vec<Connection> = Vec::new();
        for (id, source) in self.buses {
            assert!(
                conns.iter().all(|c| c.id != id),
                "dbus: {id:?} requested twice"
            );
            let (mut stream, what) = match source {
                Source::Address(a) => {
                    let s = address::connect(&a)
                        .unwrap_or_else(|e| panic!("dbus: connect {id:?} at {a}: {e}"));
                    (s, a)
                }
                Source::Stream(s) => (s, "the supplied stream".to_owned()),
            };
            let fds_ok = auth::authenticate(&mut stream, uid)
                .unwrap_or_else(|e| panic!("dbus: authenticate {id:?} at {what}: {e}"));
            let mut conn = Connection::new(id, stream, fds_ok);
            conn.arm_read(
                &mut app.resource_mut::<Ring>(),
                Vec::with_capacity(READ_CAPACITY),
            );
            conn.hello();
            conns.push(conn);
        }
        app.insert_resource(DBus { conns });
        while app
            .resource::<DBus>()
            .conns
            .iter()
            .any(|c| c.unique_name.is_none() && !c.dead)
        {
            Ring::turn(app);
        }
        if let Some(c) = app.resource::<DBus>().conns.iter().find(|c| c.dead) {
            panic!("dbus: {:?} closed or refused Hello", c.id);
        }
    }
}

/// Raise the replies of calls that failed before being sent.
fn on_tick(app: &mut App, _: &Tick) {
    let outs = app.resource_mut::<DBus>().take_deferred();
    emit(app, outs);
}

fn on_before_wait(app: &mut App, _: &BeforeWait) {
    let (mut dbus, mut ring) = app.query::<(ResMut<DBus>, ResMut<Ring>)>();
    dbus.before_wait(&mut ring);
}

fn on_io(app: &mut App, e: &IoEvent) {
    let Some(id) = app.resource::<DBus>().conn_for(e.token) else {
        return;
    };
    let done = app
        .resource_mut::<Ring>()
        .finish(e.token)
        .expect("dbus: a completed op is finishable");
    let outs = {
        let (mut dbus, mut ring) = app.query::<(ResMut<DBus>, ResMut<Ring>)>();
        dbus.conn_mut(id).on_io(e, done, &mut ring)
    };
    emit(app, outs);
}
