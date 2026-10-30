#![forbid(unsafe_code)]
//! Pointer, touch and keyboard input in, per-node events out.
//!
//! `interactivity` knows no compositor and no protocol: `presentation`
//! (or a test, until `presentation` exists) reduces `wl_pointer`,
//! `wl_touch` and `wl_keyboard` to two signals, [`ContactInput`] and
//! [`KeyboardInput`], and this crate turns those into per-node events:
//! [`Press`], [`PressMoved`], [`PressCancelled`], [`Release`], [`Enter`],
//! [`Exit`] and [`Clicked`] at the
//! nodes under a contact's position, and [`KeyPress`], [`KeyRepeat`],
//! [`KeyRelease`] at the node holding the keyboard focus.
//!
//! # Model — contacts
//!
//! - Every event dispatches deepest/frontmost-first, the contact's window
//!   last: a preorder walk's matches, reversed.
//! - [`Enter`]/[`Exit`] come from a fresh hit test on every `Moved`,
//!   independent of any capture in progress — unless the contact is
//!   claimed (below).
//! - [`Press`] locks in ("captures") the hit-set under the contact for
//!   that press; [`Release`] and [`Clicked`] route to that captured set,
//!   never to a fresh hit test, even if the contact moved in between.
//! - [`PressMoved`] fires on every `Moved` of a contact with a capture,
//!   to the captured set, after that move's `Exit`/`Enter`.
//!   [`PressCancelled`] goes to the captured set on a `Cancelled`, before
//!   the teardown `Exit`; that set then never gets `Release`/`Clicked`.
//! - A handler on a captured node can claim the contact with
//!   [`ContextClaim::claim_contact`]: the capture becomes that node
//!   alone, the rest of the old capture gets [`PressCancelled`] and the
//!   rest of the old hit-set [`Exit`]. The claim takes effect inside the
//!   call, so a `Released` already queued goes to the claimer alone.
//! - A press over a node with [`GrabPresses`]`(true)` is claimed from the
//!   start by the outermost such node in its hit-set: it alone gets
//!   [`Press`], and the rest of the hit-set gets [`Exit`].
//! - While claimed, a `Moved` does no hover tracking and sends
//!   [`PressMoved`] to the claimer alone; `Release`/`Clicked` go to the
//!   claimer alone. The claim ends with the capture. A mouse that has
//!   not moved since gets no [`Enter`] until its next `Moved`.
//! - A `Pressed` on a contact that already holds a capture (a second
//!   mouse button) is a no-op: the capture and any claim stay as they are.
//! - A `Touch` contact's state is torn down right after its `Released`
//!   (with a final `Exit` for whatever it is over at that point), since a
//!   finger has no idle position to keep hovering at. A `Mouse` contact's
//!   state persists across `Released` and keeps hovering.
//! - [`Clicked`] fires unconditionally alongside `Release`: there is
//!   no check that the contact is still within the captured set's bounds
//!   at release time.
//! - Every [`ContactInput`] carries the Wayland event's millisecond
//!   `time`; [`Press`] and [`Release`] pass it on unchanged. It wraps at
//!   `u32::MAX`.
//!
//! # Model — keyboard
//!
//! - [`KeyboardFocus`] is the node keyboard events dispatch to. Whatever
//!   owns that focus sets it with [`KeyboardFocus::focus`] and clears it
//!   with [`KeyboardFocus::blur`]. A report with no focus is dropped.
//! - [`KeyPress`], [`KeyRepeat`] and [`KeyRelease`] carry the key and the
//!   modifiers in effect; no polling, no per-frame deltas — one report
//!   becomes one event the focused node's handler runs.
//!
//! # Quick start
//!
//! ```
//! use std::cell::Cell;
//! use std::rc::Rc;
//!
//! use app::prelude::*;
//! use geometry::Point;
//! use layout::prelude::*;
//! use window::prelude::*;
//! use interactivity::prelude::*;
//!
//! struct Button {
//!     clicked: Rc<Cell<bool>>,
//! }
//! struct ButtonBuilder;
//! impl Build for ButtonBuilder {
//!     type Widget = Button;
//! }
//! impl Widget for Button {
//!     type Builder = ButtonBuilder;
//!     fn build(_: ButtonBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
//!         let clicked = Rc::new(Cell::new(false));
//!         let flag = clicked.clone();
//!         s.on::<Clicked>(me, move |_, _| flag.set(true));
//!         Button { clicked }
//!     }
//! }
//!
//! let mut app = App::new();
//! app.add_module(LayoutModule)
//!     .add_module(WindowModule)
//!     .add_module(InteractivityModule);
//! let win = app.spawn(app.root(), window());
//! let button = app.spawn_with(
//!     win,
//!     ButtonBuilder,
//!     (LayoutStyle::default().size(px(40.0), px(40.0)),),
//! );
//! app.tick();
//!
//! let pos = Point::ZERO; // the button sits at the window's content origin
//! app.signal(ContactInput { window: win.id(), contact: ContactId::Mouse, phase: ContactPhase::Pressed, position: pos, time: 0 });
//! app.signal(ContactInput { window: win.id(), contact: ContactId::Mouse, phase: ContactPhase::Released, position: pos, time: 0 });
//! app.flush();
//!
//! assert!(app.widget::<Button>(button).unwrap().clicked.get());
//! ```

mod claim;
mod contact;
mod contacts;
mod events;
mod hit_test;
mod keyboard;
mod module;

pub use claim::{ContextClaim, GrabPresses};
pub use contact::{ContactId, ContactInput, ContactPhase};
pub use contacts::Contacts;
pub use events::{
    Clicked, Enter, Exit, KeyPress, KeyRelease, KeyRepeat, Press, PressCancelled, PressMoved,
    Release,
};
pub use keyboard::{KeyMeaning, KeyState, KeyboardFocus, KeyboardInput, Modifiers};
pub use module::InteractivityModule;

pub mod prelude {
    pub use crate::{
        Clicked, ContactId, ContactInput, ContactPhase, Contacts, ContextClaim, Enter, Exit,
        GrabPresses, InteractivityModule, KeyMeaning, KeyPress, KeyRelease, KeyRepeat, KeyState,
        KeyboardFocus, KeyboardInput, Modifiers, Press, PressCancelled, PressMoved, Release,
    };
}
