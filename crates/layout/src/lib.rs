#![forbid(unsafe_code)]
//! The layout module: `LayoutStyle` and `Measure` in, `ComputedLayout` out,
//! through taffy's flexbox and block algorithms over the core's columns.
//!
//! # Model
//!
//! - Every node carries a [`LayoutStyle`], the box it asks for, and a
//!   [`Measure`], how a leaf sizes itself under the constraints it is
//!   offered. Both are written by whoever owns the node.
//! - A node marked with [`LayoutRoot`]`(true)` is the top of one tree to
//!   lay out. Its subtree is laid out in its coordinates, the root's box
//!   at the origin, sized by the root's own style. A node under no root is
//!   never laid out.
//! - Every node carries a [`ComputedLayout`], the target box in whole
//!   pixels. The animation module maintains the separate displayed
//!   `animation::Layout`, copying targets when animation is disabled.
//! - The pass runs on `PostTick`, ahead of the core's `OnChanged` drains,
//!   and takes the change records for the three inputs itself, so
//!   `OnChanged<LayoutStyle>`, `OnChanged<Measure>` and
//!   `OnChanged<LayoutRoot>` never fire while [`LayoutModule`] is
//!   installed. A spawn or removal dirties the parent's root. Only dirty
//!   roots are recomputed; taffy's cache skips unchanged subtrees.
//! - [`Viewport`] shifts and clips how descendants are drawn and hit. It
//!   is read by render and the hit test, never by the pass.
//! - [`LayoutDone`] is signalled once per tick, after the pass, naming
//!   the roots recomputed; empty when none was.
//!
//! # When a change is seen
//!
//! Everything written between ticks, and everything written by a `Tick`
//! system or a handler `Tick` caused, is laid out in the next `PostTick`.
//! A spawn or removal queued by a `Tick` system, or a write by a system
//! on a signal `Tick` queued, arrives after `PostTick` and is laid out the
//! tick after. Nothing is lost.
//!
//! # Quick start
//!
//! ```
//! use app::prelude::*;
//! use geometry::Rect;
//! use layout::prelude::*;
//!
//! # struct Leaf;
//! # impl Build for Leaf { type Widget = Leaf; }
//! # impl Widget for Leaf {
//! #     type Builder = Leaf;
//! #     fn build(b: Leaf, _: Handle<Self>, _: &mut Spawner<'_, Self>) -> Self { b }
//! # }
//! let mut app = App::new();
//! app.add_module(LayoutModule);
//!
//! // A window-sized root, and a box inside it.
//! let window = app.spawn_with(
//!     app.root(),
//!     Leaf,
//!     (LayoutRoot(true), LayoutStyle::default().size(px(200.0), px(100.0))),
//! );
//! let panel = app.spawn_with(window, Leaf, (LayoutStyle::default().size(px(50.0), percent(100.0)),));
//!
//! app.tick();
//! assert_eq!(app.component::<ComputedLayout>(panel).unwrap().rect, Rect::new(0.0, 0.0, 50.0, 100.0));
//!
//! // Restyle between ticks; the next tick lays it out.
//! app.component_mut::<LayoutStyle>(panel).unwrap().width = px(80.0);
//! app.tick();
//! assert_eq!(app.component::<ComputedLayout>(panel).unwrap().rect.width(), 80.0);
//! ```

mod context;
mod style;
mod tree;

use std::fmt;

use app::{App, Component, Module, NodeId, PostTick, Removed, Resource, Signal, Spawned};
use geometry::{Insets, Point, Rect, Size};
use taffy::AvailableSpace;
use taffy::geometry::Size as TSize;
use taffy::style_helpers::TaffyMaxContent;
use taffy::{compute_root_layout, round_layout};

pub use context::StyleContext;
pub use style::{
    Align, Direction, Display, Justify, LayoutStyle, Position, Val, Wrap, auto, percent, px,
};

use tree::{LayoutTree, taffy_id};

pub mod prelude {
    pub use crate::{
        Align, Available, ComputedLayout, Constraints, Direction, Display, Justify, LayoutDone,
        LayoutModule, LayoutRoot, LayoutStyle, Measure, Position, StyleContext, Val, Viewport,
        Wrap, auto, percent, px,
    };
}

// ---------------------------------------------------------------------------
// Measure
// ---------------------------------------------------------------------------

/// The space offered on one axis when a leaf is measured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Available {
    /// This many pixels.
    Definite(f32),
    /// As little as possible: the leaf's smallest acceptable size.
    MinContent,
    /// As much as it wants: the leaf's natural size.
    MaxContent,
}

impl Available {
    fn from_taffy(a: AvailableSpace) -> Self {
        match a {
            AvailableSpace::Definite(v) => Available::Definite(v),
            AvailableSpace::MinContent => Available::MinContent,
            AvailableSpace::MaxContent => Available::MaxContent,
        }
    }
}

/// What a leaf is measured under: the dimensions already fixed by its
/// style or its parent, and the space available on each axis. The exact
/// contract taffy offers a leaf (`compute_leaf_layout` in
/// `taffy/src/compute/leaf.rs`), in our own types.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Constraints {
    pub known_width: Option<f32>,
    pub known_height: Option<f32>,
    pub available_width: Available,
    pub available_height: Available,
}

impl Constraints {
    pub(crate) fn from_taffy(known: TSize<Option<f32>>, available: TSize<AvailableSpace>) -> Self {
        Self {
            known_width: known.width,
            known_height: known.height,
            available_width: Available::from_taffy(available.width),
            available_height: Available::from_taffy(available.height),
        }
    }
}

/// A leaf's size, asked for under [`Constraints`]. Dense: every node has
/// one, unset by default, and only a leaf's is read. A known dimension is
/// one the style or the parent already fixed; return it on that axis.
///
/// The closure is `'static`, so it captures clones or `Arc`s, never
/// borrows. There is no `PartialEq`: every write is a change, which is
/// what a widget that rewrote its text wants. Taffy caches what it
/// measured, so the closure runs a few times per pass for a node whose
/// constraints it has not seen, and not at all for a cached one.
///
/// ```
/// use geometry::Size;
/// use layout::{Available, Constraints, Measure};
///
/// // A line of text 8 pixels per glyph, 12 high, that wraps at the width
/// // it is offered.
/// let text = String::from("hello world");
/// let measure = Measure::with(move |c: Constraints| {
///     let natural = text.len() as f32 * 8.0;
///     let width = match (c.known_width, c.available_width) {
///         (Some(w), _) | (None, Available::Definite(w)) => w.min(natural),
///         _ => natural,
///     };
///     let lines = (natural / width).ceil().max(1.0);
///     Size::new(width, lines * 12.0)
/// });
/// let wide = Constraints {
///     known_width: None,
///     known_height: None,
///     available_width: Available::MaxContent,
///     available_height: Available::MaxContent,
/// };
/// assert_eq!(measure.measure(wide), Size::new(88.0, 12.0));
/// let narrow = Constraints { available_width: Available::Definite(44.0), ..wide };
/// assert_eq!(measure.measure(narrow), Size::new(44.0, 24.0));
/// ```
#[derive(Default)]
pub struct Measure(Option<Box<dyn Fn(Constraints) -> Size>>);

impl Component for Measure {}

impl Measure {
    /// Unset: measures as zero.
    pub fn none() -> Self {
        Self(None)
    }

    /// Always `size`, whatever the constraints.
    pub fn fixed(size: Size) -> Self {
        Self::with(move |_| size)
    }

    pub fn with(f: impl Fn(Constraints) -> Size + 'static) -> Self {
        Self(Some(Box::new(f)))
    }

    pub fn is_set(&self) -> bool {
        self.0.is_some()
    }

    /// The leaf's size under `c`; `Size::ZERO` when unset.
    pub fn measure(&self, c: Constraints) -> Size {
        match &self.0 {
            Some(f) => f(c),
            None => Size::ZERO,
        }
    }
}

impl fmt::Debug for Measure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(_) => f.write_str("Measure(set)"),
            None => f.write_str("Measure(unset)"),
        }
    }
}

// ---------------------------------------------------------------------------
// LayoutRoot, ComputedLayout
// ---------------------------------------------------------------------------

/// Marks the top of one independently laid-out tree. Only the subtree
/// under a true root is laid out, with the root's box at the origin of
/// its own coordinates, sized by its own style. A node under no root keeps
/// its default [`ComputedLayout`]. A root's subtree must not contain another root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LayoutRoot(pub bool);

impl Component for LayoutRoot {}

/// The target box resolved for a node: rounded to whole pixels, in its root's
/// coordinates. `padding` and `border` are what the content box is inset
/// by. Written by the pass only, and only when it changed, so
/// `OnChanged<ComputedLayout>` names exactly the nodes whose target changed.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ComputedLayout {
    pub rect: Rect,
    pub padding: Insets<f32>,
    pub border: Insets<f32>,
}

impl Component for ComputedLayout {}

impl ComputedLayout {
    /// The rect inside padding and border, each dimension clamped at zero.
    pub fn content(&self) -> Rect {
        self.rect.inset(Insets::new(
            self.padding.top + self.border.top,
            self.padding.right + self.border.right,
            self.padding.bottom + self.border.bottom,
            self.padding.left + self.border.left,
        ))
    }
}

/// A scrolling window onto a node's descendants. They are drawn and hit
/// at their layout position minus `scroll`, accumulated through nested
/// viewports; the node itself is not shifted. With `clip`, descendants
/// are drawn and hit only inside this node's padding box (the rect inside
/// its border), intersected with any outer clip. Layout ignores it, so
/// writing it never relays out anything.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Viewport {
    pub clip: bool,
    pub scroll: Point,
}

impl Component for Viewport {}

// ---------------------------------------------------------------------------
// Private state
// ---------------------------------------------------------------------------

/// The pass's working state per node, touched only by the pass. `cache`
/// is what taffy measured this node at, per constraints; clearing it is
/// how a node is marked dirty, and it persists across ticks because a
/// cached subtree is not revisited. `unrounded` is the box before
/// rounding, which the rounding walk reads back for this node's own
/// offset from its parent. `abs` is the node's unrounded absolute
/// origin, the sum of those offsets down from the root: the rounding
/// walk sets it on the way down and rounds it once for the `ComputedLayout`,
/// which is what keeps rounding errors from accumulating. `pass` is the
/// number of the pass that last set `abs`, so the walk can check a
/// parent was visited before its child; both are only meaningful within
/// the pass that wrote them.
#[derive(Default)]
pub(crate) struct Scratch {
    pub(crate) cache: taffy::tree::Cache,
    pub(crate) unrounded: taffy::tree::Layout,
    pub(crate) abs: geometry::Point,
    pub(crate) pass: u32,
}

impl Component for Scratch {}

/// The roots with a dirty subtree, in the order they were marked.
/// Deduplicated on push by a linear scan: roots are windows, a handful.
#[derive(Default)]
pub(crate) struct DirtyRoots(pub(crate) Vec<NodeId>);

impl Resource for DirtyRoots {}

// ---------------------------------------------------------------------------
// Signal
// ---------------------------------------------------------------------------

/// Layout has run for this tick. Sent once per tick, whatever happened.
/// `roots` is the roots recomputed this tick, in the order they were
/// marked dirty; empty when none was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutDone {
    pub roots: Vec<NodeId>,
}

impl Signal for LayoutDone {}

impl LayoutDone {
    pub fn recomputed(&self) -> bool {
        !self.roots.is_empty()
    }
}

// ---------------------------------------------------------------------------
// The module
// ---------------------------------------------------------------------------

/// Registers the columns, resources and systems. The `PostTick`
/// system is registered *before* the components on purpose: each
/// `register_component` installs a `PostTick` drain for `OnChanged<C>`,
/// and systems for one signal run in registration order, so `pass` sees
/// the change records before the drains do and takes them. As a result
/// `OnChanged<LayoutStyle>`, `OnChanged<Measure>` and
/// `OnChanged<LayoutRoot>` never fire while this module is installed.
pub struct LayoutModule;

impl Module for LayoutModule {
    fn install(self, app: &mut App) {
        app.system(pass)
            .register_component::<LayoutStyle>()
            .register_component::<Measure>()
            .register_component::<LayoutRoot>()
            .register_component::<ComputedLayout>()
            .register_component::<Viewport>()
            .register_component::<Scratch>()
            .init_resource::<DirtyRoots>()
            .system(on_spawned)
            .system(on_removed);
    }
}

/// The `PostTick` system: take the change records, lay out the dirty
/// roots, signal `LayoutDone`.
fn pass(app: &mut App, _: &PostTick) {
    drain(app);
    let roots = take_dirty_roots(app);
    for &root in &roots {
        layout_root(app, root);
    }
    // The pass wrote `Scratch` through flagging guards, and `Scratch`'s
    // drain runs after this system; taking the record here means that
    // drain emits nothing. The same call covers `invalidate`'s writes.
    app.take_changed::<Scratch>().for_each(drop);
    app.signal(LayoutDone { roots });
}

/// One pass over `root`'s subtree. The root is offered max-content space
/// and sized by its own style. `round_layout` writes every target.
fn layout_root(app: &mut App, root: NodeId) {
    let (tree, mut data) = app.split();
    let (styles, measures, layouts, scratch) =
        data.query::<(&LayoutStyle, &Measure, &mut ComputedLayout, &mut Scratch)>();
    let mut view = LayoutTree::new(tree, root, styles, measures, layouts, scratch);
    let id = taffy_id(root);
    compute_root_layout(&mut view, id, TSize::MAX_CONTENT);
    round_layout(&mut view, id);
}

/// Take the three input records and invalidate every id in them. The
/// iterators borrow the app, so each is collected before invalidating.
fn drain(app: &mut App) {
    let styles: Vec<NodeId> = app.take_changed::<LayoutStyle>().collect();
    let measures: Vec<NodeId> = app.take_changed::<Measure>().collect();
    let roots: Vec<NodeId> = app.take_changed::<LayoutRoot>().collect();
    for id in styles.into_iter().chain(measures).chain(roots) {
        invalidate(app, id);
    }
}

/// Clear taffy's cache from `id` up through its ancestors, stopping after
/// the first node marked as a root, which is pushed onto `DirtyRoots`.
/// Reaching the app root without a mark pushes nothing: the node is
/// outside every root. A stale `id` does nothing. Idempotent, so a subtree
/// that signals children before parent walks the same path harmlessly.
fn invalidate(app: &mut App, id: NodeId) {
    let mut at = id;
    loop {
        if let Some(mut scratch) = app.component_mut::<Scratch>(at) {
            scratch.cache.clear();
        }
        if app.component::<LayoutRoot>(at).is_some_and(|m| m.0) {
            mark_dirty(app, at);
            return;
        }
        match app.parent(at) {
            Some(parent) if parent != at => at = parent,
            _ => return,
        }
    }
}

fn mark_dirty(app: &mut App, root: NodeId) {
    let mut dirty = app.resource_mut::<DirtyRoots>();
    if !dirty.0.contains(&root) {
        dirty.0.push(root);
    }
}

/// A new node: its own cache is empty already; what changed is its
/// parent's child list.
fn on_spawned(app: &mut App, s: &Spawned) {
    invalidate(app, s.parent);
}

/// A removed subtree: the parent's child list changed, if the parent is
/// still there. A stale parent was removed too, and its own `Removed`
/// does the work.
fn on_removed(app: &mut App, r: &Removed) {
    if app.is_live(r.parent) {
        invalidate(app, r.parent);
    }
}

/// The dirty roots that are still live and still marked, in marking
/// order, leaving the resource empty.
fn take_dirty_roots(app: &mut App) -> Vec<NodeId> {
    let taken = std::mem::take(&mut app.resource_mut::<DirtyRoots>().0);
    taken
        .into_iter()
        .filter(|&root| app.is_live(root) && app.component::<LayoutRoot>(root).is_some_and(|m| m.0))
        .collect()
}
