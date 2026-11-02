//! The signal into `interactivity`.

use app::{NodeId, Signal};
use geometry::Point;

/// One source of pointer-like input: the mouse pointer, or one live
/// finger on a touchscreen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContactId {
    Mouse,
    /// `wl_touch`'s id for one finger; reused after that touch ends.
    Touch(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactPhase {
    /// A new position. Also how a contact is first seen: the mouse
    /// pointer's own first `Moved` is its `wl_pointer.enter`; a touch's
    /// first `Moved` carries the position its `wl_touch.down` reported,
    /// sent just before that same input's `Pressed`.
    Moved,
    /// The contact's button or finger went down at its last position.
    Pressed,
    /// It went up.
    Released,
    /// It stopped existing without a normal `Released`: `wl_pointer.leave`,
    /// `wl_touch.cancel`.
    Cancelled,
}

/// One report of pointer-like input, window-local: `presentation`'s
/// reduction of `wl_pointer` and `wl_touch` to a single shape, and the
/// only signal this crate consumes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContactInput {
    pub window: NodeId,
    pub contact: ContactId,
    pub phase: ContactPhase,
    pub position: Point,
    /// The Wayland event's millisecond timestamp, on a clock of the
    /// compositor's choosing; it wraps at `u32::MAX`, so compare with
    /// `wrapping_sub`. Events with no timestamp of their own carry the
    /// last one seen.
    pub time: u32,
}

impl Signal for ContactInput {}
