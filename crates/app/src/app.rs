use std::collections::VecDeque;

use crate::component::{Component, Components};
use crate::context::Context;
use crate::handler::{Handler, HandlerColumns, Targets, dispatch};
use crate::message::{Emitted, Moved, OnChanged, PostTick, Removed, Spawned, Tick};
use crate::nodes::{Node, Nodes};
use crate::query::{CompMut, Data, Query};
use crate::resource::{ResourceMut, Resources};
use crate::slots::Slots;
use crate::system::{System, Systems};
use crate::tree::Tree;
use crate::widgets::{Root, Widgets};
use crate::{Build, Bundle, Event, Handle, Module, NodeId, Resource, Signal, Widget};

/// A queued unit of work: a signal or an event with its dispatch baked
/// in, so the queue needs no knowledge of the concrete type.
pub(crate) type Job = Box<dyn FnOnce(&mut App)>;

/// The runtime: a tree of nodes, each backed by a widget stored in a
/// per-type column, plus the systems, handlers and queues that drive it.
///
/// Five arenas share one slot index: `slots`, `nodes`, `widgets`,
/// `components` and `handlers`. `slots` is the only validator; the rest
/// trust the index they are given. `resources` stands apart: one value
/// per type, no slot. The tree is never empty: [`App::new`]
/// creates the root, which is its own parent and cannot be removed.
///
/// Messages are queued, never run inline: [`App::signal`] and
/// [`App::emit`] push jobs, [`App::flush`] runs them.
pub struct App {
    slots: Slots,
    nodes: Nodes,
    widgets: Widgets,
    components: Components,
    /// One value per inserted type. Not an arena: no slot, nothing per
    /// node.
    resources: Resources,
    systems: Systems,
    pub(crate) handlers: HandlerColumns,
    /// Events wait here. Drained before every signal.
    events: VecDeque<Job>,
    /// Signals wait here. One runs per round of `flush`.
    signals: VecDeque<Job>,
    /// Set by a module that owns the loop. `None` means the default
    /// runner, which ticks forever.
    runner: Option<fn(App)>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let mut app = App {
            slots: Slots::new(),
            nodes: Nodes::new(),
            widgets: Widgets::new(),
            components: Components::new(),
            resources: Resources::new(),
            systems: Systems::new(),
            handlers: HandlerColumns::new(),
            events: VecDeque::new(),
            signals: VecDeque::new(),
            runner: None,
        };
        let column = app.widgets.column::<Root>(0);
        let (index, generation) = app.slots.alloc(column);
        let len = app.slots.len();
        app.widgets.grow(len);
        let id = NodeId::new(generation, column, index);
        debug_assert_eq!(id, NodeId::ROOT);
        app.nodes.grow(len);
        app.nodes.set(index, Node::new(id));
        app.widgets
            .store_mut::<Root>(column)
            .expect("root column allocated above")
            .set(index, Some(Root));
        app
    }

    /// Always live; the only node whose parent is itself.
    pub fn root(&self) -> NodeId {
        NodeId::ROOT
    }

    // ── tree: write ──────────────────────────────────────────────────────

    /// [`App::spawn_with`] with no initial component values.
    ///
    /// # Panics
    ///
    /// If `parent` is not live. See [`App::spawn_with`].
    pub fn spawn<B: Build>(&mut self, parent: impl Into<NodeId>, builder: B) -> Handle<B::Widget> {
        self.spawn_with(parent, builder, ())
    }

    /// Build `builder`'s widget, and whatever subtree its `build` attaches,
    /// as the last child of `parent`, with the component values in
    /// `bundle` written to the new node first.
    ///
    /// Order: the node is linked into the tree, then the bundle is written
    /// (each value flags the node like any other write), then `build` runs
    /// with a handle to the node and can read those values through
    /// [`Spawner::component`], then the widget is stored. So `children(parent)`
    /// already lists the node during its build, and a lookup of the node's
    /// own widget during its build is `None`. Queues a [`Spawned`] signal
    /// once the widget is stored.
    ///
    /// `bundle` is `()` or a tuple of one to six components; see [`Bundle`].
    ///
    /// # Panics
    ///
    /// If `parent` is not live, or a type in `bundle` is not registered.
    pub fn spawn_with<B: Build, K: Bundle>(
        &mut self,
        parent: impl Into<NodeId>,
        builder: B,
        bundle: K,
    ) -> Handle<B::Widget> {
        let parent = parent.into();
        assert!(
            self.slots.is_live(parent),
            "spawn under a stale parent: {parent:?}"
        );
        let widget_type = self.widgets.column::<B::Widget>(self.slots.len());
        let (index, generation) = self.slots.alloc(widget_type);
        let len = self.slots.len();
        self.widgets.grow(len);
        self.nodes.grow(len);
        self.components.grow(len);
        self.handlers.grow(len);

        let id = NodeId::new(generation, widget_type, index);
        self.nodes.set(index, Node::new(parent));
        self.node_mut(parent).children.push(id);

        bundle.insert(self, id);

        let handle = Handle::new(id);
        let widget = B::Widget::build(
            builder,
            handle,
            &mut Spawner {
                app: self,
                me: handle,
            },
        );
        debug_assert!(self.slots.is_live(id), "build removed its own node");
        self.widgets
            .store_mut::<B::Widget>(widget_type)
            .expect("column allocated above")
            .set(index, Some(widget));
        self.signal(Spawned { id, parent });
        handle
    }

    /// Remove `id` and every node under it. Every id in the subtree is
    /// stale afterwards and its slots go back to the free list. Queues a
    /// [`Removed`] signal for `id` before the node is freed.
    ///
    /// Returns `false`, changing nothing, if `id` is already stale:
    /// removing a subtree twice is legitimate once queued work exists.
    ///
    /// # Panics
    ///
    /// If `id` is the root.
    pub fn remove(&mut self, id: impl Into<NodeId>) -> bool {
        let id = id.into();
        if !self.slots.is_live(id) {
            return false;
        }
        assert!(id != NodeId::ROOT, "the root node cannot be removed");

        let parent = self.node(id).parent;
        self.signal(Removed { id, parent });
        let siblings = &mut self.node_mut(parent).children;
        let position = siblings
            .iter()
            .position(|c| *c == id)
            .expect("live node is in its parent's children");
        siblings.remove(position);

        // Parent before children: free a node, then push its children. Every
        // descendant is reached before we return, and each is freed after
        // its parent so nothing is left pointing into the tree.
        let mut pending = vec![id];
        while let Some(current) = pending.pop() {
            self.slots.free(current.slot());
            self.widgets.free(current.widget_type(), current.slot());
            self.components.free(current.slot());
            self.handlers.free(current.slot());
            let node = self.nodes.take(current.slot());
            pending.extend(node.children);
        }
        true
    }

    /// Move `id`, and every node under it, to be the last child of
    /// `parent`. The nodes keep their ids, widgets, components and
    /// handlers: the same nodes, somewhere else. Queues a [`Moved`]
    /// signal, for whatever depends on where a node is (its window, its
    /// layout, its drawing). Moving a node to the parent it has does
    /// nothing.
    ///
    /// # Panics
    ///
    /// If either id is stale, `id` is the root, or `parent` is `id` or
    /// under it.
    pub fn move_to(&mut self, id: impl Into<NodeId>, parent: impl Into<NodeId>) {
        let (id, parent) = (id.into(), parent.into());
        assert!(
            self.slots.is_live(id) && self.slots.is_live(parent),
            "a move of a stale node: {id:?} to {parent:?}"
        );
        assert!(id != NodeId::ROOT, "the root node cannot be moved");
        assert!(
            parent != id && !self.ancestors(parent).any(|a| a == id),
            "a node cannot move under itself: {id:?} to {parent:?}"
        );
        let from = self.node(id).parent;
        if from == parent {
            return;
        }
        let siblings = &mut self.node_mut(from).children;
        let position = siblings
            .iter()
            .position(|c| *c == id)
            .expect("live node is in its parent's children");
        siblings.remove(position);
        self.node_mut(parent).children.push(id);
        self.node_mut(id).parent = parent;
        self.signal(Moved {
            id,
            from,
            to: parent,
        });
    }

    // ── tree: read ───────────────────────────────────────────────────────

    /// The tree as a read-only view. Every read below is this view's
    /// method; take the view itself to hold the tree across calls or to
    /// pair it with a column view through [`App::split`].
    pub fn tree(&self) -> Tree<'_> {
        Tree::new(&self.slots, &self.nodes)
    }

    /// Whether `id` names a node that exists right now.
    pub fn is_live(&self, id: impl Into<NodeId>) -> bool {
        self.tree().is_live(id)
    }

    /// `None` if `id` is stale. The root's parent is the root.
    pub fn parent(&self, id: impl Into<NodeId>) -> Option<NodeId> {
        self.tree().parent(id)
    }

    /// In sibling order. `None` if `id` is stale.
    pub fn children(&self, id: impl Into<NodeId>) -> Option<&[NodeId]> {
        self.tree().children(id)
    }

    /// The parent chain from `id` up to and including the root, nearest
    /// first, excluding `id`. Empty for the root and for a stale id.
    pub fn ancestors(&self, id: impl Into<NodeId>) -> impl Iterator<Item = NodeId> + '_ {
        self.tree().ancestors(id)
    }

    /// The subtree under `id` in pre-order (a node before its children,
    /// children in sibling order), excluding `id`. Empty for a leaf and
    /// for a stale id. Borrows the app, so the tree cannot change while
    /// the iterator is alive.
    pub fn descendants(&self, id: impl Into<NodeId>) -> impl Iterator<Item = NodeId> + '_ {
        self.tree().descendants(id)
    }

    // ── widgets ──────────────────────────────────────────────────────────

    /// `None` if `id` is stale, holds a widget of another type, or is the
    /// node whose `build` is currently running.
    pub fn widget<W: Widget>(&self, id: impl Into<NodeId>) -> Option<&W> {
        let id = id.into();
        if !self.slots.is_live(id) {
            return None;
        }
        self.widgets
            .store::<W>(id.widget_type())?
            .get(id.slot())
            .as_ref()
    }

    /// `None` under the same conditions as [`App::widget`].
    pub fn widget_mut<W: Widget>(&mut self, id: impl Into<NodeId>) -> Option<&mut W> {
        let id = id.into();
        if !self.slots.is_live(id) {
            return None;
        }
        self.widgets
            .store_mut::<W>(id.widget_type())?
            .get_mut(id.slot())
            .as_mut()
    }

    /// Every live widget of type `W` with its id, in slot order. Empty if
    /// no `W` was ever spawned. Slot order is not tree order.
    pub fn widgets<W: Widget>(&mut self) -> impl Iterator<Item = (NodeId, &mut W)> {
        let slots = &self.slots;
        let widgets = &mut self.widgets;
        let column = widgets.column_of::<W>();
        let store = column.and_then(|c| widgets.store_mut::<W>(c).map(|s| (c, s)));
        store.into_iter().flat_map(move |(widget_type, store)| {
            store.iter_mut().filter_map(move |(index, slot)| {
                let w = slot.as_mut()?;
                let id = NodeId::new(slots.generation(index), widget_type, index);
                debug_assert!(slots.is_live(id), "a widget in a vacant slot");
                Some((id, w))
            })
        })
    }

    // ── components ───────────────────────────────────────────────────────

    /// Allocate a column for `C`. Every live node, the root included,
    /// holds `C::default()` from here on, and so does every node spawned
    /// later. Also installs the [`PostTick`] system that turns the
    /// column's changed list into [`OnChanged<C>`] events each tick.
    ///
    /// # Panics
    ///
    /// If `C` is already registered.
    pub fn register_component<C: Component>(&mut self) -> &mut Self {
        self.components.register::<C>(self.slots.len());
        self.system::<PostTick>(drain_changed::<C>)
    }

    /// One node's `C`. `None` if `id` is stale.
    ///
    /// Pays the column lookup on every call; a pass over many nodes takes
    /// a view through [`App::query`] once instead.
    ///
    /// # Panics
    ///
    /// If `C` is not registered.
    pub fn component<C: Component>(&self, id: impl Into<NodeId>) -> Option<&C> {
        self.components.column::<C>().get(&self.slots, id.into())
    }

    /// A write guard for one node's `C`. `None` if `id` is stale. The
    /// guard's first `DerefMut` records the node for [`App::take_changed`].
    ///
    /// # Panics
    ///
    /// If `C` is not registered.
    pub fn component_mut<C: Component>(&mut self, id: impl Into<NodeId>) -> Option<CompMut<'_, C>> {
        let slots = &self.slots;
        self.components.column_mut::<C>().get_mut(slots, id.into())
    }

    /// Every node whose `C` was written since the last drain, in
    /// first-write order, live nodes only. Clears the record as it goes;
    /// dropping the iterator early still clears. O(changed).
    ///
    /// This is the hand-off to the events slice, which will turn each id
    /// into an [`OnChanged<C>`](crate::OnChanged) dispatch. Nothing fires
    /// here.
    ///
    /// # Panics
    ///
    /// If `C` is not registered.
    pub fn take_changed<C: Component>(&mut self) -> impl Iterator<Item = NodeId> + '_ {
        let slots = &self.slots;
        self.components.column_mut::<C>().take_changed(slots)
    }

    /// Fetch one or more columns and resources as views: `&C` for a
    /// [`Comps`](crate::Comps), `&mut C` for a [`CompsMut`](crate::CompsMut),
    /// [`Res<R>`](crate::Res) for a `&R`, [`ResMut<R>`](crate::ResMut) for a
    /// [`ResourceMut`](crate::ResourceMut), or a tuple of up to six of
    /// those.
    ///
    /// ```
    /// # use app::prelude::*;
    /// # #[derive(Default)]
    /// # struct Style;
    /// # impl Component for Style {}
    /// # #[derive(Default)]
    /// # struct Rect;
    /// # impl Component for Rect {}
    /// # #[derive(Default)]
    /// # struct Windows;
    /// # impl Resource for Windows {}
    /// # let mut app = App::new();
    /// # app.register_component::<Style>();
    /// # app.register_component::<Rect>();
    /// # app.init_resource::<Windows>();
    /// let (style, mut rect, windows) = app.query::<(&Style, &mut Rect, Res<Windows>)>();
    /// # let _ = (&style, &mut rect, windows);
    /// ```
    ///
    /// Takes `&mut self` even for a read-only query, since one signature
    /// covers both, so the views hold the app exclusively while they live.
    /// Cost per column: one hash and one downcast. Nothing per node.
    /// Cost per resource element: one pass over the resource map, a
    /// `TypeId` compare per inserted resource.
    ///
    /// # Panics
    ///
    /// If a named type is not registered or inserted, or one place appears
    /// twice with at least one `&mut` or `ResMut`. `(&A, &A)` and
    /// `(Res<A>, Res<A>)` are fine; a type that is both a component and a
    /// resource is two places.
    pub fn query<Q: Query>(&mut self) -> Q::Out<'_> {
        Q::fetch(Data::new(
            &self.slots,
            &mut self.components,
            &mut self.resources,
        ))
    }

    /// [`App::query`] scoped to one node: the same elements, but each
    /// component element yields `id`'s value (`&C`) or write guard
    /// ([`CompMut`](crate::CompMut)) instead of a column view; resource
    /// elements are as in `query`. Everything is borrowed at once, so
    /// several guards on one node may coexist, which `component_mut`
    /// cannot offer since each call borrows the whole app.
    ///
    /// ```
    /// # use app::prelude::*;
    /// # #[derive(Default)]
    /// # struct Rect;
    /// # impl Component for Rect {}
    /// # #[derive(Default)]
    /// # struct Windows;
    /// # impl Resource for Windows {}
    /// # let mut app = App::new();
    /// # app.register_component::<Rect>();
    /// # app.init_resource::<Windows>();
    /// # let id = app.root();
    /// let (mut rect, windows) = app.fetch::<(&mut Rect, Res<Windows>)>(id);
    /// # let _ = (&mut rect, windows);
    /// ```
    ///
    /// # Panics
    ///
    /// If `id` is stale: the id came from a handle the caller holds, and
    /// [`App::component_mut`] offers the `Option` form for the doubtful
    /// case. Also under [`App::query`]'s conditions.
    pub fn fetch<Q: Query>(&mut self, id: impl Into<NodeId>) -> Q::One<'_> {
        Q::fetch_one(
            Data::new(&self.slots, &mut self.components, &mut self.resources),
            id.into(),
        )
    }

    /// The tree read-only and the data mutably, at the same time. For a
    /// pass that walks the tree while writing a column or a resource,
    /// such as layout reading `children` and writing a rect.
    pub fn split(&mut self) -> (Tree<'_>, Data<'_>) {
        (
            Tree::new(&self.slots, &self.nodes),
            Data::new(&self.slots, &mut self.components, &mut self.resources),
        )
    }

    // ── resources ────────────────────────────────────────────────────────

    /// Store `value` as the app's `R`, replacing and returning what was
    /// there. Every insert counts as a write for
    /// [`OnChanged<R>`](crate::OnChanged), the way a bundle's initial
    /// component values flag the node. The first insert of `R` also
    /// installs the [`PostTick`] system that turns the changed record into
    /// an [`OnChanged<R>`](crate::OnChanged) signal each tick; a replace
    /// does not install it again, since systems are never removed.
    pub fn insert_resource<R: Resource>(&mut self, value: R) -> Option<R> {
        let old = self.resources.insert(value);
        if old.is_none() {
            self.system::<PostTick>(drain_resource_changed::<R>);
        }
        old
    }

    /// Store `R::default()` if no `R` is present, without counting it as a
    /// write, the way a spawn's `Default` does not flag. A no-op if `R` is
    /// present, so a module can call it for a resource an earlier module
    /// may already have provided. Installs the same drain as
    /// [`App::insert_resource`] when it inserts.
    pub fn init_resource<R: Resource + Default>(&mut self) -> &mut Self {
        if self.resources.init::<R>() {
            self.system::<PostTick>(drain_resource_changed::<R>);
        }
        self
    }

    /// Whether an `R` is present. One hash; the getters panic instead.
    pub fn has_resource<R: Resource>(&self) -> bool {
        self.resources.has::<R>()
    }

    /// The app's `R`.
    ///
    /// # Panics
    ///
    /// If no `R` was inserted or initialised: a setup bug, the same class
    /// as an unregistered component.
    pub fn resource<R: Resource>(&self) -> &R {
        self.resources.get::<R>()
    }

    /// A write guard for the app's `R`. The guard's first `DerefMut`
    /// records the resource for [`App::take_resource_changed`].
    ///
    /// # Panics
    ///
    /// If no `R` was inserted or initialised.
    pub fn resource_mut<R: Resource>(&mut self) -> ResourceMut<'_, R> {
        self.resources.get_mut::<R>()
    }

    /// Whether `R` was written since the last call, clearing the record.
    /// The resource half of [`App::take_changed`]; the
    /// [`OnChanged<R>`](crate::OnChanged) drain is written in terms of
    /// it, and a custom runner may drain by hand.
    ///
    /// # Panics
    ///
    /// If no `R` was inserted or initialised.
    pub fn take_resource_changed<R: Resource>(&mut self) -> bool {
        self.resources.take_changed::<R>()
    }

    // ── systems and signals ──────────────────────────────────────────────

    /// Run `system` for every `S` that is flushed from now on, after the
    /// systems registered for `S` before it. Never removed.
    pub fn system<S: Signal>(&mut self, system: System<S>) -> &mut Self {
        self.systems.add(system);
        self
    }

    /// Queue `signal` for the systems registered for `S`. Nothing runs
    /// until [`App::flush`]. If no system is registered for `S` right
    /// now the signal is dropped here, so unlistened traffic costs one
    /// hash; a system registered between the send and the flush still
    /// runs, because the job looks the run up when it runs.
    pub fn signal<S: Signal>(&mut self, signal: S) {
        if !self.systems.has::<S>() {
            return;
        }
        self.signals
            .push_back(Box::new(move |app: &mut App| run_signal(app, &signal)));
    }

    // ── events ───────────────────────────────────────────────────────────

    /// Queue `event` for the handlers of every node in `targets`, in that
    /// order. Nothing runs until [`App::flush`]. A target that is stale
    /// by then is skipped; a node with no handler for `E` costs one
    /// liveness check. Every emit is followed by an [`Emitted<E>`] signal
    /// carrying the event and its targets, if any system is registered for
    /// it.
    pub fn emit<E: Event>(&mut self, event: E, targets: impl Into<Targets>) {
        let targets = targets.into();
        self.events.push_back(Box::new(move |app: &mut App| {
            run_event(app, event, targets)
        }));
    }

    /// Queue `event` for every node, including the root, in tree pre-order:
    /// a node before its children, children in sibling order.
    ///
    /// Targets are collected when this method is called. Nothing runs
    /// until [`App::flush`]. Nodes spawned afterwards do not receive this
    /// event; targets removed before their turn in dispatch are skipped,
    /// even if their slots have been reused. Nodes without a handler for
    /// `E` are harmless.
    ///
    /// Uses one [`App::emit`] for the entire target list. After dispatch,
    /// queues one [`Emitted<E>`] signal if a system is registered for it,
    /// carrying the event and the original targets (including any that
    /// were skipped). A tree with only the root still targets the root.
    pub fn emit_all<E: Event>(&mut self, event: E) {
        let targets: Vec<NodeId> = std::iter::once(self.root())
            .chain(self.descendants(self.root()))
            .collect();
        self.emit(event, targets);
    }

    /// Run everything queued: every event, then one signal, then every
    /// event again, until both queues are empty. Work a job queues runs
    /// in the same call. A no-op on empty queues.
    pub fn flush(&mut self) {
        loop {
            while let Some(job) = self.events.pop_front() {
                job(self);
            }
            match self.signals.pop_front() {
                Some(job) => job(self),
                None => break,
            }
        }
    }

    // ── tick and runner ──────────────────────────────────────────────────

    /// One step of the loop: queue [`Tick`], queue [`PostTick`], flush.
    /// `Tick`'s systems run, then everything they caused, then `PostTick`'s
    /// systems, then everything those caused.
    pub fn tick(&mut self) {
        self.signal(Tick);
        self.signal(PostTick);
        self.flush();
    }

    /// The function [`App::run`] hands the app to. Set by the module that
    /// owns the loop, typically one that blocks on a compositor.
    ///
    /// # Panics
    ///
    /// If a runner is already set: two modules both think they own the
    /// loop.
    pub fn set_runner(&mut self, runner: fn(App)) -> &mut Self {
        assert!(self.runner.is_none(), "runner already set");
        self.runner = Some(runner);
        self
    }

    /// Hand the app to the runner. Returns when the runner does; the
    /// default runner never does.
    pub fn run(self) {
        let runner = self.runner.unwrap_or(default_runner);
        runner(self)
    }

    // ── modules ──────────────────────────────────────────────────────────

    /// Install `module` now. See [`Module`].
    pub fn add_module(&mut self, module: impl Module) -> &mut Self {
        module.install(self);
        self
    }

    // ── internals ────────────────────────────────────────────────────────

    /// The node record of an id the caller has already validated.
    fn node(&self, id: NodeId) -> &Node {
        self.nodes.get(id.slot())
    }

    fn node_mut(&mut self, id: NodeId) -> &mut Node {
        self.nodes.get_mut(id.slot())
    }
}

/// The runner when no module set one: tick forever. A busy loop, right
/// for a headless app until a presentation module sets a runner that
/// blocks.
fn default_runner(mut app: App) {
    loop {
        app.tick();
    }
}

/// Run `signal` through every system for `S`, by index, so a system
/// registered during the pass runs in it.
fn run_signal<S: Signal>(app: &mut App, signal: &S) {
    let mut i = 0;
    while let Some(system) = app.systems.get::<S>(i) {
        system(app, signal);
        i += 1;
    }
}

/// Run one queued emit: the handlers of every target, then the
/// `Emitted<E>` signal for the systems, behind every signal already
/// queued.
fn run_event<E: Event>(app: &mut App, event: E, targets: Targets) {
    dispatch(app, &event, &targets);
    app.signal(Emitted { event, targets });
}

/// The `PostTick` system behind `OnChanged<C>`: drain the changed list
/// and, if anything came out, emit once to all of it. On `PostTick`
/// rather than `Tick` so that writes by every `Tick` system, whenever it
/// was registered, and by every event `Tick` caused, fire in this tick.
fn drain_changed<C: Component>(app: &mut App, _: &PostTick) {
    let ids: Vec<NodeId> = app.take_changed::<C>().collect();
    if !ids.is_empty() {
        app.emit(OnChanged::<C>::new(), ids);
    }
}

/// The `PostTick` system behind `OnChanged<R>`: clear the record and, if
/// it was set, signal once. On `PostTick` for the same reason as
/// `drain_changed`.
fn drain_resource_changed<R: Resource>(app: &mut App, _: &PostTick) {
    if app.take_resource_changed::<R>() {
        app.signal(OnChanged::<R>::new());
    }
}

/// What a [`Widget::build`] gets to touch while it runs: the node being
/// built, attaching children under it, single-node component access
/// meant for `me` and the nodes it spawned, the app's resources, and
/// attaching handlers on its behalf. Typed by the widget being built so
/// a handler it registers knows what `me` is. Deliberately narrow: no
/// whole-column views, no drain, no split, no messages. [`Spawner::fetch`]
/// is the way to hold several guards on one node at once.
pub struct Spawner<'a, W: Widget> {
    app: &'a mut App,
    me: Handle<W>,
}

impl<W: Widget> Spawner<'_, W> {
    /// The node being built: the `me` the build was given.
    pub fn me(&self) -> Handle<W> {
        self.me
    }

    /// Build `builder` as the last child of `parent`, which is the `me`
    /// the build was given or a handle returned by an earlier `spawn` in
    /// the same build. Either way it is live. See [`App::spawn`].
    pub fn spawn<B: Build>(&mut self, parent: impl Into<NodeId>, builder: B) -> Handle<B::Widget> {
        self.app.spawn(parent, builder)
    }

    /// Run `handler` whenever an `E` lands on `target`, on behalf of the
    /// node being built: the handler's [`Context::me`] is this node and
    /// its [`Context::target`] is `target`. `target` is usually `me`, or a
    /// child spawned earlier in the same build. Handlers for one
    /// `(target, E)` run in registration order; they go away with the
    /// target, and are skipped once this node is gone.
    ///
    /// # Panics
    ///
    /// If `target` is stale.
    pub fn on<E: Event>(
        &mut self,
        target: impl Into<NodeId>,
        mut handler: impl FnMut(&mut Context<'_, W>, &E) + 'static,
    ) -> &mut Self {
        let target = target.into();
        assert!(
            self.app.is_live(target),
            "handler on a stale target: {target:?}"
        );
        let owner = self.me;
        let erased: Handler<E> = Box::new(move |app: &mut App, target: NodeId, event: &E| {
            if !app.is_live(owner) {
                return;
            }
            let mut ctx = Context::new(app, owner, target);
            handler(&mut ctx, event);
        });
        let len = self.app.slots.len();
        self.app
            .handlers
            .column_mut::<E>(len)
            .get_mut(target.slot())
            .0
            .push(erased);
        self
    }

    /// [`Spawner::spawn`] with initial component values. See
    /// [`App::spawn_with`].
    pub fn spawn_with<B: Build, K: Bundle>(
        &mut self,
        parent: impl Into<NodeId>,
        builder: B,
        bundle: K,
    ) -> Handle<B::Widget> {
        self.app.spawn_with(parent, builder, bundle)
    }

    /// See [`App::component`].
    pub fn component<C: Component>(&self, id: impl Into<NodeId>) -> Option<&C> {
        self.app.component(id)
    }

    /// See [`App::component_mut`]. A write here flags like any other
    /// write; the `Default` a spawn starts from does not.
    pub fn component_mut<C: Component>(&mut self, id: impl Into<NodeId>) -> Option<CompMut<'_, C>> {
        self.app.component_mut(id)
    }

    /// See [`App::resource`]. Panics if `R` is not inserted.
    pub fn resource<R: Resource>(&self) -> &R {
        self.app.resource::<R>()
    }

    /// See [`App::resource_mut`]. Panics if `R` is not inserted.
    pub fn resource_mut<R: Resource>(&mut self) -> ResourceMut<'_, R> {
        self.app.resource_mut::<R>()
    }

    /// See [`App::fetch`]. `id` is `me` or a node spawned earlier in this
    /// build, so it is live; a stale id is a caller bug and panics.
    pub fn fetch<Q: Query>(&mut self, id: impl Into<NodeId>) -> Q::One<'_> {
        self.app.fetch::<Q>(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_is_column_zero_slot_zero() {
        let app = App::new();
        assert_eq!(app.root(), NodeId::new(0, 0, 0));
        assert_eq!(app.widgets.column_of::<Root>(), Some(0));
        assert!(app.widgets.store::<Root>(0).unwrap().get(0).is_some());
    }

    struct Leaf;
    impl Build for Leaf {
        type Widget = Leaf;
    }
    impl Widget for Leaf {
        type Builder = Leaf;
        fn build(b: Leaf, _: Handle<Leaf>, _: &mut Spawner<'_, Self>) -> Leaf {
            b
        }
    }

    #[test]
    fn a_reused_slot_keeps_its_index_and_bumps_the_generation() {
        let mut app = App::new();
        let old = app.spawn(app.root(), Leaf).id();
        assert!(app.remove(old));
        let new = app.spawn(app.root(), Leaf).id();
        assert_eq!(new.slot(), old.slot());
        assert_eq!(new.widget_type(), old.widget_type());
        assert_eq!(new.generation(), old.generation() + 1);
        assert!(app.is_live(new));
        assert!(!app.is_live(old));
    }

    #[test]
    fn spawn_grows_and_remove_frees_the_handler_columns() {
        struct Poke;
        impl crate::Event for Poke {}

        let mut app = App::new();
        let len = app.slots.len();
        app.handlers.column_mut::<Poke>(len);
        let id = app.spawn(app.root(), Leaf).id();
        let column = app.handlers.column_of::<Poke>().unwrap();
        column.get_mut(id.slot()).0.push(Box::new(|_, _, _| {}));
        assert_eq!(column.get(id.slot()).0.len(), 1, "spawn grew the column");
        app.remove(id);
        let column = app.handlers.column_of::<Poke>().unwrap();
        assert!(
            column.get(id.slot()).0.is_empty(),
            "remove emptied the slot"
        );
    }

    #[test]
    fn removed_slots_are_back_to_default_in_every_arena() {
        let mut app = App::new();
        let id = app.spawn(app.root(), Leaf).id();
        app.remove(id);
        let node = app.nodes.get(id.slot());
        assert_eq!(node.parent, NodeId::ROOT);
        assert!(node.children.is_empty());
        let column = app.widgets.column_of::<Leaf>().unwrap();
        assert!(
            app.widgets
                .store::<Leaf>(column)
                .unwrap()
                .get(id.slot())
                .is_none()
        );
    }
}
