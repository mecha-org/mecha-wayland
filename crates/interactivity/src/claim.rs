//! Taking a contact over: claiming one mid-press, or grabbing presses
//! before they reach the nodes beneath.

use app::prelude::*;

use crate::contact::ContactId;
use crate::contacts::Contacts;
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

    /// End `contact`'s press for its whole capture (`PressCancelled`, no
    /// `Clicked`) without claiming it. `false` if the target is not in it.
    fn cancel_contact(&mut self, contact: ContactId) -> bool;
}

impl<W: Widget> ContextClaim for Context<'_, W> {
    fn claim_contact(&mut self, contact: ContactId) -> bool {
        let target = self.target();
        let already = self.resource::<Contacts>().claimed(contact) == Some(target);
        let Some((cancelled, exited, position)) =
            self.resource_mut::<Contacts>().claim(contact, target)
        else {
            return false;
        };
        if !cancelled.is_empty() {
            self.emit(PressCancelled { contact, position }, &cancelled[..]);
        }
        if !exited.is_empty() {
            self.emit(Exit { contact, position }, &exited[..]);
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

    fn cancel_contact(&mut self, contact: ContactId) -> bool {
        let target = self.target();
        let Some((captured, position)) = self.resource_mut::<Contacts>().cancel(contact, target)
        else {
            return false;
        };
        self.emit(PressCancelled { contact, position }, &captured[..]);
        true
    }
}
