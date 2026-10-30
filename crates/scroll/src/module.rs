//! The systems that drive a `Scrollable` between contacts: the glide on
//! each frame, and the range after each layout.

use std::time::Instant;

use animation::Time;
use app::prelude::*;
use interactivity::prelude::*;
use layout::prelude::*;
use window::{Frame, InWindow, RequestFrame, Window};

use crate::physics::{Ballistic, Range, Tuning};
use crate::scrollable::{Axis, Scrollable, State, offset, set_offset};

/// The instant a glide is timed by. `None` reads `animation::Time`; a
/// test sets it to step the glide deterministically.
#[doc(hidden)]
#[derive(Debug, Default)]
pub struct ScrollClock {
    pub now: Option<Instant>,
}

impl Resource for ScrollClock {}

/// Inserts [`Tuning`] unless one is present, and [`ScrollClock`], and
/// attaches the systems. Installs after `InteractivityModule` and
/// `AnimationModule`, and before `RenderModule`, so a frame's glide is
/// written before render draws it.
pub struct ScrollModule;

impl Module for ScrollModule {
    fn install(self, app: &mut App) {
        assert!(
            app.has_resource::<Contacts>(),
            "ScrollModule installs after InteractivityModule"
        );
        assert!(
            app.has_resource::<Time>(),
            "ScrollModule installs after AnimationModule"
        );
        app.init_resource::<Tuning>()
            .init_resource::<ScrollClock>()
            .system(on_frame)
            .system(on_layout_done);
    }
}

fn now(app: &App) -> Instant {
    app.resource::<ScrollClock>()
        .now
        .unwrap_or_else(|| app.resource::<Time>().now())
}

fn window_of(app: &App, id: NodeId) -> Option<NodeId> {
    app.component::<InWindow>(id).and_then(|w| w.0)
}

fn release_grab(app: &mut App, id: NodeId) {
    *app.component_mut::<GrabPresses>(id).unwrap() = GrabPresses(false);
}

/// Advance every glide in the frame's window; one that has settled rests
/// exactly where its simulation ends, the rest ask for the next frame.
fn on_frame(app: &mut App, f: &Frame) {
    let gliding: Vec<NodeId> = app
        .widgets::<Scrollable>()
        .filter(|(_, s)| matches!(s.state, State::Ballistic { .. }))
        .map(|(id, _)| id)
        .collect();
    if gliding.is_empty() {
        return;
    }
    let now = now(app);
    for id in gliding {
        if window_of(app, id) != Some(f.0) {
            continue;
        }
        let Some(s) = app.widget_mut::<Scrollable>(id) else {
            continue;
        };
        let axis = s.axis;
        let State::Ballistic { sim, start } = &mut s.state else {
            continue;
        };
        let t = now
            .saturating_duration_since(*start.get_or_insert(now))
            .as_secs_f64();
        let (x, done) = (sim.x(t), sim.is_done(t));
        if done {
            s.state = State::Idle;
        }
        set_offset(app.component_mut::<Viewport>(id).unwrap(), axis, x);
        if done {
            release_grab(app, id);
        } else {
            app.signal(RequestFrame(f.0));
        }
    }
}

/// Take each `Scrollable`'s range from the layout just done. An idle one
/// is clamped into it; a glide is rebuilt against it from where it is,
/// and a drag reads it on its next move.
fn on_layout_done(app: &mut App, e: &LayoutDone) {
    let ids: Vec<NodeId> = app.widgets::<Scrollable>().map(|(id, _)| id).collect();
    for id in ids {
        let window = window_of(app, id);
        let axis = app.widget::<Scrollable>(id).unwrap().axis;
        if window.is_some_and(|w| e.roots.contains(&w))
            && let Some((range, viewport)) = measure(app, id, axis)
        {
            let s = app.widget_mut::<Scrollable>(id).unwrap();
            let changed = s.range != range;
            s.range = range;
            s.viewport = viewport;
            if changed {
                reaim(app, id, window);
            }
        }
        let s = app.widget::<Scrollable>(id).unwrap();
        if matches!(s.state, State::Idle) {
            let range = s.range;
            let viewport = app.component_mut::<Viewport>(id).unwrap();
            let clamped = offset(&viewport, axis).clamp(range.min, range.max);
            set_offset(viewport, axis, clamped);
        }
    }
}

/// A glide whose range changed carries on against the new one, from its
/// current position and velocity; if that leaves nothing to do it rests.
fn reaim(app: &mut App, id: NodeId, window: Option<NodeId>) {
    let tuning = app.resource::<Tuning>().clone();
    let now = now(app);
    let dpr = window
        .and_then(|w| app.widget::<Window>(w))
        .map_or(1.0, |w| f64::from(w.scale()));
    let s = app.widget_mut::<Scrollable>(id).unwrap();
    let State::Ballistic { sim, start } = &s.state else {
        return;
    };
    let t = start.map_or(0.0, |start| {
        now.saturating_duration_since(start).as_secs_f64()
    });
    let (x, v) = (sim.x(t), sim.dx(t));
    let v = if s.fits() { 0.0 } else { v };
    match Ballistic::new(&tuning, x, v, s.range, dpr) {
        Some(sim) => {
            s.state = State::Ballistic {
                sim,
                start: Some(now),
            }
        }
        None => {
            s.state = State::Idle;
            let axis = s.axis;
            set_offset(app.component_mut::<Viewport>(id).unwrap(), axis, x);
            release_grab(app, id);
        }
    }
}

/// The range and the viewport length from `id`'s layout. The extent runs
/// from the padding box's start to the furthest end of a direct child,
/// plus the end padding; the viewport is the padding box.
fn measure(app: &App, id: NodeId, axis: Axis) -> Option<(Range, f64)> {
    let me = app.component::<ComputedLayout>(id)?;
    let (start, length, pad, border) = match axis {
        Axis::Vertical => (
            me.rect.y(),
            me.rect.height(),
            (me.padding.top, me.padding.bottom),
            (me.border.top, me.border.bottom),
        ),
        Axis::Horizontal => (
            me.rect.x(),
            me.rect.width(),
            (me.padding.left, me.padding.right),
            (me.border.left, me.border.right),
        ),
    };
    let content_start = start + border.0 + pad.0;
    let end = app
        .children(id)?
        .iter()
        .filter_map(|&c| app.component::<ComputedLayout>(c))
        .map(|c| match axis {
            Axis::Vertical => c.rect.bottom(),
            Axis::Horizontal => c.rect.right(),
        })
        .fold(content_start, f32::max);
    let extent = f64::from(end - content_start + pad.0 + pad.1);
    let viewport = f64::from((length - border.0 - border.1).max(0.0));
    Some((Range::new(extent, viewport), viewport))
}
