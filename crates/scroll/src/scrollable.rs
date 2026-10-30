//! `Scrollable`: a box whose children scroll along one axis under a
//! finger. `build` never spawns children; a caller attaches them to the
//! handle it gets back, as with a `Div`.

use std::time::Instant;

use app::prelude::*;
use geometry::Point;
use interactivity::prelude::*;
use layout::prelude::*;
use window::{InWindow, RequestFrame, Window};

use crate::physics::{Ballistic, Range, Tuning, drag};
use crate::velocity::VelocityTracker;

/// The one direction a [`Scrollable`] moves its content in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Axis {
    #[default]
    Vertical,
    Horizontal,
}

impl Axis {
    /// `p`'s coordinate along this axis.
    pub(crate) fn of(self, p: Point) -> f64 {
        f64::from(match self {
            Axis::Vertical => p.y,
            Axis::Horizontal => p.x,
        })
    }

    /// `p` with its coordinate along this axis replaced by `v`.
    fn with(self, p: Point, v: f64) -> Point {
        match self {
            Axis::Vertical => Point::new(p.x, v as f32),
            Axis::Horizontal => Point::new(v as f32, p.y),
        }
    }
}

/// A vertical `Scrollable` with the default style for its axis.
pub fn scrollable() -> ScrollableBuilder {
    ScrollableBuilder {
        style: None,
        axis: Axis::Vertical,
    }
}

pub struct ScrollableBuilder {
    /// `None` until set: the default depends on the axis.
    style: Option<LayoutStyle>,
    axis: Axis,
}

impl ScrollableBuilder {
    /// The whole style, replacing the axis's default (see the crate's
    /// `# Model`). Children must keep their natural length along the
    /// axis, so a replacement for a horizontal list stays a non-wrapping
    /// row whose children set `shrink(0.0)`.
    pub fn style(mut self, style: LayoutStyle) -> Self {
        self.style = Some(style);
        self
    }

    pub fn axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }
}

impl Build for ScrollableBuilder {
    type Widget = Scrollable;
}

/// A box that scrolls its children through its padding box. State is
/// private; there is no API to read or set the offset.
pub struct Scrollable {
    pub(crate) axis: Axis,
    pub(crate) state: State,
    /// The driving contact's recent positions, for the release speed.
    tracker: VelocityTracker,
    /// The offsets the content rests in, as of the last layout.
    pub(crate) range: Range,
    /// The padding box's length along the axis, as of the last layout.
    pub(crate) viewport: f64,
}

/// What the scroll is doing. Offsets and positions are along the axis.
#[derive(Debug, Clone)]
pub(crate) enum State {
    Idle,
    /// Contacts are down and none has yet moved past the slop; the first
    /// that does drives.
    Tracking {
        pending: Vec<Pending>,
        start_offset: f64,
    },
    /// The contact claimed and moves the content.
    Dragging {
        contact: ContactId,
        last: f64,
    },
    /// Released and moving on its own; `start` is set on the first frame.
    Ballistic {
        sim: Ballistic,
        start: Option<Instant>,
    },
}

/// A contact pressed on the `Scrollable` while tracking.
#[derive(Debug, Clone)]
pub(crate) struct Pending {
    contact: ContactId,
    origin: f64,
    tracker: VelocityTracker,
}

impl Pending {
    fn new(e: &Press, origin: f64) -> Self {
        let mut tracker = VelocityTracker::default();
        tracker.add(e.time, origin);
        Pending {
            contact: e.contact,
            origin,
            tracker,
        }
    }
}

impl Widget for Scrollable {
    type Builder = ScrollableBuilder;

    fn build(b: ScrollableBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        let style = b.style.unwrap_or_else(|| match b.axis {
            // Block layout keeps children at their natural height.
            Axis::Vertical => LayoutStyle::default().block(),
            Axis::Horizontal => LayoutStyle::default().row(),
        });
        *s.component_mut::<LayoutStyle>(me).unwrap() = style;
        *s.component_mut::<Viewport>(me).unwrap() = Viewport {
            clip: true,
            scroll: Point::ZERO,
        };
        s.on::<Press>(me, on_press)
            .on::<PressMoved>(me, on_press_moved)
            .on::<Release>(me, |ctx, e| finish(ctx, e.contact, Some(e.time)))
            .on::<PressCancelled>(me, |ctx, e| finish(ctx, e.contact, None));
        Scrollable {
            axis: b.axis,
            state: State::Idle,
            tracker: VelocityTracker::default(),
            range: Range::new(0.0, 0.0),
            viewport: 0.0,
        }
    }
}

impl Scrollable {
    /// Whether the content fits, and so neither moves nor bounces.
    pub(crate) fn fits(&self) -> bool {
        self.range.max <= self.range.min
    }
}

/// The offset `viewport` holds along `axis`.
pub(crate) fn offset(viewport: &Viewport, axis: Axis) -> f64 {
    axis.of(viewport.scroll)
}

/// Write `offset` along `axis`, flagging the component only if it moved.
pub(crate) fn set_offset(mut viewport: CompMut<'_, Viewport>, axis: Axis, offset: f64) {
    let scroll = axis.with(viewport.scroll, offset);
    if viewport.scroll != scroll {
        viewport.scroll = scroll;
    }
}

fn current_offset(ctx: &mut Context<'_, Scrollable>) -> f64 {
    let axis = ctx.me().axis;
    offset(ctx.component::<Viewport>().unwrap(), axis)
}

fn set_grab(ctx: &mut Context<'_, Scrollable>, grab: bool) {
    *ctx.component_mut::<GrabPresses>().unwrap() = GrabPresses(grab);
}

/// Idle or tracking: track the contact too. Gliding: the press was
/// grabbed, so it stops the glide and drags from where it is, with no tap
/// underneath.
fn on_press(ctx: &mut Context<'_, Scrollable>, e: &Press) {
    let offset = current_offset(ctx);
    let me = ctx.me();
    let position = me.axis.of(e.position);
    match &mut me.state {
        State::Idle => {
            me.state = State::Tracking {
                pending: vec![Pending::new(e, position)],
                start_offset: offset,
            }
        }
        State::Tracking { pending, .. } => {
            if pending.iter().all(|p| p.contact != e.contact) {
                pending.push(Pending::new(e, position));
            }
        }
        State::Ballistic { .. } => {
            me.state = State::Dragging {
                contact: e.contact,
                last: position,
            };
            me.tracker.reset();
            me.tracker.add(e.time, position);
        }
        // Another contact drives.
        State::Dragging { .. } => {}
    }
}

/// Past the slop a tracked contact becomes the drag, and every tracked
/// contact is claimed; a drag moves the content.
fn on_press_moved(ctx: &mut Context<'_, Scrollable>, e: &PressMoved) {
    let tuning = ctx.resource::<Tuning>().clone();
    let offset = current_offset(ctx);
    let me = ctx.me();
    let position = me.axis.of(e.position);
    match &mut me.state {
        State::Tracking {
            pending,
            start_offset,
        } => {
            let Some(i) = pending.iter().position(|p| p.contact == e.contact) else {
                return;
            };
            let p = &mut pending[i];
            p.tracker.add(e.time, position);
            if (position - p.origin).abs() <= tuning.slop {
                return;
            }
            // The drag starts from the crossing point: no jump by the slop.
            let start_offset = *start_offset;
            let pending = std::mem::take(pending);
            me.tracker = pending[i].tracker.clone();
            me.state = State::Dragging {
                contact: e.contact,
                last: position,
            };
            let axis = me.axis;
            // The rows under every finger get `PressCancelled`; the others'
            // moves and releases then reach only this, and are ignored.
            for p in &pending {
                ctx.claim_contact(p.contact);
            }
            set_grab(ctx, true);
            set_offset(ctx.component_mut::<Viewport>().unwrap(), axis, start_offset);
        }
        State::Dragging { contact, last } if *contact == e.contact => {
            // The finger moving down moves the offset toward the start.
            let delta = -(position - *last);
            *last = position;
            me.tracker.add(e.time, position);
            if me.fits() {
                return;
            }
            let to = drag(&tuning, offset, delta, me.range, me.viewport);
            let axis = me.axis;
            set_offset(ctx.component_mut::<Viewport>().unwrap(), axis, to);
        }
        _ => {}
    }
}

/// A contact went up (`time`) or was cancelled (`None`, as a release
/// that does not fling). Tracking ends with the last tracked contact; the
/// driving one ends a drag, which hands over to the ballistic.
fn finish(ctx: &mut Context<'_, Scrollable>, contact: ContactId, time: Option<u32>) {
    let tuning = ctx.resource::<Tuning>().clone();
    let offset = current_offset(ctx);
    let window = ctx.component::<InWindow>().and_then(|w| w.0);
    let dpr = window
        .and_then(|w| ctx.widget::<Window>(w))
        .map_or(1.0, |w| f64::from(w.scale()));
    let me = ctx.me();
    match &mut me.state {
        State::Tracking { pending, .. } => {
            pending.retain(|p| p.contact != contact);
            if pending.is_empty() {
                me.state = State::Idle;
            }
        }
        State::Dragging { contact: c, .. } if *c == contact => {
            let velocity = match time {
                Some(time) if !me.fits() => {
                    // The tracker follows the finger; the offset moves against it.
                    let v = (-me.tracker.velocity(time)).clamp(-tuning.max_fling, tuning.max_fling);
                    if v.abs() < tuning.min_fling { 0.0 } else { v }
                }
                _ => 0.0,
            };
            match Ballistic::new(&tuning, offset, velocity, me.range, dpr) {
                Some(sim) => {
                    me.state = State::Ballistic { sim, start: None };
                    if let Some(w) = window {
                        ctx.signal(RequestFrame(w));
                    }
                }
                None => {
                    me.state = State::Idle;
                    set_grab(ctx, false);
                }
            }
        }
        _ => {}
    }
}
