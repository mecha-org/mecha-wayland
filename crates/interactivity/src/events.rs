//! Node-level events dispatched from a `ContactInput`. All five carry the
//! same two fields; see the crate docs for which nodes receive them and
//! in what order.

use app::Event;
use geometry::Point;

use crate::ContactId;
use crate::keyboard::{KeyMeaning, Modifiers};

/// A contact went down over this node: every node in the fresh hit-set at
/// the `Pressed` position, deepest-first, window last. Locks in ("captures")
/// that set for `Release`/`Clicked` to route to later.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Press {
    pub contact: ContactId,
    pub position: Point,
    /// The originating `ContactInput`'s timestamp, in milliseconds.
    pub time: u32,
}
/// A contact that pressed this node has now gone up. Reaches the set
/// captured at `Press`, not a fresh hit test — the contact may have moved
/// off every one of these nodes in between.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Release {
    pub contact: ContactId,
    pub position: Point,
    /// The originating `ContactInput`'s timestamp, in milliseconds.
    pub time: u32,
}
/// A contact's fresh hit-set now includes this node, where it did not on
/// the previous `Moved`. Independent of any capture in progress.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Enter {
    pub contact: ContactId,
    pub position: Point,
}
/// A contact's fresh hit-set no longer includes this node, where it did
/// on the previous `Moved`. Independent of any capture in progress; also
/// fired, for the captured set, when a `Cancelled` or a `Touch`'s
/// `Released` tears down that contact's state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Exit {
    pub contact: ContactId,
    pub position: Point,
}
/// A contact that pressed this node has now gone up over it, in the usual
/// sense of "clicked". Fires alongside `Release`, on the same captured
/// set. Unconditional: there is no check that the contact is still
/// within this set's bounds at release time, so `Clicked` fires even if
/// the contact moved off every one of these nodes before releasing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clicked {
    pub contact: ContactId,
    pub position: Point,
}

impl Event for Press {}
impl Event for Release {}
impl Event for Enter {}
impl Event for Exit {}
impl Event for Clicked {}

// ── keyboard ──────────────────────────────────────────────────────────────

/// A key went down on the node holding keyboard focus. Carries the key's
/// decoded meaning and the modifiers in effect. The event analog of
/// [`Press`]: one per `KeyboardInput` whose state was `Pressed`,
/// dispatched at the [`crate::KeyboardFocus`] node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPress {
    pub meaning: KeyMeaning,
    pub modifiers: Modifiers,
}
/// A held key repeated on the node holding keyboard focus — the event
/// analog of a repeated press. Carries the same fields as [`KeyPress`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRepeat {
    pub meaning: KeyMeaning,
    pub modifiers: Modifiers,
}
/// A key went up on the node holding keyboard focus. Carries the same
/// fields as [`KeyPress`]. The event analog of [`Release`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRelease {
    pub meaning: KeyMeaning,
    pub modifiers: Modifiers,
}

impl Event for KeyPress {}
impl Event for KeyRepeat {}
impl Event for KeyRelease {}
