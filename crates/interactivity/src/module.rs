//! Pointer, touch and keyboard input in, per-node events out.
//!
//! Contact input is four systems on the same `ContactInput`, each
//! filtering on its own phase, rather than one function with a match
//! over all four: `Moved` is independent hover tracking; `Pressed`,
//! `Released` and `Cancelled` are one capture lifecycle. Keyboard input
//! is one system on `KeyboardInput`, routing to whichever node holds the
//! keyboard focus.

use app::{App, Module, NodeId};

use crate::contact::{ContactId, ContactInput, ContactPhase};
use crate::contacts::{Contacts, HitSet};
use crate::events::{Clicked, Enter, Exit, KeyPress, KeyRelease, KeyRepeat, Press, Release};
use crate::hit_test::hit_test;
use crate::keyboard::{KeyState, KeyboardFocus, KeyboardInput};

/// `presentation` is trusted to know which surface an input came from; a
/// stale or non-`Window` id here is its bug, not this crate's. `hit_test`
/// against a bad id just finds nothing, so release behavior stays a
/// silent no-op — this only catches the mistake in debug builds.
fn trusted_window(app: &App, window: NodeId) {
    debug_assert!(
        app.widget::<window::Window>(window).is_some(),
        "ContactInput.window must name a live Window node; this is a producer bug"
    );
}

/// Registers the `Contacts` and `KeyboardFocus` resources and the systems
/// that turn a `ContactInput` into `Press`/`Release`/`Enter`/`Exit`/
/// `Clicked`, and a `KeyboardInput` into `KeyPress`/`KeyRepeat`/
/// `KeyRelease`. Installs after `WindowModule`.
pub struct InteractivityModule;

impl Module for InteractivityModule {
    fn install(self, app: &mut App) {
        app.init_resource::<Contacts>()
            .init_resource::<KeyboardFocus>()
            .system(on_moved)
            .system(on_pressed)
            .system(on_released)
            .system(on_cancelled)
            .system(on_keyboard);
    }
}

fn diff(old: &[NodeId], new: &[NodeId]) -> (HitSet, HitSet) {
    let entered = new.iter().copied().filter(|id| !old.contains(id)).collect();
    let exited = old.iter().copied().filter(|id| !new.contains(id)).collect();
    (entered, exited)
}

fn on_moved(app: &mut App, input: &ContactInput) {
    if input.phase != ContactPhase::Moved {
        return;
    }
    trusted_window(app, input.window);
    let hit = hit_test(app, input.window, input.position);
    let (entered, exited) = {
        let mut contacts = app.resource_mut::<Contacts>();
        let state = contacts.entry(input.contact, input.window);
        let (entered, exited) = diff(&state.hit, &hit);
        state.hit = hit;
        state.position = input.position;
        (entered, exited)
    };
    if !exited.is_empty() {
        app.emit(
            Exit {
                contact: input.contact,
                position: input.position,
            },
            &exited[..],
        );
    }
    if !entered.is_empty() {
        app.emit(
            Enter {
                contact: input.contact,
                position: input.position,
            },
            &entered[..],
        );
    }
}

fn on_pressed(app: &mut App, input: &ContactInput) {
    if input.phase != ContactPhase::Pressed {
        return;
    }
    trusted_window(app, input.window);
    let hit = hit_test(app, input.window, input.position);
    {
        let mut contacts = app.resource_mut::<Contacts>();
        let state = contacts.entry(input.contact, input.window);
        // No diff against the previous `hit` here, unlike `on_moved`: the
        // producer contract guarantees a `Pressed` always follows a
        // `Moved` at the same position, so `state.hit` is already this
        // same set and no Enter/Exit is owed.
        state.hit = hit.clone();
        state.captured = Some(hit.clone());
        state.position = input.position;
    }
    if !hit.is_empty() {
        app.emit(
            Press {
                contact: input.contact,
                position: input.position,
                time: input.time,
            },
            &hit[..],
        );
    }
}

fn on_released(app: &mut App, input: &ContactInput) {
    if input.phase != ContactPhase::Released {
        return;
    }
    let captured = {
        let mut contacts = app.resource_mut::<Contacts>();
        let Some(state) = contacts.get_mut(input.contact) else {
            return;
        };
        state.position = input.position;
        state.captured.take()
    };
    if let Some(captured) = captured {
        if !captured.is_empty() {
            app.emit(
                Release {
                    contact: input.contact,
                    position: input.position,
                    time: input.time,
                },
                &captured[..],
            );
            app.emit(
                Clicked {
                    contact: input.contact,
                    position: input.position,
                },
                &captured[..],
            );
        }
    }
    if let ContactId::Touch(_) = input.contact {
        let hit = {
            let mut contacts = app.resource_mut::<Contacts>();
            contacts.take(input.contact).map(|s| s.hit)
        };
        if let Some(hit) = hit {
            if !hit.is_empty() {
                app.emit(
                    Exit {
                        contact: input.contact,
                        position: input.position,
                    },
                    &hit[..],
                );
            }
        }
    }
}

fn on_cancelled(app: &mut App, input: &ContactInput) {
    if input.phase != ContactPhase::Cancelled {
        return;
    }
    let hit = {
        let mut contacts = app.resource_mut::<Contacts>();
        contacts.take(input.contact).map(|s| s.hit)
    };
    let Some(hit) = hit else {
        return;
    };
    if !hit.is_empty() {
        app.emit(
            Exit {
                contact: input.contact,
                position: input.position,
            },
            &hit[..],
        );
    }
}

// ── keyboard ──────────────────────────────────────────────────────────────

/// One report becomes one event at the focused node — the same shape
/// `ContactInput` takes to `Press`/`Release`. With no focus the report
/// is dropped: a keyboard with nowhere to go is a no-op.
fn on_keyboard(app: &mut App, input: &KeyboardInput) {
    let Some(node) = app.resource::<KeyboardFocus>().focused() else {
        return;
    };
    let meaning = input.meaning.clone();
    let modifiers = input.modifiers;
    match input.state {
        KeyState::Pressed => app.emit(KeyPress { meaning, modifiers }, node),
        KeyState::Repeated => app.emit(KeyRepeat { meaning, modifiers }, node),
        KeyState::Released => app.emit(KeyRelease { meaning, modifiers }, node),
    }
}
