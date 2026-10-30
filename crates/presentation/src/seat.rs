//! `wl_seat`'s pointer and touch, reduced to `interactivity::ContactInput`.
//! `Seat` is presentation's own state for this: the bound `wl_seat`, the
//! `wl_pointer`/`wl_touch` objects once requested, and (from later in
//! this file) the small bit of focus/position bookkeeping a reducer
//! needs that `interactivity::Contacts` cannot supply in time.
//! `App::signal` queues onto a list `App::flush` drains later, so a
//! `ContactInput` just sent has not reached `Contacts` yet when the next
//! protocol event for the same contact arrives.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use app::prelude::*;
use geometry::Point;
use interactivity::{
    ContactId, ContactInput, ContactPhase, KeyMeaning, KeyState, KeyboardInput, Modifiers,
};
use ring::{IoEvent, Ring, Token};
use wayland::prelude::*;
use xkbcommon::xkb;

use crate::Surfaces;

pub(crate) struct Seat {
    seat: WlSeat,
    pointer: Option<WlPointer>,
    touch: Option<WlTouch>,
    keyboard: Option<WlKeyboard>,
    /// The xkb context, keymap and state for decoding `wl_keyboard.key`
    /// events into [`KeyMeaning`]. `None` until a `Keymap` event arrives.
    xkb: Option<Xkb>,
    /// The modifier set from the latest `wl_keyboard.modifiers`, folded
    /// into every `KeyboardInput` this seat signals. The two events
    /// arrive on separate `wl_keyboard` events and are joined here, at
    /// the reducer.
    modifiers: Modifiers,
    /// Client-side key repeat: the compositor only reports press and
    /// release, and the client repeats a held key per `repeat_info`.
    repeat: Repeat,
    /// The window the pointer last entered, and its position there.
    /// `None` between a `Leave` and the next `Enter`.
    pointer_focus: Option<(NodeId, Point)>,
    /// Every touch point currently down: `wl_touch`'s id to the window
    /// it started on and its last known position.
    touches: HashMap<i32, (NodeId, Point)>,
}
impl Resource for Seat {}

/// The xkb state for one keyboard: the context (shared, owns include
/// paths), the compiled keymap, and the mutable state updated on each
/// key and modifier event. `!Send`/`!Sync` — the app is single-threaded.
struct Xkb {
    _ctx: xkb::Context,
    keymap: xkb::Keymap,
    state: xkb::State,
}

/// Client-side key repeat state, per `wl_keyboard.repeat_info`.
struct Repeat {
    /// Repeats per second; `0` disables repeat.
    rate: i32,
    /// Milliseconds from key down to the first repeat.
    delay: i32,
    /// The raw keycode held down, if it repeats; cleared on its release
    /// or on keyboard `Leave`.
    held: Option<u32>,
    /// When the held key next repeats.
    deadline: Instant,
    /// Ring timeouts in flight. A press or release does not cancel one;
    /// a stale timer that fires before `deadline` (or with nothing held)
    /// is finished and ignored.
    timers: Vec<Token>,
}

impl Default for Repeat {
    /// The protocol leaves the rate unset until `repeat_info` (v4+);
    /// these are the conventional defaults until then.
    fn default() -> Self {
        Repeat {
            rate: 25,
            delay: 600,
            held: None,
            deadline: Instant::now(),
            timers: Vec::new(),
        }
    }
}

impl Seat {
    pub(crate) fn new(seat: WlSeat) -> Seat {
        Seat {
            seat,
            pointer: None,
            touch: None,
            keyboard: None,
            xkb: None,
            modifiers: Modifiers::default(),
            repeat: Repeat::default(),
            pointer_focus: None,
            touches: HashMap::new(),
        }
    }
}

/// The seat's capabilities changed (or were reported for the first
/// time): request a pointer/touch object the first time each bit is
/// seen. Never released — capability loss at runtime is out of scope,
/// per the design doc.
pub(crate) fn on_seat(app: &mut App, e: &WlSeatEvent) {
    let WlSeatEvent::Capabilities { capabilities, .. } = e else {
        return;
    };
    let (mut seat, mut wl) = app.query::<(ResMut<Seat>, ResMut<Wayland>)>();
    let s = seat.seat;
    if capabilities.contains(WlSeatCapability::POINTER) && seat.pointer.is_none() {
        seat.pointer = Some(s.get_pointer(&mut wl));
    }
    if capabilities.contains(WlSeatCapability::TOUCH) && seat.touch.is_none() {
        seat.touch = Some(s.get_touch(&mut wl));
    }
    if capabilities.contains(WlSeatCapability::KEYBOARD) && seat.keyboard.is_none() {
        seat.keyboard = Some(s.get_keyboard(&mut wl));
    }
}

/// `wl_pointer` reduced to `ContactInput`. `Button` and `Leave` carry no
/// position of their own — the protocol's own words for `Button` are
/// "the location of the click is given by the last motion or enter
/// event", which is exactly `pointer_focus`, always fresh by
/// construction since every `Motion`/`Enter` updates it first.
pub(crate) fn on_pointer(app: &mut App, e: &WlPointerEvent) {
    match e {
        WlPointerEvent::Enter {
            surface,
            surface_x,
            surface_y,
            ..
        } => {
            let Some(window) = app.resource::<Surfaces>().window_of(surface.id()) else {
                app.resource_mut::<Seat>().pointer_focus = None;
                return;
            };
            let position = Point::new(*surface_x, *surface_y);
            app.resource_mut::<Seat>().pointer_focus = Some((window, position));
            app.signal(ContactInput {
                window,
                contact: ContactId::Mouse,
                phase: ContactPhase::Moved,
                position,
            });
        }
        WlPointerEvent::Motion {
            surface_x,
            surface_y,
            ..
        } => {
            let Some((window, _)) = app.resource::<Seat>().pointer_focus else {
                return;
            };
            let position = Point::new(*surface_x, *surface_y);
            app.resource_mut::<Seat>().pointer_focus = Some((window, position));
            app.signal(ContactInput {
                window,
                contact: ContactId::Mouse,
                phase: ContactPhase::Moved,
                position,
            });
        }
        WlPointerEvent::Button { state, .. } => {
            let Some((window, position)) = app.resource::<Seat>().pointer_focus else {
                return;
            };
            let phase = match state {
                WlPointerButtonState::Pressed => ContactPhase::Pressed,
                WlPointerButtonState::Released => ContactPhase::Released,
            };
            app.signal(ContactInput {
                window,
                contact: ContactId::Mouse,
                phase,
                position,
            });
        }
        WlPointerEvent::Leave { .. } => {
            let Some((window, position)) = app.resource_mut::<Seat>().pointer_focus.take() else {
                return;
            };
            app.signal(ContactInput {
                window,
                contact: ContactId::Mouse,
                phase: ContactPhase::Cancelled,
                position,
            });
        }
        _ => {}
    }
}

/// `wl_touch` reduced to `ContactInput`. A `Down` is two signals in
/// order — `Moved` then `Pressed`, both at the same position —
/// `interactivity` depends on exactly that ordering (see
/// `ContactPhase::Moved`'s doc). `Cancel` carries no `id` at all: the
/// protocol says it "applies to all touch points currently active on
/// this client", so every tracked touch is cancelled at once.
pub(crate) fn on_touch(app: &mut App, e: &WlTouchEvent) {
    match e {
        WlTouchEvent::Down {
            surface, id, x, y, ..
        } => {
            let Some(window) = app.resource::<Surfaces>().window_of(surface.id()) else {
                return;
            };
            let position = Point::new(*x, *y);
            app.resource_mut::<Seat>()
                .touches
                .insert(*id, (window, position));
            app.signal(ContactInput {
                window,
                contact: ContactId::Touch(*id as u32),
                phase: ContactPhase::Moved,
                position,
            });
            app.signal(ContactInput {
                window,
                contact: ContactId::Touch(*id as u32),
                phase: ContactPhase::Pressed,
                position,
            });
        }
        WlTouchEvent::Motion { id, x, y, .. } => {
            let Some((window, _)) = app.resource::<Seat>().touches.get(id).copied() else {
                return;
            };
            let position = Point::new(*x, *y);
            app.resource_mut::<Seat>()
                .touches
                .insert(*id, (window, position));
            app.signal(ContactInput {
                window,
                contact: ContactId::Touch(*id as u32),
                phase: ContactPhase::Moved,
                position,
            });
        }
        WlTouchEvent::Up { id, .. } => {
            let Some((window, position)) = app.resource_mut::<Seat>().touches.remove(id) else {
                return;
            };
            app.signal(ContactInput {
                window,
                contact: ContactId::Touch(*id as u32),
                phase: ContactPhase::Released,
                position,
            });
        }
        WlTouchEvent::Cancel { .. } => {
            let touches = std::mem::take(&mut app.resource_mut::<Seat>().touches);
            for (id, (window, position)) in touches {
                app.signal(ContactInput {
                    window,
                    contact: ContactId::Touch(id as u32),
                    phase: ContactPhase::Cancelled,
                    position,
                });
            }
        }
        _ => {}
    }
}

/// `wl_keyboard` reduced to `interactivity::KeyboardInput`. The keymap
/// event loads the xkb keymap (read from the fd as a string, compiled
/// safely — `new_from_fd` is `unsafe` and this crate forbids unsafe);
/// modifier events update the xkb state's modifier mask and the
/// `Modifiers` struct folded into each report; key events decode the
/// keycode through xkb into a `KeyMeaning` and signal it.
/// `RepeatInfo` sets the client-side repeat rate; a held key that
/// repeats per the keymap signals `Repeated` from a ring timer (see
/// [`on_repeat_timer`]). `Enter` is dropped and `Leave` only stops the
/// repeat: keyboard focus is the `interactivity` crate's concern,
/// surfaced as `KeyboardFocus`.
pub(crate) fn on_keyboard(app: &mut App, e: &WlKeyboardEvent) {
    match e {
        WlKeyboardEvent::Keymap {
            fd, format, size, ..
        } => {
            // `new_from_fd` is unsafe (mmap); read the fd to a string and
            // compile with the safe `new_from_string`. Only `xkb_v1` is
            // handled; `no_keymap` leaves xkb unset.
            // Seek to the start first: memfds are seekable, so the read is
            // offset-independent of whatever a prior run left behind.
            if *format == WlKeyboardKeymapFormat::XkbV1 {
                let mut file = std::fs::File::from(fd.try_clone().expect("dup the keymap fd"));
                use std::io::{Read, Seek, SeekFrom};
                let _ = file.seek(SeekFrom::Start(0));
                let mut buf = vec![0u8; *size as usize];
                if file.read_exact(&mut buf).is_ok() {
                    if let Ok(string) = String::from_utf8(buf) {
                        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
                        if let Some(keymap) = xkb::Keymap::new_from_string(
                            &ctx,
                            string,
                            xkb::KEYMAP_FORMAT_TEXT_V1,
                            xkb::KEYMAP_COMPILE_NO_FLAGS,
                        ) {
                            let state = xkb::State::new(&keymap);
                            app.resource_mut::<Seat>().xkb = Some(Xkb {
                                _ctx: ctx,
                                keymap,
                                state,
                            });
                        }
                    }
                }
            }
        }
        WlKeyboardEvent::Modifiers {
            mods_depressed,
            mods_latched,
            mods_locked,
            group,
            ..
        } => {
            let mut seat = app.resource_mut::<Seat>();
            let combined = *mods_depressed | *mods_latched | *mods_locked;
            seat.modifiers = Modifiers::from_bits(combined);
            if let Some(xkb) = seat.xkb.as_mut() {
                xkb.state
                    .update_mask(*mods_depressed, *mods_latched, *mods_locked, 0, 0, *group);
            }
        }
        WlKeyboardEvent::Key { key, state, .. } => {
            let state = match state {
                WlKeyboardKeyState::Pressed => KeyState::Pressed,
                WlKeyboardKeyState::Released => KeyState::Released,
                WlKeyboardKeyState::Repeated => KeyState::Repeated,
            };
            match state {
                KeyState::Pressed => start_repeat(app, *key),
                KeyState::Released => {
                    let mut seat = app.resource_mut::<Seat>();
                    if seat.repeat.held == Some(*key) {
                        seat.repeat.held = None;
                    }
                }
                KeyState::Repeated => {}
            }
            signal_key(app, *key, state);
        }
        WlKeyboardEvent::RepeatInfo { rate, delay, .. } => {
            let mut seat = app.resource_mut::<Seat>();
            seat.repeat.rate = *rate;
            seat.repeat.delay = *delay;
        }
        WlKeyboardEvent::Leave { .. } => {
            app.resource_mut::<Seat>().repeat.held = None;
        }
        // `Enter` (compositor surface focus) has nowhere to go.
        _ => {}
    }
}

/// Decode `key` and signal it as a `KeyboardInput` in `state`, then
/// fold a press or release into the xkb state.
fn signal_key(app: &mut App, key: u32, state: KeyState) {
    let (meaning, modifiers) = {
        let seat = app.resource::<Seat>();
        (decode(&seat, key), seat.modifiers)
    };
    // Update the xkb state after decoding, so the keysym reported
    // for this event is not affected by the event itself (per
    // xkb docs). Only on press/release — repeats carry no state
    // change.
    if matches!(state, KeyState::Pressed | KeyState::Released) {
        if let Some(xkb) = app.resource_mut::<Seat>().xkb.as_mut() {
            let keycode = xkb::Keycode::new(key + 8);
            let dir = if state == KeyState::Pressed {
                xkb::KeyDirection::Down
            } else {
                xkb::KeyDirection::Up
            };
            xkb.state.update_key(keycode, dir);
        }
    }
    app.signal(KeyboardInput {
        meaning,
        state,
        modifiers,
    });
}

/// A key went down: hold it for repeat if repeat is on and the keymap
/// says the key repeats (modifiers don't), arming the first timer at
/// `delay`. A new press replaces any key already held.
fn start_repeat(app: &mut App, key: u32) {
    let (mut seat, mut ring) = app.query::<(ResMut<Seat>, ResMut<Ring>)>();
    let repeats = seat
        .xkb
        .as_ref()
        .is_some_and(|x| x.keymap.key_repeats(xkb::Keycode::new(key + 8)));
    if !repeats || seat.repeat.rate <= 0 {
        seat.repeat.held = None;
        return;
    }
    let delay = Duration::from_millis(seat.repeat.delay.max(0) as u64);
    seat.repeat.held = Some(key);
    seat.repeat.deadline = Instant::now() + delay;
    let token = ring.timeout(delay);
    seat.repeat.timers.push(token);
}

/// A repeat timer fired: if a key is still held and its deadline has
/// passed, signal it `Repeated` and arm the next one at `1/rate`.
pub(crate) fn on_repeat_timer(app: &mut App, e: &IoEvent) {
    let key = {
        let (mut seat, mut ring) = app.query::<(ResMut<Seat>, ResMut<Ring>)>();
        let Some(i) = seat.repeat.timers.iter().position(|&t| t == e.token) else {
            return;
        };
        seat.repeat.timers.swap_remove(i);
        ring.finish(e.token);
        let now = Instant::now();
        let Some(key) = seat.repeat.held else {
            return;
        };
        if now < seat.repeat.deadline || seat.repeat.rate <= 0 {
            return;
        }
        let interval = Duration::from_secs(1) / seat.repeat.rate as u32;
        seat.repeat.deadline = (seat.repeat.deadline + interval).max(now);
        let token = ring.timeout(seat.repeat.deadline.saturating_duration_since(now));
        seat.repeat.timers.push(token);
        key
    };
    signal_key(app, key, KeyState::Repeated);
}

/// Decode one raw evdev keycode into a [`KeyMeaning`] through the xkb
/// state. Control modifiers (Ctrl/Alt/Logo) suppress text insertion —
/// those keys produce `None` so a widget can treat them as shortcuts
/// instead. Shift and caps/num lock are already folded into the keysym
/// and utf8 by xkb's level resolution.
fn decode(seat: &Seat, key: u32) -> KeyMeaning {
    let Some(xkb) = seat.xkb.as_ref() else {
        return KeyMeaning::None;
    };
    let keycode = xkb::Keycode::new(key + 8);
    let sym = xkb.state.key_get_one_sym(keycode).raw();
    use xkb::keysyms::*;
    if sym == KEY_BackSpace {
        KeyMeaning::Backspace
    } else if sym == KEY_Delete {
        KeyMeaning::Delete
    } else if sym == KEY_Left {
        KeyMeaning::Left
    } else if sym == KEY_Right {
        KeyMeaning::Right
    } else if sym == KEY_Home {
        KeyMeaning::Home
    } else if sym == KEY_End {
        KeyMeaning::End
    } else {
        // Control modifiers suppress text: Ctrl+C is "copy", not 'c'.
        if seat.modifiers.ctrl || seat.modifiers.alt || seat.modifiers.logo {
            return KeyMeaning::None;
        }
        let text = xkb.state.key_get_utf8(keycode);
        if text.is_empty() {
            KeyMeaning::None
        } else {
            KeyMeaning::Text(text)
        }
    }
}
