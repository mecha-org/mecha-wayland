#![forbid(unsafe_code)]
//! The UI runtime: a tree of nodes, each backed by a widget.
//!
//! # Model
//!
//! - [`App`] owns the tree. It is never empty: the root exists from
//!   [`App::new`], is its own parent, and cannot be removed.
//! - A [`NodeId`] names a node. It is opaque: a token to hand back to the
//!   app. A [`Handle`] is a `NodeId` that also knows the widget type. A
//!   removed node's id is stale for good, even once its slot is reused.
//! - A [`Widget`] is the per-node value. Widgets are stored by type, one
//!   column per type. The widget names its builder through
//!   [`Widget::Builder`] and owns the build: [`Widget::build`] runs once,
//!   after the node is linked into the tree and before the widget is
//!   stored, with a [`Spawner`] that attaches children under it.
//! - A [`Build`] is the marker a builder implements so that
//!   [`App::spawn`] can infer the widget type from the builder.
//! - A [`Component`] is per-node data outside the widget, one value of
//!   every registered type on every node, written through a guard that
//!   records the node as changed.
//! - A [`Resource`] is app-wide data, one value per type, inserted by
//!   value and written through a guard that records it as changed.
//!   [`App::query`] fetches columns and resources together, naming
//!   components as `&C` and `&mut C` and resources as [`Res`] and
//!   [`ResMut`]; [`App::fetch`] is the same query at one node.
//! - A [`Signal`] is an app-wide message; a [`System`] is a plain function
//!   run for every signal of its type. An [`Event`] is a node-level
//!   message sent to a list of nodes; a handler is a closure a widget's
//!   build attached with [`Spawner::on`], run with a [`Context`] over its
//!   owner, which queues and never flushes. Every emit is followed by an
//!   [`Emitted`] signal.
//! - Nothing runs when a message is sent. [`App::flush`] drains every
//!   event, runs one signal, and repeats. [`App::tick`] sends [`Tick`] and
//!   [`PostTick`] and flushes; [`App::run`] hands the app to the runner.
//! - A [`Module`] installs components, resources, systems and the runner.
//!   [`OnChanged`] comes from the `PostTick` drain each registration or
//!   first insert installs: an event for a component, a signal for a
//!   resource.
//!
//! # Failure
//!
//! Reads return `None` for a stale id or a wrong widget type.
//! [`App::remove`] returns `false` for a stale id. Panics are setup bugs:
//! spawning under a dead parent, removing the root, a handler on a stale
//! target, a second runner, a component registered twice or never, a
//! resource read before it is inserted, a fetch of a stale id.
//! Everything at run time skips and continues: a stale event target, a
//! signal with no systems, a handler whose owner is gone.
//!
//! The one run-time panic is [`Context::me`], and [`Context::fetch`]
//! under the same condition: it panics if a handler removed its own
//! owner, or an ancestor of it, earlier in the same call. [`App::remove`]
//! runs immediately, so that is a caller bug, not queued work; a handler
//! whose owner was already removed before it runs is skipped instead,
//! never called.
//!
//! Data calls add four more caller-bug panics: registering a component
//! twice, using a component or resource that was never registered or
//! inserted, indexing a view with a stale id, and a query or fetch that
//! names one place twice with a `&mut` or `ResMut`.
//!
//! # Components
//!
//! Per-node data outside the widget. Every node carries one value of
//! every registered [`Component`] type, `Default` until written. Reads
//! are plain references. Writes go through a [`CompMut`] guard whose
//! first `DerefMut` records the node as changed, and
//! [`App::take_changed`] drains that record in first-write order.
//! [`App::query`] fetches one or more columns at once as [`Comps`] and
//! [`CompsMut`] views, and [`App::split`] lends the tree read-only
//! beside them. [`App::spawn_with`] gives a node its initial values as a
//! [`Bundle`], a tuple of components written before its `build` runs.
//!
//! ```
//! use app::prelude::*;
//! # struct Leaf;
//! # impl Build for Leaf { type Widget = Leaf; }
//! # impl Widget for Leaf {
//! #     type Builder = Leaf;
//! #     fn build(b: Leaf, _: Handle<Self>, _: &mut Spawner<'_, Self>) -> Self { b }
//! # }
//! #[derive(Default, PartialEq, Debug)]
//! struct Depth(u32);
//! impl Component for Depth {}
//!
//! let mut app = App::new();
//! app.register_component::<Depth>();
//! let a = app.spawn(app.root(), Leaf);
//! let b = app.spawn(a, Leaf);
//!
//! let (tree, mut data) = app.split();
//! let mut depth = data.query::<&mut Depth>();
//! for id in tree.descendants(tree.root()) {
//!     depth.get_mut(id).unwrap().0 = tree.ancestors(id).count() as u32;
//! }
//! drop(depth);
//!
//! assert_eq!(app.component::<Depth>(b), Some(&Depth(2)));
//! assert_eq!(
//!     app.take_changed::<Depth>().collect::<Vec<_>>(),
//!     vec![a.id(), b.id()]
//! );
//! ```
//!
//! # Resources
//!
//! App-wide data outside the tree: one value per [`Resource`] type,
//! inserted with [`App::insert_resource`] or, for a `Default` type,
//! [`App::init_resource`]. Reads are plain references; writes go through
//! a [`ResourceMut`] guard whose first `DerefMut` records the resource as
//! changed, and each tick's `PostTick` drain turns that record into one
//! [`OnChanged<R>`](OnChanged) signal. A query names resources beside
//! components with [`Res`] and [`ResMut`]. Handlers and builds reach
//! resources through [`Context`] and [`Spawner`], and hold several guards
//! at once through their `fetch`.
//!
//! ```
//! use app::prelude::*;
//!
//! #[derive(Default, PartialEq, Debug)]
//! struct Frame(u64);
//! impl Resource for Frame {}
//!
//! fn advance(app: &mut App, _: &Tick) {
//!     app.resource_mut::<Frame>().0 += 1;
//! }
//! fn report(app: &mut App, _: &OnChanged<Frame>) {
//!     assert_eq!(app.resource::<Frame>(), &Frame(1));
//! }
//!
//! let mut app = App::new();
//! app.init_resource::<Frame>().system(advance).system(report);
//! app.tick();
//! assert_eq!(app.query::<Res<Frame>>(), &Frame(1));
//! ```
//!
//! # Quick start
//!
//! ```
//! use app::prelude::*;
//!
//! struct Counter(u32);
//! struct CounterBuilder { start: u32 }
//! impl Build for CounterBuilder { type Widget = Counter; }
//! impl Widget for Counter {
//!     type Builder = CounterBuilder;
//!     fn build(b: CounterBuilder, _me: Handle<Self>, _s: &mut Spawner<'_, Self>) -> Self {
//!         Counter(b.start)
//!     }
//! }
//!
//! struct Row;
//! struct RowBuilder { counters: u32 }
//! impl Build for RowBuilder { type Widget = Row; }
//! impl Widget for Row {
//!     type Builder = RowBuilder;
//!     fn build(b: RowBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
//!         for start in 0..b.counters {
//!             s.spawn(me, CounterBuilder { start });
//!         }
//!         Row
//!     }
//! }
//!
//! let mut app = App::new();
//! let row = app.spawn(app.root(), RowBuilder { counters: 3 });
//! assert_eq!(app.children(row).unwrap().len(), 3);
//!
//! for (_, counter) in app.widgets::<Counter>() {
//!     counter.0 += 10;
//! }
//! let first = app.children(row).unwrap()[0];
//! assert_eq!(app.widget::<Counter>(first).unwrap().0, 10);
//!
//! assert!(app.remove(row));
//! assert!(app.widget::<Counter>(first).is_none());
//! assert_eq!(app.widgets::<Counter>().count(), 0);
//! ```
//!
//! # Messages
//!
//! ```
//! use app::prelude::*;
//!
//! struct Click;
//! impl Event for Click {}
//!
//! struct Button(u32);
//! struct ButtonBuilder;
//! impl Build for ButtonBuilder { type Widget = Button; }
//! impl Widget for Button {
//!     type Builder = ButtonBuilder;
//!     fn build(_: ButtonBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
//!         s.on::<Click>(me, |ctx, _| ctx.me().0 += 1);
//!         Button(0)
//!     }
//! }
//!
//! fn count_clicks(app: &mut App, e: &Emitted<Click>) {
//!     let total: u32 = e.targets.iter().map(|&id| app.widget::<Button>(id).unwrap().0).sum();
//!     assert_eq!(total, 1);
//! }
//!
//! let mut app = App::new();
//! app.system(count_clicks);
//! let button = app.spawn(app.root(), ButtonBuilder);
//! app.emit(Click, button);
//! assert_eq!(app.widget::<Button>(button).unwrap().0, 0, "queued, not run");
//! app.flush();
//! assert_eq!(app.widget::<Button>(button).unwrap().0, 1);
//! ```

mod app;
mod bundle;
mod component;
mod context;
mod handler;
mod id;
mod message;
mod module;
mod nodes;
mod query;
mod resource;
mod slots;
mod store;
mod system;
mod tree;
mod widgets;

pub use app::{App, Spawner};
pub use bundle::Bundle;
pub use component::Component;
pub use context::Context;
pub use handler::Targets;
pub use id::{Handle, NodeId};
pub use message::{Emitted, Event, Moved, OnChanged, PostTick, Removed, Signal, Spawned, Tick};
pub use module::Module;
pub use query::{CompMut, Comps, CompsMut, Data, Query, Res, ResMut};
pub use resource::{Resource, ResourceMut};
pub use system::System;
pub use tree::Tree;
pub use widgets::{Build, Widget};

pub mod prelude {
    pub use crate::{
        App, Build, Bundle, CompMut, Component, Comps, CompsMut, Context, Data, Emitted, Event,
        Handle, Module, Moved, NodeId, OnChanged, PostTick, Query, Removed, Res, ResMut, Resource,
        ResourceMut, Signal, Spawned, Spawner, System, Targets, Tick, Tree, Widget,
    };
}
