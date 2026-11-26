#![forbid(unsafe_code)]
//! D-Bus on the ring: call, watch and serve objects on the system and
//! session buses from the one io_uring loop, with no threads and no
//! async runtime.
//!
//! # Model
//!
//! - [`DBus`] is a resource holding one connection per bus requested
//!   from [`DBusModule`]: [`DBus::system`], [`DBus::session`], or
//!   [`DBus::bus`] for a [`BusId::Custom`] address. Each gives a [`Bus`],
//!   on which every call below is a method. Reach it with
//!   `app.resource_mut::<DBus>()` in a system and
//!   `ctx.resource_mut::<DBus>()` in a widget handler.
//! - [`DBusModule`] installs after `RingModule`. At install it connects,
//!   authenticates (blocking, `EXTERNAL`), says `Hello` and turns the
//!   ring until every bus has its unique name.
//! - Ordinary sending belongs in `Tick` systems and in handlers. A
//!   `BeforeWait` system ordered after [`DBusModule`] sends a round
//!   later: D-Bus has already flushed by then, and leaves the send as
//!   work left.
//! - A call that fails before it is sent, on a dead bus or with fds the
//!   bus refuses, gets its [`MethodReply`] in the next tick.
//! - Nothing is a callback or a future. Every result is a signal:
//!   [`MethodReply`] for a [`CallId`], [`SignalReceived`] for the
//!   [`MatchId`]s a signal satisfies, [`MethodCallReceived`] and
//!   [`PropertySetRequested`] for exported objects, [`NameAcquired`],
//!   [`NameLost`], [`MatchFailed`] and [`BusDisconnected`].
//! - Calling: [`Bus::object`] gives an [`ObjectRef`] with `call`,
//!   `get_property`, `get_all`, `set_property`; [`MethodCall`] has every
//!   option (timeouts, interactive authorization). A call's timeout
//!   (25 s by default) fails it with [`CallError::Timeout`].
//! - Watching: [`Bus::add_match`] with a [`MatchRule`];
//!   [`Bus::watch_properties`] for `PropertiesChanged`. A rule naming a
//!   well-known sender follows that name's owner.
//! - Serving: [`Bus::export`] an [`InterfaceSpec`]; answer each
//!   [`IncomingCall`] with [`DBus::reply`] now or later. A call dropped
//!   unanswered gets `NoReply`. Introspectable, Peer and Properties are
//!   built in; property values live in the library, updated with
//!   [`Bus::set_property_value`].
//! - Values: typed through [`Encode`]/[`Decode`] (tuples are bodies and
//!   structs), dynamic through [`Value`]; `a{sv}` is [`PropMap`].
//!
//! # With a UI
//!
//! A service module (NetworkManager, BlueZ, a portal backend) owns the
//! D-Bus side: it handles the signals above and keeps a domain resource
//! up to date. Writing that resource raises `OnChanged<R>`; a system
//! turns it into an event at the widget's node, whose handler updates
//! the widget. Widgets ask for actions with domain signals the service
//! module handles, so `Message` and `Value` stay out of widget code.
//! `examples/wifi.rs` is the pattern end to end.
//!
//! # Failure
//!
//! Setup bugs panic: installing before `RingModule`, a bus that cannot
//! connect or authenticate at install, [`DBus::system`] or
//! [`DBus::session`] for a bus not requested, an invalid name literal, a
//! string holding a NUL, a property whose initial value has the wrong
//! type. Nothing a peer sends panics: a malformed message drops the
//! connection, which raises [`BusDisconnected`] once and fails every
//! pending call with [`CallError::Disconnected`]. A body that does not
//! decode is still delivered; [`Message::body`] returns the error. There
//! is no reconnection.
//!
//! # Quick start
//!
//! ```no_run
//! use app::prelude::*;
//! use dbus::prelude::*;
//! use ring::prelude::*;
//!
//! #[derive(Default)]
//! struct Pending(Option<CallId>);
//! impl Resource for Pending {}
//!
//! fn on_reply(app: &mut App, e: &MethodReply) {
//!     if app.resource::<Pending>().0 != Some(e.call) {
//!         return;
//!     }
//!     if let Ok(msg) = &e.result {
//!         if let Ok((state,)) = msg.body::<(Value,)>() {
//!             println!("NetworkManager state: {state:?}");
//!         }
//!     }
//! }
//!
//! let mut app = App::new();
//! app.add_module(RingModule::default())
//!     .add_module(DBusModule::new().system())
//!     .init_resource::<Pending>()
//!     .system(on_reply);
//! let call = app
//!     .resource_mut::<DBus>()
//!     .system()
//!     .object("org.freedesktop.NetworkManager", "/org/freedesktop/NetworkManager")
//!     .get_property("org.freedesktop.NetworkManager", "State");
//! app.resource_mut::<Pending>().0 = Some(call);
//! app.run();
//! ```

mod address;
mod auth;
mod bus;
mod client;
mod connection;
mod fd;
mod matching;
mod message;
mod names;
mod server;
mod signals;
mod signature;
mod standard;
mod types;
mod value;
mod wire;

#[cfg(feature = "fake")]
pub mod fake;

pub use bus::{Bus, BusId, DBus, DBusModule, NameFlags};
pub use client::{CallError, CallId, DEFAULT_TIMEOUT, MethodCall, ObjectRef};
pub use fd::UnixFd;
pub use matching::{MatchId, MatchRule, PropertiesChanged};
pub use message::{Flags, MAX_MESSAGE, Message, MessageKind};
pub use names::{BusName, ErrorName, InterfaceName, MemberName, NameError, ObjectPath};
pub use server::{Access, ExportError, ExportId, IncomingCall, InterfaceSpec, PropertyError};
pub use signals::{
    BusDisconnected, MatchFailed, MethodCallReceived, MethodReply, NameAcquired, NameLost,
    PropertySetRequested, SignalReceived,
};
pub use signature::Signature;
pub use types::{Basic, Body, Decode, Encode, FromBody, Type, from_bytes, to_bytes};
pub use value::{PropMap, PropMapExt, Value};
pub use wire::{DecodeError, Decoder, Encoder, MAX_ARRAY, MAX_DEPTH};

pub mod prelude {
    pub use crate::{
        Access, Bus, BusDisconnected, BusId, BusName, CallError, CallId, DBus, DBusModule,
        ErrorName, ExportError, ExportId, IncomingCall, InterfaceName, InterfaceSpec, MatchFailed,
        MatchId, MatchRule, MemberName, Message, MessageKind, MethodCall, MethodCallReceived,
        MethodReply, NameAcquired, NameFlags, NameLost, ObjectPath, ObjectRef, PropMap, PropMapExt,
        PropertiesChanged, PropertyError, PropertySetRequested, SignalReceived, Signature, UnixFd,
        Value,
    };
}
