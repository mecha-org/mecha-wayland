//! The two kinds of message in the runtime, and the built-in ones.
//!
//! - A [`Signal`] is app-wide: it goes to every *system* registered for
//!   its type, through [`App::signal`](crate::App::signal).
//! - An [`Event`] is node-level: it goes to the *handlers* of the nodes it
//!   is emitted at, through [`App::emit`](crate::App::emit).
//!
//! Both are markers. Any `'static` type can be one, or both. Nothing runs
//! when a message is sent; [`App::flush`](crate::App::flush) runs it.

use std::marker::PhantomData;

use crate::NodeId;
use crate::handler::Targets;
use crate::{Component, Resource};

/// An app-wide message consumed by systems.
///
/// ```
/// # use app::Signal;
/// struct Resized { width: u32, height: u32 }
/// impl Signal for Resized {}
/// ```
pub trait Signal: 'static {}

/// A node-level message consumed by handlers.
///
/// ```
/// # use app::Event;
/// struct Click;
/// impl Event for Click {}
/// ```
pub trait Event: 'static {}

/// The built-in signal [`App::tick`](crate::App::tick) sends first, once
/// per tick. Systems that do a frame's work sit here.
pub struct Tick;
impl Signal for Tick {}

/// The built-in signal [`App::tick`](crate::App::tick) sends after
/// [`Tick`], once everything `Tick` caused has run. The
/// [`OnChanged`](crate::OnChanged) drains sit here, component and
/// resource alike, so a tick's writes fire in the same tick.
pub struct PostTick;
impl Signal for PostTick {}

/// A node was spawned. Queued by [`App::spawn`](crate::App::spawn) after
/// the node's `build` returned and its widget is stored, so a system
/// sees a finished node. A subtree's signals arrive children before
/// parent, since a child's spawn finishes inside its parent's build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spawned {
    pub id: NodeId,
    pub parent: NodeId,
}
impl Signal for Spawned {}

/// A subtree was removed. Queued by [`App::remove`](crate::App::remove),
/// once per call, for the subtree's root only. By the time a system sees
/// it, `id` is stale; `parent` is the node it was removed from and is
/// usually still live, but is not guaranteed to be, since it may itself
/// be removed later in the same flush. Both fields are for bookkeeping,
/// not lookups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Removed {
    pub id: NodeId,
    pub parent: NodeId,
}
impl Signal for Removed {}

/// A subtree moved. Queued by [`App::move_to`](crate::App::move_to),
/// once per call, for the subtree's root only: `id` was a child of `from`
/// and is now the last child of `to`. Either node may be gone by the time
/// a system sees it, if it was removed later in the same flush.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Moved {
    pub id: NodeId,
    pub from: NodeId,
    pub to: NodeId,
}
impl Signal for Moved {}

/// The signal form of an emit: the event by value and the nodes it went
/// to, sent after those nodes' handlers ran. How a system observes an
/// event without owning a node. Dropped, like any signal, when no system
/// is registered for it.
pub struct Emitted<E: Event> {
    pub event: E,
    pub targets: Targets,
}
impl<E: Event> Signal for Emitted<E> {}

/// A `T` was written since the last tick's drain, where `T` is a
/// component or a resource. A marker either way: the receiver reads the
/// data itself.
///
/// For a component, an *event*: emitted once per tick from a `PostTick`
/// system that `register_component::<C>` installs, to every node
/// `take_changed::<C>()` yields. A node written twice fires once; a node
/// removed after its write does not fire. A handler is
/// `s.on::<OnChanged<Rect>>(me, ..)`; a system that wants every changed
/// id at once registers for `Emitted<OnChanged<Rect>>` and reads
/// `targets`.
///
/// For a resource, a *signal*: sent once per tick from a `PostTick`
/// system that the first `insert_resource::<R>` or the inserting
/// `init_resource::<R>` installs, if `take_resource_changed::<R>()` was
/// set. An insert counts as a write; an init does not. A system is
/// `fn(&mut App, &OnChanged<Windows>)`.
///
/// Order within a tick: component drains emit events, resource drains
/// queue signals, and `flush` runs every queued event before the next
/// signal. So every `OnChanged<C>` handler of a tick runs before the
/// first `OnChanged<R>` system, and a resource written by an
/// `OnChanged<C>` handler fires on the next tick, since its drain
/// already ran. Drains run in registration order: components in
/// `register_component` order, resources in first-insert order.
pub struct OnChanged<T: 'static>(PhantomData<fn() -> T>);

impl<T: 'static> OnChanged<T> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

impl<T: 'static> Default for OnChanged<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: Component> Event for OnChanged<C> {}
impl<R: Resource> Signal for OnChanged<R> {}
