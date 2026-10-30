#![forbid(unsafe_code)]
//! Pointer, touch and keyboard input in, per-node events out.
//!
//! `interactivity` knows no compositor and no protocol: `presentation`
//! (or a test, until `presentation` exists) reduces `wl_pointer`,
//! `wl_touch` and `wl_keyboard` to two signals, [`ContactInput`] and
//! [`KeyboardInput`], and this crate turns those into per-node events:
//! [`Press`], [`Release`], [`Enter`], [`Exit`] and [`Clicked`] at the
//! nodes under a contact's position, and [`KeyPress`], [`KeyRepeat`],
//! [`KeyRelease`] at the node holding the keyboard focus.
//!
//! # Model — contacts
//!
//! - Every event dispatches deepest/frontmost-first, the contact's window
//!   last: a preorder walk's matches, reversed.
//! - [`Enter`]/[`Exit`] come from a fresh hit test on every `Moved`,
//!   independent of any capture in progress.
//! - [`Press`] locks in ("captures") the hit-set under the contact for
//!   that press; [`Release`] and [`Clicked`] route to that captured set,
//!   never to a fresh hit test, even if the contact moved in between.
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

mod contact;
mod contacts;
mod events;
mod hit_test;
mod keyboard;
mod module;

pub use contact::{ContactId, ContactInput, ContactPhase};
pub use contacts::Contacts;
pub use events::{Clicked, Enter, Exit, KeyPress, KeyRelease, KeyRepeat, Press, Release};
pub use keyboard::{KeyMeaning, KeyState, KeyboardFocus, KeyboardInput, Modifiers};
pub use module::InteractivityModule;

pub mod prelude {
    pub use crate::{
        Clicked, ContactId, ContactInput, ContactPhase, Contacts, Enter, Exit, InteractivityModule,
        KeyMeaning, KeyPress, KeyRelease, KeyRepeat, KeyState, KeyboardFocus, KeyboardInput,
        Modifiers, Press, Release,
    };
}
