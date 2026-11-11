#![forbid(unsafe_code)]
//! `zwp_text_input_v3`: IME composition into the `Input` widget.
//!
//! The protocol half of text input. The other half — control keys
//! (Backspace, Delete, arrows) from a real or virtual keyboard — lives
//! in the `Input` widget itself, which handles `KeyPress`/`KeyRepeat`
//! directly and emits `InputFocus` to tell this crate to report the
//! change to the IME. This crate only drives the `zwp_text_input_v3`
//! protocol: `enable`/`disable`/`set_surrounding_text`/`commit` on
//! focus changes, and accumulating `preedit_string`/
//! `commit_string`/`delete_surrounding_text` into one `InputEdit` per
//! `done`.
//!
//! # Model
//!
//! - [`TextInputModule`] binds `zwp_text_input_manager_v3` and makes one
//!   `zwp_text_input_v3` for the seat. Installs after `RingModule`,
//!   `WaylandModule` (which must have bound `WlSeat`) and
//!   `InteractivityModule`.
//! - Focus follows the press, but the press is detected *in the
//!   widget*: an `Input` whose `Press` handler runs emits an
//!   `InputFocus { focused: true }` at itself, which this crate's
//!   `Emitted<InputFocus>` system turns into a committed `enable`
//!   carrying the widget's text as the surrounding text. A `Press` that
//!   misses the focused widget in the focused widget's own window — by a
//!   cheap id check against the press targets, no per-press tree walk —
//!   blurs it with a committed `disable`; a press in another of the
//!   app's windows (an OSK, a panel) leaves the focus alone. The
//!   surface focus the `Enter`/`Leave` events carry is the
//!   compositor's business: a `Leave` only resets (the compositor
//!   ignores our requests until the next `Enter` anyway) — and, not
//!   being double-buffered, clears the widget's preedit immediately —
//!   and that `Enter` re-enables for whichever widget is still focused.
//! - An edit is everything one `done` carries. The `preedit_string`,
//!   `commit_string` and `delete_surrounding_text` events since the last
//!   `done` accumulate in [`TextInput`]; the `done` emits one
//!   `widgets::InputEdit` at the focused widget, whose handler applies
//!   it — delete around the cursor, then the commit at the cursor, else
//!   the new preedit shown there — and funnels the result to its `Text`
//!   through the `Input` context's `set_text`.
//! - The `Emitted<InputEdit>` that follows sends the widget's new
//!   surrounding text back with `set_text_change_cause(input_method)`,
//!   committed, so the input method's picture of the text stays true
//!   across every edit: a backspace's `delete_surrounding_text` is
//!   counted from what we last reported.
//! - `language`, `action` and `preedit_hint` events are read and
//!   dropped: nowhere to put them. The `done` serial is not
//!   checked against our commit count; the surrounding text is resent
//!   after every edit regardless, which is what a mismatched serial
//!   asks for anyway.
//! - A focused widget removed from the tree blurs.
//!
//! # Quick start
//!
//! ```no_run
//! use app::prelude::*;
//! use atlas::prelude::*;
//! use interactivity::prelude::*;
//! use layout::prelude::*;
//! use paint::prelude::*;
//! use ring::prelude::*;
//! use text_input::prelude::*;
//! use wayland::prelude::*;
//! use widgets::prelude::*;
//! use window::prelude::*;
//!
//! let mut app = App::new();
//! app.add_module(LayoutModule)
//!     .add_module(PaintModule)
//!     .add_module(WindowModule)
//!     .add_module(InteractivityModule);
//! app.add_module(RingModule::default())
//!     .add_module(WaylandModule::new().bind::<WlSeat>())
//!     .add_module(TextInputModule);
//! app.insert_resource(Atlas::new());
//! let font = app
//!     .resource_mut::<Atlas>()
//!     .add_font(include_bytes!("../../atlas/tests/fixtures/Inter-Regular.ttf"))
//!     .unwrap();
//! let root = app.root();
//! let win = app.spawn(root, window());
//! app.spawn(win, input(font));
//! // A press on the input focuses it, the compositor raises the OSK,
//! // and every key press lands in the widget.
//! app.run();
//! ```

use app::prelude::*;
use interactivity::{KeyboardFocus, Press};
use wayland::prelude::*;
use widgets::{ContentHint, ContentPurpose, Input, InputEdit, InputFocus};
use window::InWindow;

pub mod prelude {
    pub use crate::{TextInput, TextInputModule};
}

/// The protocol's byte cap on `set_surrounding_text`.
const SURROUNDING_MAX: usize = 4000;

/// The seat's text input: the `zwp_text_input_v3` object and the focus
/// bookkeeping around it. One per app, for whatever seat
/// `WaylandModule` bound.
pub struct TextInput {
    input: ZwpTextInputV3,
    /// Whether the compositor's text-input focus is on our surface; an
    /// `enable` is only valid while it is.
    entered: bool,
    /// Whether a committed `enable` is in effect.
    enabled: bool,
    /// The input widget that wants the text, if any.
    focused: Option<NodeId>,
    /// The edit the events since the last `done` have built up.
    pending: Option<InputEdit>,
}
impl Resource for TextInput {}

impl TextInput {
    /// The input widget that currently holds the text focus.
    pub fn focused(&self) -> Option<NodeId> {
        self.focused
    }
}

/// Binds the manager, makes the seat's text input, and listens.
///
/// # Panics
///
/// At install, if `WlSeat` was not bound by `WaylandModule`, or if the
/// compositor does not advertise `zwp_text_input_manager_v3`.
pub struct TextInputModule;

impl Module for TextInputModule {
    fn install(self, app: &mut App) {
        let seat = *app.resource::<WlSeat>();
        let manager: ZwpTextInputManagerV3 = {
            let (globals, mut wl) = app.query::<(Res<Globals>, ResMut<Wayland>)>();
            globals.bind_if_offered(&mut wl, 1).unwrap_or_else(|| {
                panic!(
                    "text-input: the compositor does not advertise {}",
                    ZwpTextInputManagerV3::NAME
                )
            })
        };
        let input = {
            let mut wl = app.resource_mut::<Wayland>();
            let input = manager.get_text_input(&mut wl, seat);
            manager.destroy(&mut wl);
            input
        };
        app.insert_resource(TextInput {
            input,
            entered: false,
            enabled: false,
            focused: None,
            pending: None,
        });
        app.system(on_text_input)
            .system(on_press)
            .system(on_focus)
            .system(on_edited)
            .system(on_removed);
    }
}

/// Focus `w`: the committed `enable` for it, with its text as the
/// surrounding text. A different widget already enabled is disabled
/// first, as the protocol asks. Before an `Enter` the flags are all the
/// request can set: the `Enter` that follows runs this again. Protocol
/// only — the caret toggle is the widget's own `InputFocus` handler,
/// driven by the emits the press path and `on_focus` produce.
fn focus(app: &mut App, w: NodeId) {
    let (text, cursor, hint, purpose) = match app.widget::<Input>(w) {
        Some(i) => (
            i.text().to_string(),
            i.cursor(),
            i.content_hint(),
            i.content_purpose(),
        ),
        None => (String::new(), 0, ContentHint::NONE, ContentPurpose::Normal),
    };
    app.resource_mut::<KeyboardFocus>().focus(w);
    let (mut ti, mut wl) = app.query::<(ResMut<TextInput>, ResMut<Wayland>)>();
    let ti = &mut *ti;
    if ti.enabled {
        ti.input.disable(&mut wl);
        ti.input.commit(&mut wl);
        ti.enabled = false;
    }
    ti.focused = Some(w);
    ti.pending = None;
    if ti.entered && !ti.enabled {
        ti.input.enable(&mut wl);
        let (text, cursor) = cap(&text, cursor);
        ti.input
            .set_surrounding_text(&mut wl, &text, cursor as i32, cursor as i32);
        // Tell the input method what kind of field this is so it can
        // raise the right panel (e.g. a numeric keypad for `Pin`, a
        // hidden-text keyboard for `Password`). Double-buffered like
        // the surrounding text, so it lands with this `commit`.
        ti.input
            .set_content_type(&mut wl, wayland_hint(hint), wayland_purpose(purpose));
        ti.input.commit(&mut wl);
        ti.enabled = true;
    }
}

/// Translate the widget's [`ContentHint`] (mirroring the protocol's
/// `content_hint` bits) into the generated
/// [`ZwpTextInputV3ContentHint`]. Both derive from the same XML, so a
/// bit-for-bit truncation is exact.
fn wayland_hint(hint: ContentHint) -> ZwpTextInputV3ContentHint {
    ZwpTextInputV3ContentHint::from_bits_truncate(hint.bits())
}

/// Translate the widget's [`ContentPurpose`] (mirroring the protocol's
/// `content_purpose` enum) into the generated
/// [`ZwpTextInputV3ContentPurpose`].
fn wayland_purpose(purpose: ContentPurpose) -> ZwpTextInputV3ContentPurpose {
    match purpose {
        ContentPurpose::Normal => ZwpTextInputV3ContentPurpose::Normal,
        ContentPurpose::Alpha => ZwpTextInputV3ContentPurpose::Alpha,
        ContentPurpose::Digits => ZwpTextInputV3ContentPurpose::Digits,
        ContentPurpose::Number => ZwpTextInputV3ContentPurpose::Number,
        ContentPurpose::Phone => ZwpTextInputV3ContentPurpose::Phone,
        ContentPurpose::Url => ZwpTextInputV3ContentPurpose::Url,
        ContentPurpose::Email => ZwpTextInputV3ContentPurpose::Email,
        ContentPurpose::Name => ZwpTextInputV3ContentPurpose::Name,
        ContentPurpose::Password => ZwpTextInputV3ContentPurpose::Password,
        ContentPurpose::Pin => ZwpTextInputV3ContentPurpose::Pin,
        ContentPurpose::Date => ZwpTextInputV3ContentPurpose::Date,
        ContentPurpose::Time => ZwpTextInputV3ContentPurpose::Time,
        ContentPurpose::Datetime => ZwpTextInputV3ContentPurpose::Datetime,
        ContentPurpose::Terminal => ZwpTextInputV3ContentPurpose::Terminal,
    }
}

/// Blur: the committed `disable`, and no focused widget. Protocol only.
fn blur(app: &mut App) {
    app.resource_mut::<KeyboardFocus>().blur();
    let (mut ti, mut wl) = app.query::<(ResMut<TextInput>, ResMut<Wayland>)>();
    let ti = &mut *ti;
    if ti.enabled {
        ti.input.disable(&mut wl);
        ti.input.commit(&mut wl);
    }
    ti.enabled = false;
    ti.focused = None;
    ti.pending = None;
}

/// The surrounding text is capped at 4000 bytes by the protocol; keep
/// the window that ends at the string's end, so the cursor stays in it.
fn cap(text: &str, cursor: usize) -> (String, usize) {
    if text.len() <= SURROUNDING_MAX {
        return (text.to_string(), cursor);
    }
    let mut start = text.len() - SURROUNDING_MAX;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    (text[start..].to_string(), cursor.saturating_sub(start))
}

/// A press that misses the focused widget blurs it — the way tapping
/// outside a field ends its edit on a phone. Focus gain is the widget's
/// own `Press` handler emitting `InputFocus { focused: true }`; this
/// system only handles the converse, and by a cheap id check against the
/// press targets — no per-press tree walk for an `Input`.
///
/// Only a press in the focused widget's own window counts. A press in
/// another of the app's windows — an on-screen keyboard or a panel
/// whose layer surface takes no keyboard focus — leaves the text focus
/// where it is: whether that surface takes the keyboard is the
/// compositor's call, reported as a `Leave`.
fn on_press(app: &mut App, e: &Emitted<Press>) {
    let Some(focused) = app.resource::<TextInput>().focused else {
        return;
    };
    if e.targets.iter().any(|&id| id == focused) {
        return;
    }
    let window_of = |id| app.component::<InWindow>(id).and_then(|w| w.0);
    let pressed = e.targets.first().and_then(|&id| window_of(id));
    if pressed.is_some() && pressed == window_of(focused) {
        app.emit(InputFocus { focused: false }, focused);
    }
}

/// The widget's focus request (from its `Press` handler) or blur (from
/// `on_press`): drive the protocol — `enable`/`disable`/
/// `set_surrounding_text`/`commit` — for the change. A `{ focused: true }`
/// on the already-focused widget is a cursor move from a press: report
/// the new cursor back to the IM with `Other` as the cause, so its next
/// edit lands where the user clicked.
fn on_focus(app: &mut App, e: &Emitted<InputFocus>) {
    let Some(&w) = e.targets.first() else {
        return;
    };
    if e.event.focused {
        if app.resource::<TextInput>().focused != Some(w) {
            focus(app, w);
        } else {
            report(app, w, ZwpTextInputV3ChangeCause::Other);
        }
    } else if app.resource::<TextInput>().focused == Some(w) {
        blur(app);
    }
}

/// Report the widget's current text and cursor back to the IM, with
/// `cause`, committed. A no-op when not enabled or not entered.
fn report(app: &mut App, w: NodeId, cause: ZwpTextInputV3ChangeCause) {
    let (text, cursor) = match app.widget::<Input>(w) {
        Some(i) => cap(i.text(), i.cursor()),
        None => return,
    };
    let (mut ti, mut wl) = app.query::<(ResMut<TextInput>, ResMut<Wayland>)>();
    let ti = &mut *ti;
    if !ti.enabled {
        return;
    }
    ti.input
        .set_surrounding_text(&mut wl, &text, cursor as i32, cursor as i32);
    ti.input.set_text_change_cause(&mut wl, cause);
    ti.input.commit(&mut wl);
}

/// The protocol's events. The edit events accumulate; `done` hands them
/// to the focused widget as one `InputEdit`.
fn on_text_input(app: &mut App, e: &ZwpTextInputV3Event) {
    match e {
        ZwpTextInputV3Event::Enter { .. } => {
            let w = {
                let mut ti = app.resource_mut::<TextInput>();
                ti.entered = true;
                ti.pending = None;
                ti.focused.filter(|_| !ti.enabled)
            };
            if let Some(w) = w {
                focus(app, w);
            }
        }
        ZwpTextInputV3Event::Leave { .. } => {
            let w = {
                let mut ti = app.resource_mut::<TextInput>();
                ti.entered = false;
                ti.enabled = false;
                ti.pending = None;
                ti.focused
            };
            // `Leave` is not double-buffered: clear the widget's preedit.
            if let Some(w) = w {
                app.emit(InputEdit::default(), w);
            }
        }
        ZwpTextInputV3Event::PreeditString { text, .. } => {
            let mut ti = app.resource_mut::<TextInput>();
            let edit = ti.pending.get_or_insert_default();
            edit.preedit = text.clone().filter(|t| !t.is_empty());
        }
        ZwpTextInputV3Event::CommitString { text, .. } => {
            let mut ti = app.resource_mut::<TextInput>();
            let edit = ti.pending.get_or_insert_default();
            // A null text is an empty commit; not exclusive with preedit.
            edit.commit = Some(text.clone().unwrap_or_default());
        }
        ZwpTextInputV3Event::DeleteSurroundingText {
            before_length,
            after_length,
            ..
        } => {
            let mut ti = app.resource_mut::<TextInput>();
            let edit = ti.pending.get_or_insert_default();
            edit.delete_before = *before_length;
            edit.delete_after = *after_length;
        }
        ZwpTextInputV3Event::Done { .. } => {
            let (w, edit) = {
                let mut ti = app.resource_mut::<TextInput>();
                (ti.focused, ti.pending.take())
            };
            // A `done` with no accumulated events still must clear any
            // preedit the widget is showing: a default `InputEdit`
            // carries no commit and an empty preedit, which `apply`
            // turns into `self.preedit = None`.
            if let Some(w) = w {
                app.emit(edit.unwrap_or_default(), w);
            }
        }
        // `action`, `language` and `preedit_hint` have nowhere to go.
        _ => {}
    }
}

/// The focused widget applied an edit: report its new text back, with
/// the input method as the cause, committed.
fn on_edited(app: &mut App, e: &Emitted<InputEdit>) {
    let Some(&w) = e.targets.first() else {
        return;
    };
    report(app, w, ZwpTextInputV3ChangeCause::InputMethod);
}

/// A focused widget leaving the tree blurs.
fn on_removed(app: &mut App, r: &Removed) {
    if app.resource::<TextInput>().focused == Some(r.id) {
        blur(app);
    }
}
