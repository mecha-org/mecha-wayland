//! Per-contact state. Everything but the two reads on `Contacts` is
//! `pub(crate)`: the four systems in `module.rs` and
//! `ContextClaim::claim_contact` in `claim.rs` are its only writers.

use std::collections::HashMap;

use app::{NodeId, Resource};
use geometry::Point;
use smallvec::SmallVec;

use crate::contact::ContactId;

pub(crate) type HitSet = SmallVec<[NodeId; 8]>;

/// One live contact: where it is, what is under it right now (`hit`,
/// fresh every `Moved`), and — only while a button or finger is down —
/// the set `Press`/`Release`/`Clicked` stay locked onto (`captured`),
/// and the one node of it that took the contact over, if any (`claimed`).
#[derive(Debug)]
pub(crate) struct ContactState {
    /// The window this contact's input names. Recorded on every
    /// `entry()` call for a future consumer (e.g. debug/introspection);
    /// nothing in `src/` reads it back today, since `hit_test` is always
    /// called with the window straight off the incoming `ContactInput`.
    pub(crate) window: NodeId,
    pub(crate) position: Point,
    pub(crate) hit: HitSet,
    pub(crate) captured: Option<HitSet>,
    /// The node that claimed this contact, by `claim_contact` or by
    /// grabbing its press; `captured` is then exactly `[claimed]`. Ends
    /// with the capture.
    pub(crate) claimed: Option<NodeId>,
}

/// Every live contact — the mouse pointer plus each active touch — keyed
/// by [`ContactId`]. The two reads here are its only public surface.
#[derive(Debug, Default)]
pub struct Contacts {
    live: HashMap<ContactId, ContactState>,
}

impl Resource for Contacts {}

impl Contacts {
    /// Whether `contact` has a button or finger down right now.
    pub fn is_pressed(&self, contact: ContactId) -> bool {
        self.live
            .get(&contact)
            .is_some_and(|s| s.captured.is_some())
    }

    /// `contact`'s last known position, or `None` if it has never been
    /// seen, or has since been torn down.
    pub fn position(&self, contact: ContactId) -> Option<Point> {
        self.live.get(&contact).map(|s| s.position)
    }

    /// `contact`'s state, creating it at `window` with a zero position, an
    /// empty hit-set and no capture on first sight. `window` is written
    /// every call, not only on creation.
    pub(crate) fn entry(&mut self, contact: ContactId, window: NodeId) -> &mut ContactState {
        let state = self.live.entry(contact).or_insert_with(|| ContactState {
            window,
            position: Point::ZERO,
            hit: HitSet::new(),
            captured: None,
            claimed: None,
        });
        state.window = window;
        state
    }

    /// `contact`'s state if it has been seen before, without creating it.
    pub(crate) fn get_mut(&mut self, contact: ContactId) -> Option<&mut ContactState> {
        self.live.get_mut(&contact)
    }

    /// Hands `contact`'s capture to `node` alone: `captured` becomes
    /// `[node]`, `claimed` becomes `node`, and `hit` keeps only `node`.
    /// Returns what `node` took the contact from — the old captured set
    /// and the old hit-set, each without `node` — and the contact's
    /// position. `None`, changing nothing, if `contact` has no capture or
    /// `node` is not in it.
    pub(crate) fn claim(
        &mut self,
        contact: ContactId,
        node: NodeId,
    ) -> Option<(HitSet, HitSet, Point)> {
        let state = self.live.get_mut(&contact)?;
        let captured = state.captured.as_mut().filter(|c| c.contains(&node))?;
        let cancelled = captured.iter().copied().filter(|&id| id != node).collect();
        *captured = HitSet::from_slice(&[node]);
        let exited = state.hit.iter().copied().filter(|&id| id != node).collect();
        state.hit.retain(|id| *id == node);
        state.claimed = Some(node);
        Some((cancelled, exited, state.position))
    }

    /// Removes and returns `contact`'s state, if any.
    pub(crate) fn take(&mut self, contact: ContactId) -> Option<ContactState> {
        self.live.remove(&contact)
    }
}

#[cfg(test)]
mod tests {
    use app::App;

    use super::*;

    /// A live `NodeId` with no tree of its own needed: any `App`'s root.
    fn some_node() -> NodeId {
        App::new().root()
    }

    #[test]
    fn entry_creates_on_first_sight_and_keeps_state_on_the_next() {
        let mut c = Contacts::default();
        let w = some_node();
        c.entry(ContactId::Mouse, w).position = Point::new(5.0, 6.0);
        assert_eq!(c.position(ContactId::Mouse), Some(Point::new(5.0, 6.0)));
        assert!(!c.is_pressed(ContactId::Mouse));

        c.entry(ContactId::Mouse, w).captured = Some(HitSet::new());
        assert!(c.is_pressed(ContactId::Mouse));
        assert_eq!(
            c.position(ContactId::Mouse),
            Some(Point::new(5.0, 6.0)),
            "the second entry() call kept the state"
        );
    }

    #[test]
    fn get_mut_and_take() {
        let mut c = Contacts::default();
        let w = some_node();
        assert!(c.get_mut(ContactId::Mouse).is_none());
        c.entry(ContactId::Mouse, w);
        assert!(c.get_mut(ContactId::Mouse).is_some());
        assert!(c.take(ContactId::Mouse).is_some());
        assert!(c.get_mut(ContactId::Mouse).is_none());
        assert!(c.take(ContactId::Mouse).is_none(), "already gone");
    }

    #[test]
    fn reads_are_none_for_a_contact_never_seen() {
        let c = Contacts::default();
        assert_eq!(c.position(ContactId::Touch(3)), None);
        assert!(!c.is_pressed(ContactId::Touch(3)));
    }
}
