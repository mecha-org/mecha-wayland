//! Taking a contact over: claiming one mid-press, or grabbing presses
//! before they reach the nodes beneath.

use app::prelude::*;

use geometry::Point;

use crate::contact::ContactId;
use crate::contacts::{Contacts, HitSet};
use crate::events::{Exit, PressCancelled};

/// A node with `GrabPresses(true)` takes every press over it before the
/// nodes beneath see it: the press goes to it alone, already claimed.
/// With several in one hit-set, the outermost (nearest the window) wins.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GrabPresses(pub bool);

impl Component for GrabPresses {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContactClaimed {
    pub contact: ContactId,
    pub by: NodeId,
}

impl Signal for ContactClaimed {}

/// Claiming a contact from a handler.
pub trait ContextClaim {
    /// Make the handler's target the only node in `contact`'s capture:
    /// it alone gets the rest of the press's `PressMoved`, `Release` and
    /// `Clicked`, and the contact stops hovering until the press ends.
    /// The rest of the old capture gets [`PressCancelled`] and the rest
    /// of the old hit-set [`Exit`], both queued here.
    ///
    /// Takes effect inside the call, so a `Released` already queued
    /// behind the signal being handled goes to the claimer alone.
    /// Returns `false`, doing nothing, if `contact` has no capture or the
    /// target is not in it.
    ///
    /// A claim made from a `PressMoved` handler does not shorten that
    /// event's own target list: nodes dispatched after the claimer still
    /// get that one `PressMoved`.
    fn claim_contact(&mut self, contact: ContactId) -> bool;

    /// [`ContextClaim::claim_contact`] without claiming: the target alone
    /// keeps the press, but no [`ContactClaimed`] is sent.
    fn hold_contact(&mut self, contact: ContactId) -> bool;
}

impl<W: Widget> ContextClaim for Context<'_, W> {
    fn claim_contact(&mut self, contact: ContactId) -> bool {
        let target = self.target();
        let already = self.resource::<Contacts>().claimed(contact) == Some(target);
        let taken = self.resource_mut::<Contacts>().claim(contact, target);
        if !cancel_others(self, contact, taken) {
            return false;
        }
        // Claiming again what it already holds changes nothing to announce.
        if !already {
            self.signal(ContactClaimed {
                contact,
                by: target,
            });
        }
        true
    }

    fn hold_contact(&mut self, contact: ContactId) -> bool {
        let target = self.target();
        let taken = self.resource_mut::<Contacts>().hold(contact, target);
        cancel_others(self, contact, taken)
    }
}

/// Tells the nodes a claim or hold took `contact` from; `false` if none was made.
fn cancel_others<W: Widget>(
    ctx: &mut Context<'_, W>,
    contact: ContactId,
    taken: Option<(HitSet, HitSet, Point)>,
) -> bool {
    let Some((cancelled, exited, position)) = taken else {
        return false;
    };
    if !cancelled.is_empty() {
        ctx.emit(PressCancelled { contact, position }, &cancelled[..]);
    }
    if !exited.is_empty() {
        ctx.emit(Exit { contact, position }, &exited[..]);
    }
    true
}
