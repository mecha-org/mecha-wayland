//! Pointer, touch and keyboard input in, per-node events out.
//!
//! Contact input is four systems on the same `ContactInput`, each
//! filtering on its own phase, rather than one function with a match
//! over all four: `Moved` is independent hover tracking; `Pressed`,
//! `Released` and `Cancelled` are one capture lifecycle. Keyboard input
//! is one system on `KeyboardInput`, routing to whichever node holds the
//! keyboard focus. A claimed contact (see `claim.rs`) short-circuits all
//! four to its claimer.

use app::{App, Module, NodeId};

use crate::claim::GrabPresses;
use crate::contact::{ContactId, ContactInput, ContactPhase};
use crate::contacts::{Contacts, HitSet};
use crate::events::{
    Clicked, Enter, Exit, KeyPress, KeyRelease, KeyRepeat, Press, PressCancelled, PressMoved,
    Release,
};
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

/// Registers the `Contacts` and `KeyboardFocus` resources, the
/// `GrabPresses` component, and the systems
/// that turn a `ContactInput` into `Press`/`PressMoved`/`PressCancelled`/
/// `Release`/`Enter`/`Exit`/`Clicked`, and a `KeyboardInput` into `KeyPress`/`KeyRepeat`/
/// `KeyRelease`. Installs after `WindowModule`.
pub struct InteractivityModule;

impl Module for InteractivityModule {
    fn install(self, app: &mut App) {
        app.init_resource::<Contacts>()
            .init_resource::<KeyboardFocus>()
            .register_component::<GrabPresses>()
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
    // A claimed contact does no hover tracking: its moves are the
    // claimer's alone.
    let claimed = {
        let mut contacts = app.resource_mut::<Contacts>();
        let state = contacts.entry(input.contact, input.window);
        if state.claimed.is_some() {
            state.position = input.position;
        }
        state.claimed
    };
    if let Some(claimer) = claimed {
        app.emit(
            PressMoved {
                contact: input.contact,
                position: input.position,
                time: input.time,
            },
            claimer,
        );
        return;
    }
    let hit = hit_test(app, input.window, input.position);
    let (entered, exited, captured) = {
        let mut contacts = app.resource_mut::<Contacts>();
        let state = contacts.entry(input.contact, input.window);
        let (entered, exited) = diff(&state.hit, &hit);
        state.hit = hit;
        state.position = input.position;
        (entered, exited, state.captured.clone())
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
    if let Some(captured) = captured.filter(|c| !c.is_empty()) {
        app.emit(
            PressMoved {
                contact: input.contact,
                position: input.position,
                time: input.time,
            },
            &captured[..],
        );
    }
}

fn on_pressed(app: &mut App, input: &ContactInput) {
    if input.phase != ContactPhase::Pressed {
        return;
    }
    trusted_window(app, input.window);
    // One capture per contact: a further `Pressed` while one is held (a
    // second mouse button) leaves the capture, and any claim, untouched.
    if app.resource::<Contacts>().is_pressed(input.contact) {
        return;
    }
    let hit = hit_test(app, input.window, input.position);
    // The outermost grabber: the hit-set runs window last.
    let grabber = hit
        .iter()
        .rev()
        .copied()
        .find(|&id| app.component::<GrabPresses>(id).is_some_and(|g| g.0));
    if let Some(grabber) = grabber {
        let exited: HitSet = hit.iter().copied().filter(|&id| id != grabber).collect();
        {
            let mut contacts = app.resource_mut::<Contacts>();
            let state = contacts.entry(input.contact, input.window);
            state.hit = HitSet::from_slice(&[grabber]);
            state.captured = Some(HitSet::from_slice(&[grabber]));
            state.claimed = Some(grabber);
            state.position = input.position;
        }
        if !exited.is_empty() {
            app.emit(
                Exit {
                    contact: input.contact,
                    position: input.position,
                },
                &exited[..],
            );
        }
        app.emit(
            Press {
                contact: input.contact,
                position: input.position,
                time: input.time,
            },
            grabber,
        );
        return;
    }
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
        // The claim ends with the capture; a claimed capture is already
        // just `[claimer]`.
        state.claimed = None;
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
    let state = {
        let mut contacts = app.resource_mut::<Contacts>();
        contacts.take(input.contact)
    };
    let Some(state) = state else {
        return;
    };
    if let Some(captured) = state.captured.filter(|c| !c.is_empty()) {
        app.emit(
            PressCancelled {
                contact: input.contact,
                position: input.position,
            },
            &captured[..],
        );
    }
    let hit = state.hit;
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
