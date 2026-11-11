//! The renderer-facing geometry and its transitions from Taffy's resolved target.

use std::time::Instant;

use app::prelude::*;
use app::{Emitted, Spawned};
use geometry::{Insets, Rect};
use layout::{ComputedLayout, LayoutDone, LayoutRoot};
use window::{Frame, InWindow, RequestFrame};

use crate::{
    AnimationTime, LayoutAnimationSettings, Time,
    animatable::{Animatable, Running},
    settings,
};

/// The displayed box in its layout root's coordinates. Initialized from
/// [`ComputedLayout`] on first resolution, then maintained by the animation
/// module. Renderers read this box; frame-time values may be fractional pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Layout {
    pub rect: Rect,
    pub padding: Insets<f32>,
    pub border: Insets<f32>,
}

impl Component for Layout {}

impl From<ComputedLayout> for Layout {
    fn from(value: ComputedLayout) -> Self {
        Self {
            rect: value.rect,
            padding: value.padding,
            border: value.border,
        }
    }
}

impl Layout {
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

impl Animatable for Layout {
    fn max_delta(self, target: Self) -> f64 {
        [
            (self.rect.x(), target.rect.x()),
            (self.rect.y(), target.rect.y()),
            (self.rect.width(), target.rect.width()),
            (self.rect.height(), target.rect.height()),
            (self.padding.top, target.padding.top),
            (self.padding.right, target.padding.right),
            (self.padding.bottom, target.padding.bottom),
            (self.padding.left, target.padding.left),
            (self.border.top, target.border.top),
            (self.border.right, target.border.right),
            (self.border.bottom, target.border.bottom),
            (self.border.left, target.border.left),
        ]
        .into_iter()
        .map(|(from, to)| (f64::from(to) - f64::from(from)).abs())
        .fold(0.0, f64::max)
    }

    /// Blend every numeric layout field using the already-eased amount.
    /// This does not rerun Taffy or enforce intermediate layout constraints.
    /// The amount is not clamped and the result is not rounded here.
    fn interpolate(self, target: Self, amount: f32) -> Self {
        let lerp = |a: f32, b: f32| a + (b - a) * amount;
        let insets = |a: Insets<f32>, b: Insets<f32>| {
            Insets::new(
                lerp(a.top, b.top),
                lerp(a.right, b.right),
                lerp(a.bottom, b.bottom),
                lerp(a.left, b.left),
            )
        };
        Self {
            rect: Rect::new(
                lerp(self.rect.x(), target.rect.x()),
                lerp(self.rect.y(), target.rect.y()),
                lerp(self.rect.width(), target.rect.width()),
                lerp(self.rect.height(), target.rect.height()),
            ),
            padding: insets(self.padding, target.padding),
            border: insets(self.border, target.border),
        }
    }
}

/// Per-node runtime state. A node is unresolved until its layout root's pass
/// first visits it, even when that pass produces the default zero-sized box.
/// Running data is absent when idle and resets when the node is removed.
#[derive(Default)]
pub(crate) struct Transition {
    resolved: bool,
    running: Option<Running<Layout>>,
}
impl Component for Transition {}

/// Nodes spawned but not yet visited by a layout pass. A zero-sized first
/// resolution produces no `ComputedLayout` change event, so these are found
/// by membership in the roots a pass recomputed instead.
#[derive(Default)]
pub(crate) struct Unresolved(pub(crate) Vec<NodeId>);
impl Resource for Unresolved {}

/// Nodes with a running layout transition: the ones whose windows still need
/// frames. Stale and finished entries are dropped on the next `LayoutDone`.
#[derive(Default)]
pub(crate) struct Active(Vec<NodeId>);
impl Resource for Active {}

/// Every new node starts unresolved.
pub(crate) fn on_spawned(app: &mut App, spawned: &Spawned) {
    app.resource_mut::<Unresolved>().0.push(spawned.id);
}

/// A moved subtree starts over too: its old place was in other
/// coordinates, maybe another window, so it takes its new place at once
/// rather than animating there.
pub(crate) fn on_moved(app: &mut App, m: &Moved) {
    let subtree: Vec<NodeId> = std::iter::once(m.id).chain(app.descendants(m.id)).collect();
    for &node in &subtree {
        if let Some(mut transition) = app.component_mut::<Transition>(node) {
            *transition = Transition::default();
        }
    }
    app.resource_mut::<Unresolved>().0.extend(subtree);
}

/// Copy first resolutions and keep running transitions drawing; do not
/// advance time here.
///
/// `LayoutDone` is queued by layout's `PostTick` system once per tick,
/// whatever happened. Only two kinds of node need anything from it:
/// - Unresolved nodes inside a root this pass recomputed copy their target,
///   even when it stayed zero, and request a frame if that changed `Layout`.
/// - Nodes with a running transition request a frame for their window, so an
///   animation never waits on some other wake-up to reach its next frame.
///
/// Target changes are reconciled by [`on_target_changed`], node by node.
/// Nothing else is visited, so a tick where no layout changed costs one
/// check per running transition.
pub(crate) fn on_layout_done(app: &mut App, done: &LayoutDone) {
    let mut windows = Vec::new();
    if done.recomputed() {
        let pending = std::mem::take(&mut app.resource_mut::<Unresolved>().0);
        let mut still = Vec::new();
        for id in pending {
            if !app.is_live(id) {
                continue;
            }
            let visited = std::iter::once(id)
                .chain(app.ancestors(id))
                .any(|node| done.roots.contains(&node));
            if !visited {
                still.push(id);
                continue;
            }
            let target = Layout::from(*app.component::<ComputedLayout>(id).unwrap());
            app.component_mut::<Transition>(id).unwrap().resolved = true;
            let changed = app.component_mut::<Layout>(id).unwrap().set_if_neq(target);
            if changed && let Some(window) = app.component::<InWindow>(id).unwrap().0 {
                push_unique(&mut windows, window);
            }
        }
        app.resource_mut::<Unresolved>().0.extend(still);
    }

    let active = std::mem::take(&mut app.resource_mut::<Active>().0);
    let mut still = Vec::new();
    for id in active {
        if !app.is_live(id) || app.component::<Transition>(id).unwrap().running.is_none() {
            continue;
        }
        if let Some(window) = app.component::<InWindow>(id).unwrap().0 {
            push_unique(&mut windows, window);
        }
        still.push(id);
    }
    app.resource_mut::<Active>().0 = still;

    finish(app, windows);
}

/// Reconcile the nodes whose target changed with their displayed layouts.
///
/// For each resolved node:
/// - Copies the target and cancels a transition when animation is disabled,
///   there is no window, or the node is a layout root.
/// - Keeps a running transition if its destination has not changed.
/// - Otherwise starts or replaces a transition from the currently displayed
///   layout, or leaves the node idle if it already equals its target.
///
/// Settings are read here, when a target changes: a new duration, easing or
/// a zero duration takes effect from the next move, and a running transition
/// keeps the settings it started with.
pub(crate) fn on_target_changed(app: &mut App, changed: &Emitted<OnChanged<ComputedLayout>>) {
    let now = app.resource::<Time>().now();
    let mut windows = Vec::new();
    for &id in changed.targets.iter() {
        if !app.is_live(id) || !app.component::<Transition>(id).unwrap().resolved {
            continue;
        }
        let target = Layout::from(*app.component::<ComputedLayout>(id).unwrap());
        let window = app.component::<InWindow>(id).unwrap().0;
        let configuration = settings::<LayoutAnimationSettings>(app, id).filter(|s| {
            !matches!(s.time, AnimationTime::Duration(d) if d.is_zero())
                && window.is_some()
                && !app.component::<LayoutRoot>(id).unwrap().0
        });
        let Some(configuration) = configuration else {
            let changed = app.component_mut::<Layout>(id).unwrap().set_if_neq(target);
            app.component_mut::<Transition>(id).unwrap().running = None;
            if changed && let Some(window) = window {
                push_unique(&mut windows, window);
            }
            continue;
        };
        let current = *app.component::<Layout>(id).unwrap();
        let window = window.unwrap();
        let running = {
            let mut transition = app.component_mut::<Transition>(id).unwrap();
            if transition
                .running
                .as_ref()
                .is_some_and(|t| t.target == target)
            {
                // Keep the original start and clock.
            } else if current != target {
                transition.running = Some(Running::new(current, target, now, configuration));
            } else {
                transition.running = None;
            }
            transition.running.is_some()
        };
        if running {
            push_unique(&mut app.resource_mut::<Active>().0, id);
            push_unique(&mut windows, window);
        }
    }
    finish(app, windows);
}

/// Drop `Transition`'s change records, announce `Layout` writes made after
/// the normal drain, and request the frames.
fn finish(app: &mut App, windows: Vec<NodeId>) {
    // Transition is private bookkeeping, not a consumer-facing change event.
    app.take_changed::<Transition>().for_each(drop);

    // The normal PostTick drain already ran. Notify consumers about displayed
    // Layout writes made here now, and clear their records so they don't fire
    // again on the next tick.
    let changed: Vec<NodeId> = app.take_changed::<Layout>().collect();
    if !changed.is_empty() {
        app.emit(OnChanged::<Layout>::new(), changed);
    }
    for window in windows {
        app.signal(RequestFrame(window));
    }
}

fn push_unique(list: &mut Vec<NodeId>, id: NodeId) {
    if !list.contains(&id) {
        list.push(id);
    }
}

/// Advance existing transitions for the window whose drawing opportunity arrived.
/// The clock's `Frame` system has already sampled `Time`, and render runs after
/// this system. It neither discovers new targets nor requests another frame;
/// those jobs belong to `on_layout_done`. A frame is not a layout pass.
pub(crate) fn on_frame(app: &mut App, frame: &Frame) {
    advance(app, frame.0, app.resource::<Time>().now());
}

/// Sample one window's active transitions at `now`, writing only displayed layout.
/// Each transition uses elapsed time since its own start, not global `Time::delta`,
/// which may include intervening ticks or other windows' frames. Completion writes
/// the exact target and clears the running state. Other windows are untouched.
/// The explicit timestamp also lets tests sample progress without sleeping.
fn advance(app: &mut App, window: NodeId, now: Instant) {
    let (membership, mut layouts, mut transitions) =
        app.query::<(&InWindow, &mut Layout, &mut Transition)>();
    for (id, mut transition) in transitions.iter_mut() {
        if membership[id].0 != Some(window) {
            continue;
        }
        let Some(running) = &transition.running else {
            continue; // skip everything that doesn't have a currently running animation
        };
        let (value, finished) = running.advance(now);
        layouts.get_mut(id).unwrap().set_if_neq(value); // set the layout here
        if finished {
            transition.running = None;
        }
    }
    drop((membership, layouts, transitions));
    app.take_changed::<Transition>().for_each(drop);
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
