//! Pointer and touch input, end to end on an `App`. There is no
//! `presentation` yet, so every test sends `ContactInput` by hand.

use std::cell::RefCell;

use app::prelude::*;
use geometry::Point;
use interactivity::prelude::*;
use layout::prelude::*;
use window::prelude::*;

thread_local! {
    static SEEN: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}
fn log_contact_input(_: &mut App, input: &ContactInput) {
    SEEN.with(|l| {
        l.borrow_mut().push(match input.phase {
            ContactPhase::Moved => "moved",
            ContactPhase::Pressed => "pressed",
            ContactPhase::Released => "released",
            ContactPhase::Cancelled => "cancelled",
        })
    });
}

#[test]
fn contact_input_is_a_plain_signal() {
    let mut app = App::new();
    app.system(log_contact_input);
    let root = app.root();
    app.signal(ContactInput {
        window: root,
        contact: ContactId::Mouse,
        phase: ContactPhase::Moved,
        position: Point::new(1.0, 2.0),
        time: 0,
    });
    app.flush();
    assert_eq!(SEEN.with(|l| l.borrow().clone()), vec!["moved"]);
}

#[test]
fn every_event_carries_its_contact_and_position_and_reaches_its_target() {
    struct Watcher;
    struct WatcherBuilder(std::rc::Rc<std::cell::Cell<Option<ContactId>>>);
    impl Build for WatcherBuilder {
        type Widget = Watcher;
    }
    impl Widget for Watcher {
        type Builder = WatcherBuilder;
        fn build(b: WatcherBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
            let flag = b.0.clone();
            s.on::<Press>(me, move |_, e| flag.set(Some(e.contact)));
            Watcher
        }
    }

    let mut app = App::new();
    let seen = std::rc::Rc::new(std::cell::Cell::new(None));
    let watcher = app.spawn(app.root(), WatcherBuilder(seen.clone()));
    let pos = Point::new(3.0, 4.0);
    app.emit(
        Press {
            contact: ContactId::Touch(9),
            position: pos,
            time: 0,
        },
        watcher,
    );
    app.flush();
    assert_eq!(seen.get(), Some(ContactId::Touch(9)));
}

// ── fixtures for the dispatch tests ────────────────────────────────────

struct Leaf;
impl Build for Leaf {
    type Widget = Leaf;
}
impl Widget for Leaf {
    type Builder = Leaf;
    fn build(b: Leaf, _: Handle<Self>, _: &mut Spawner<'_, Self>) -> Self {
        b
    }
}

thread_local! {
    static ENTER: RefCell<Vec<NodeId>> = const { RefCell::new(Vec::new()) };
    static EXIT: RefCell<Vec<NodeId>> = const { RefCell::new(Vec::new()) };
}
fn log_enter(_: &mut App, e: &Emitted<Enter>) {
    ENTER.with(|l| l.borrow_mut().extend(e.targets.iter().copied()));
}
fn log_exit(_: &mut App, e: &Emitted<Exit>) {
    EXIT.with(|l| l.borrow_mut().extend(e.targets.iter().copied()));
}
fn take_enter() -> Vec<NodeId> {
    ENTER.with(|l| std::mem::take(&mut *l.borrow_mut()))
}
fn take_exit() -> Vec<NodeId> {
    EXIT.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

thread_local! {
    static TIMES: RefCell<Vec<(&'static str, u32)>> = const { RefCell::new(Vec::new()) };
}
fn log_press_time(_: &mut App, e: &Emitted<Press>) {
    TIMES.with(|l| l.borrow_mut().push(("press", e.event.time)));
}
fn log_release_time(_: &mut App, e: &Emitted<Release>) {
    TIMES.with(|l| l.borrow_mut().push(("release", e.event.time)));
}

thread_local! {
    static PRESS: RefCell<Vec<NodeId>> = const { RefCell::new(Vec::new()) };
    static RELEASE: RefCell<Vec<NodeId>> = const { RefCell::new(Vec::new()) };
    static CLICKED: RefCell<Vec<NodeId>> = const { RefCell::new(Vec::new()) };
}
fn log_press(_: &mut App, e: &Emitted<Press>) {
    PRESS.with(|l| l.borrow_mut().extend(e.targets.iter().copied()));
}
fn log_release(_: &mut App, e: &Emitted<Release>) {
    RELEASE.with(|l| l.borrow_mut().extend(e.targets.iter().copied()));
}
fn log_clicked(_: &mut App, e: &Emitted<Clicked>) {
    CLICKED.with(|l| l.borrow_mut().extend(e.targets.iter().copied()));
}
fn take_press() -> Vec<NodeId> {
    PRESS.with(|l| std::mem::take(&mut *l.borrow_mut()))
}
fn take_release() -> Vec<NodeId> {
    RELEASE.with(|l| std::mem::take(&mut *l.borrow_mut()))
}
fn take_clicked() -> Vec<NodeId> {
    CLICKED.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

// A shared-order log, alongside the per-event ones above: those can't
// express relative order between different event types, since each is
// its own `Vec`. This one records just the event kind, in emission
// order, across every logger below.
thread_local! {
    static ORDER: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}
fn log_order_press(_: &mut App, _: &Emitted<Press>) {
    ORDER.with(|l| l.borrow_mut().push("press"));
}
fn log_order_release(_: &mut App, _: &Emitted<Release>) {
    ORDER.with(|l| l.borrow_mut().push("release"));
}
fn log_order_clicked(_: &mut App, _: &Emitted<Clicked>) {
    ORDER.with(|l| l.borrow_mut().push("clicked"));
}
fn log_order_exit(_: &mut App, _: &Emitted<Exit>) {
    ORDER.with(|l| l.borrow_mut().push("exit"));
}
thread_local! {
    static PRESS_MOVED: RefCell<Vec<NodeId>> = const { RefCell::new(Vec::new()) };
}
fn log_press_moved(_: &mut App, e: &Emitted<PressMoved>) {
    PRESS_MOVED.with(|l| l.borrow_mut().extend(e.targets.iter().copied()));
}
fn take_press_moved() -> Vec<NodeId> {
    PRESS_MOVED.with(|l| std::mem::take(&mut *l.borrow_mut()))
}
fn log_order_press_cancelled(_: &mut App, _: &Emitted<PressCancelled>) {
    ORDER.with(|l| l.borrow_mut().push("press_cancelled"));
}

fn take_order() -> Vec<&'static str> {
    ORDER.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

fn app() -> App {
    let mut app = App::new();
    app.add_module(LayoutModule)
        .add_module(WindowModule)
        .add_module(InteractivityModule)
        .system(log_enter)
        .system(log_exit)
        .system(log_press)
        .system(log_release)
        .system(log_clicked)
        .system(log_order_press)
        .system(log_order_release)
        .system(log_order_clicked)
        .system(log_order_exit)
        .system(log_press_moved)
        .system(log_order_press_cancelled);
    app
}

/// A 200x200 window at the origin.
fn a_window() -> WindowBuilder {
    window().layout(LayoutStyle::default().size(px(200.0), px(200.0)))
}

/// An absolutely positioned box at `(x, y)`, `w` by `h`, relative to its
/// containing block.
fn at(x: f32, y: f32, w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::default()
        .absolute()
        .inset(geometry::Insets::new(px(y), auto(), auto(), px(x)))
        .size(px(w), px(h))
}

fn moved(window: impl Into<NodeId>, contact: ContactId, position: Point) -> ContactInput {
    ContactInput {
        window: window.into(),
        contact,
        phase: ContactPhase::Moved,
        position,
        time: 0,
    }
}

fn pressed(window: impl Into<NodeId>, contact: ContactId, position: Point) -> ContactInput {
    ContactInput {
        window: window.into(),
        contact,
        phase: ContactPhase::Pressed,
        position,
        time: 0,
    }
}
fn released(window: impl Into<NodeId>, contact: ContactId, position: Point) -> ContactInput {
    ContactInput {
        window: window.into(),
        contact,
        phase: ContactPhase::Released,
        position,
        time: 0,
    }
}
fn cancelled(window: impl Into<NodeId>, contact: ContactId, position: Point) -> ContactInput {
    ContactInput {
        window: window.into(),
        contact,
        phase: ContactPhase::Cancelled,
        position,
        time: 0,
    }
}

/// A 120x120 card absolutely positioned at (20, 20) in `win`, and a
/// 40x40 button, the card's one flow child — window-relative rect
/// (20, 20, 40, 40).
fn card_and_button(app: &mut App, win: Handle<Window>) -> (Handle<Leaf>, Handle<Leaf>) {
    let card = app.spawn_with(win, Leaf, (at(20.0, 20.0, 120.0, 120.0),));
    let button = app.spawn_with(
        card,
        Leaf,
        (LayoutStyle::default().size(px(40.0), px(40.0)),),
    );
    (card, button)
}

// ── Moved: Enter / Exit ─────────────────────────────────────────────────

#[test]
fn moved_onto_a_nested_node_enters_deepest_first_window_last() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let (card, button) = card_and_button(&mut app, win);
    app.tick();

    app.signal(moved(win, ContactId::Mouse, Point::new(30.0, 30.0)));
    app.flush();

    assert_eq!(take_enter(), vec![button.id(), card.id(), win.id()]);
    assert!(take_exit().is_empty());
}

#[test]
fn a_second_moved_still_inside_fires_nothing_again() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let (_, _) = card_and_button(&mut app, win);
    app.tick();

    app.signal(moved(win, ContactId::Mouse, Point::new(30.0, 30.0)));
    app.flush();
    take_enter();

    app.signal(moved(win, ContactId::Mouse, Point::new(35.0, 35.0)));
    app.flush();
    assert!(take_enter().is_empty());
    assert!(take_exit().is_empty());
}

#[test]
fn moving_off_every_node_exits_deepest_first_window_last() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let (card, button) = card_and_button(&mut app, win);
    app.tick();

    app.signal(moved(win, ContactId::Mouse, Point::new(30.0, 30.0)));
    app.flush();
    take_enter();

    app.signal(moved(win, ContactId::Mouse, Point::new(199.0, 199.0)));
    app.flush();
    assert_eq!(take_exit(), vec![button.id(), card.id()]);
    assert!(
        take_enter().is_empty(),
        "the window is still under the point"
    );
}

#[test]
fn moving_directly_from_one_leaf_to_a_sibling_only_touches_their_own_chains() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let left = app.spawn_with(win, Leaf, (at(0.0, 0.0, 50.0, 50.0),));
    let right = app.spawn_with(win, Leaf, (at(100.0, 0.0, 50.0, 50.0),));
    app.tick();

    app.signal(moved(win, ContactId::Mouse, Point::new(25.0, 25.0)));
    app.flush();
    assert_eq!(take_enter(), vec![left.id(), win.id()]);

    app.signal(moved(win, ContactId::Mouse, Point::new(125.0, 25.0)));
    app.flush();
    assert_eq!(
        take_exit(),
        vec![left.id()],
        "the window stayed in both hit-sets"
    );
    assert_eq!(take_enter(), vec![right.id()]);
}

// ── Pressed / Released / Cancelled: capture ─────────────────────────────

#[test]
fn press_then_release_at_the_same_spot_fires_press_release_clicked_on_the_same_set() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let (card, button) = card_and_button(&mut app, win);
    app.tick();
    let expected = vec![button.id(), card.id(), win.id()];

    let p = Point::new(30.0, 30.0);
    app.signal(pressed(win, ContactId::Mouse, p));
    app.flush();
    assert_eq!(take_press(), expected);
    assert!(app.resource::<Contacts>().is_pressed(ContactId::Mouse));

    app.signal(released(win, ContactId::Mouse, p));
    app.flush();
    assert_eq!(take_release(), expected);
    assert_eq!(take_clicked(), expected);
    assert!(!app.resource::<Contacts>().is_pressed(ContactId::Mouse));
}

#[test]
fn press_and_release_carry_the_input_time() {
    let mut app = app();
    app.system(log_press_time).system(log_release_time);
    let win = app.spawn(app.root(), a_window());
    card_and_button(&mut app, win);
    app.tick();

    let p = Point::new(30.0, 30.0);
    app.signal(ContactInput {
        time: 1000,
        ..moved(win, ContactId::Mouse, p)
    });
    app.signal(ContactInput {
        time: 1000,
        ..pressed(win, ContactId::Mouse, p)
    });
    app.signal(ContactInput {
        time: 1040,
        ..released(win, ContactId::Mouse, p)
    });
    app.flush();
    let times = TIMES.with(|l| std::mem::take(&mut *l.borrow_mut()));
    assert!(times.contains(&("press", 1000)));
    assert!(times.contains(&("release", 1040)));
    assert!(
        times
            .iter()
            .all(|&(k, t)| t == if k == "press" { 1000 } else { 1040 })
    );
}

#[test]
fn a_capture_survives_moving_off_the_node_before_release() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let (card, button) = card_and_button(&mut app, win);
    app.tick();
    let expected = vec![button.id(), card.id(), win.id()];

    app.signal(pressed(win, ContactId::Mouse, Point::new(30.0, 30.0)));
    app.flush();
    take_press();

    app.signal(moved(win, ContactId::Mouse, Point::new(199.0, 199.0)));
    app.flush();
    assert_eq!(
        take_exit(),
        vec![button.id(), card.id()],
        "hover left despite the capture"
    );

    app.signal(released(win, ContactId::Mouse, Point::new(199.0, 199.0)));
    app.flush();
    assert_eq!(
        take_release(),
        expected,
        "release still targets the pressed set, not a fresh hit test"
    );
    assert_eq!(take_clicked(), expected);
}

#[test]
fn cancelled_exits_the_held_set_with_no_release_or_clicked() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let (card, button) = card_and_button(&mut app, win);
    app.tick();
    let expected = vec![button.id(), card.id(), win.id()];

    app.signal(pressed(win, ContactId::Mouse, Point::new(30.0, 30.0)));
    app.flush();
    take_press();

    app.signal(cancelled(win, ContactId::Mouse, Point::new(30.0, 30.0)));
    app.flush();
    assert_eq!(take_exit(), expected);
    assert!(take_release().is_empty());
    assert!(take_clicked().is_empty());
    assert!(!app.resource::<Contacts>().is_pressed(ContactId::Mouse));

    app.signal(released(win, ContactId::Mouse, Point::new(30.0, 30.0)));
    app.flush();
    assert!(
        take_release().is_empty(),
        "nothing was captured left to release"
    );
}

#[test]
fn a_touchs_release_tears_down_its_state_but_the_mouse_keeps_hovering() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let (card, button) = card_and_button(&mut app, win);
    app.tick();
    let expected = vec![button.id(), card.id(), win.id()];
    let p = Point::new(30.0, 30.0);

    app.signal(moved(win, ContactId::Touch(1), p));
    app.signal(pressed(win, ContactId::Touch(1), p));
    app.signal(released(win, ContactId::Touch(1), p));
    app.flush();
    assert_eq!(take_enter(), expected);
    assert_eq!(take_press(), expected);
    assert_eq!(take_release(), expected);
    assert_eq!(take_clicked(), expected);
    assert_eq!(
        take_exit(),
        expected,
        "a finger has no idle position to keep hovering at"
    );
    assert_eq!(
        app.resource::<Contacts>().position(ContactId::Touch(1)),
        None
    );
    assert!(!app.resource::<Contacts>().is_pressed(ContactId::Touch(1)));

    app.signal(moved(win, ContactId::Mouse, p));
    app.signal(pressed(win, ContactId::Mouse, p));
    app.signal(released(win, ContactId::Mouse, p));
    app.flush();
    take_enter();
    take_press();
    take_release();
    take_clicked();
    assert!(take_exit().is_empty(), "the mouse pointer keeps its entry");
    assert_eq!(
        app.resource::<Contacts>().position(ContactId::Mouse),
        Some(p)
    );
}

#[test]
fn a_touchs_release_without_a_prior_press_still_tears_down_its_state() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let (card, button) = card_and_button(&mut app, win);
    app.tick();
    let expected = vec![button.id(), card.id(), win.id()];
    let p = Point::new(30.0, 30.0);

    app.signal(moved(win, ContactId::Touch(1), p));
    app.flush();
    take_enter();

    app.signal(released(win, ContactId::Touch(1), p));
    app.flush();
    assert_eq!(
        take_exit(),
        expected,
        "the hovered set still gets torn down"
    );
    assert!(take_release().is_empty(), "there was never a capture");
    assert!(take_clicked().is_empty(), "there was never a capture");
    assert_eq!(
        app.resource::<Contacts>().position(ContactId::Touch(1)),
        None,
        "the entry is gone"
    );
}

// ── multiple contacts stay independent ──────────────────────────────────

#[test]
fn two_contacts_at_different_spots_stay_independent() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let left = app.spawn_with(win, Leaf, (at(0.0, 0.0, 50.0, 50.0),));
    let right = app.spawn_with(win, Leaf, (at(100.0, 0.0, 50.0, 50.0),));
    app.tick();

    app.signal(pressed(win, ContactId::Mouse, Point::new(25.0, 25.0)));
    app.signal(pressed(win, ContactId::Touch(7), Point::new(125.0, 25.0)));
    app.flush();

    assert_eq!(
        take_press(),
        vec![left.id(), win.id(), right.id(), win.id()],
        "the mouse's Pressed dispatches fully before the touch's"
    );
    assert!(app.resource::<Contacts>().is_pressed(ContactId::Mouse));
    assert!(app.resource::<Contacts>().is_pressed(ContactId::Touch(7)));

    app.signal(released(win, ContactId::Mouse, Point::new(25.0, 25.0)));
    app.flush();
    assert!(!app.resource::<Contacts>().is_pressed(ContactId::Mouse));
    assert!(
        app.resource::<Contacts>().is_pressed(ContactId::Touch(7)),
        "releasing the mouse does not touch the touch contact's capture"
    );
}

// ── pinning tests from the final-review fix wave ────────────────────────

#[test]
fn a_plain_press_release_fires_press_then_release_then_clicked() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let (_, _) = card_and_button(&mut app, win);
    app.tick();

    let p = Point::new(30.0, 30.0);
    app.signal(pressed(win, ContactId::Mouse, p));
    app.signal(released(win, ContactId::Mouse, p));
    app.flush();

    assert_eq!(take_order(), vec!["press", "release", "clicked"]);
}

#[test]
fn cancelled_exits_the_current_hit_set_not_the_originally_captured_one() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let (card, button) = card_and_button(&mut app, win);
    let other = app.spawn_with(win, Leaf, (at(150.0, 150.0, 20.0, 20.0),));
    app.tick();

    let press_pos = Point::new(30.0, 30.0);
    let moved_pos = Point::new(155.0, 155.0);

    // Press captures [button, card, win].
    app.signal(pressed(win, ContactId::Mouse, press_pos));
    app.flush();
    take_press();

    // Moved off to a spot with a different hit-set: state.hit becomes
    // [other, win], but state.captured stays [button, card, win].
    app.signal(moved(win, ContactId::Mouse, moved_pos));
    app.flush();
    assert_eq!(
        take_exit(),
        vec![button.id(), card.id()],
        "hover left the originally pressed nodes"
    );
    take_enter();

    app.signal(cancelled(win, ContactId::Mouse, moved_pos));
    app.flush();
    assert_eq!(
        take_exit(),
        vec![other.id(), win.id()],
        "on_cancelled exits the post-move hit-set (.hit), not the captured one"
    );
    assert!(!app.resource::<Contacts>().is_pressed(ContactId::Mouse));
}

#[test]
fn touch_release_teardown_exits_the_current_hit_set_while_release_and_clicked_use_the_captured_one()
{
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let (card, button) = card_and_button(&mut app, win);
    let other = app.spawn_with(win, Leaf, (at(150.0, 150.0, 20.0, 20.0),));
    app.tick();
    let captured = vec![button.id(), card.id(), win.id()];

    let press_pos = Point::new(30.0, 30.0);
    let moved_pos = Point::new(155.0, 155.0);

    app.signal(moved(win, ContactId::Touch(1), press_pos));
    app.signal(pressed(win, ContactId::Touch(1), press_pos));
    app.flush();
    take_enter();
    take_press();

    // Moved off before releasing: state.hit becomes [other, win], but
    // state.captured (set at Pressed) stays [button, card, win].
    app.signal(moved(win, ContactId::Touch(1), moved_pos));
    app.flush();
    take_exit();
    take_enter();

    app.signal(released(win, ContactId::Touch(1), moved_pos));
    app.flush();
    assert_eq!(
        take_release(),
        captured,
        "Released's main dispatch still uses .captured"
    );
    assert_eq!(
        take_clicked(),
        captured,
        "Clicked also dispatches to .captured"
    );
    assert_eq!(
        take_exit(),
        vec![other.id(), win.id()],
        "the touch-teardown Exit reads the post-move .hit, not .captured"
    );
}

// ── PressMoved / PressCancelled ─────────────────────────────────────────

#[test]
fn press_moved_reaches_the_captured_set_not_the_fresh_hit() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let a = app.spawn_with(win, Leaf, (at(10.0, 10.0, 40.0, 40.0),));
    let b = app.spawn_with(win, Leaf, (at(100.0, 100.0, 40.0, 40.0),));
    app.tick();

    app.signal(moved(win, ContactId::Mouse, Point::new(20.0, 20.0)));
    app.signal(pressed(win, ContactId::Mouse, Point::new(20.0, 20.0)));
    app.flush();
    take_press_moved();

    app.signal(moved(win, ContactId::Mouse, Point::new(110.0, 110.0)));
    app.flush();
    let targets = take_press_moved();
    assert_eq!(targets, vec![a.id(), win.id()]);
    assert!(!targets.contains(&b.id()));
}

#[test]
fn no_press_moved_without_a_capture() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let _a = app.spawn_with(win, Leaf, (at(10.0, 10.0, 40.0, 40.0),));
    app.tick();

    app.signal(moved(win, ContactId::Mouse, Point::new(20.0, 20.0)));
    app.signal(moved(win, ContactId::Mouse, Point::new(30.0, 30.0)));
    app.flush();
    assert!(take_press_moved().is_empty());
}

#[test]
fn cancelled_sends_press_cancelled_to_the_captured_set_then_exit() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let (_card, _button) = card_and_button(&mut app, win);
    app.tick();

    app.signal(pressed(win, ContactId::Mouse, Point::new(30.0, 30.0)));
    app.flush();
    take_order();

    app.signal(cancelled(win, ContactId::Mouse, Point::new(30.0, 30.0)));
    app.flush();
    assert_eq!(take_order(), vec!["press_cancelled", "exit"]);
    assert!(take_clicked().is_empty());
}

// ── claiming a contact / grabbing presses ───────────────────────────────

thread_local! {
    static PRESS_CANCELLED: RefCell<Vec<NodeId>> = const { RefCell::new(Vec::new()) };
}
fn log_press_cancelled(_: &mut App, e: &Emitted<PressCancelled>) {
    PRESS_CANCELLED.with(|l| l.borrow_mut().extend(e.targets.iter().copied()));
}
fn take_press_cancelled() -> Vec<NodeId> {
    PRESS_CANCELLED.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

/// Claims its contact once a press on it has moved 10 px or more.
#[derive(Default)]
struct Claimer {
    start: Option<Point>,
    claimed: bool,
}
impl Build for Claimer {
    type Widget = Claimer;
}
impl Widget for Claimer {
    type Builder = Claimer;
    fn build(b: Claimer, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        s.on::<Press>(me, |ctx, e| ctx.me().start = Some(e.position));
        s.on::<PressMoved>(me, |ctx, e| {
            let Some(start) = ctx.me().start else {
                return;
            };
            let d = e.position - start;
            if (d.x * d.x + d.y * d.y).sqrt() >= 10.0 {
                let claimed = ctx.claim_contact(e.contact);
                ctx.me().claimed = claimed;
            }
        });
        b
    }
}

/// Tries to claim on every `Enter`, recording what the claim returned.
#[derive(Default)]
struct HoverClaimer {
    result: Option<bool>,
}
impl Build for HoverClaimer {
    type Widget = HoverClaimer;
}
impl Widget for HoverClaimer {
    type Builder = HoverClaimer;
    fn build(b: HoverClaimer, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        s.on::<Enter>(me, |ctx, e| {
            let r = ctx.claim_contact(e.contact);
            ctx.me().result = Some(r);
        });
        b
    }
}

fn claim_app() -> App {
    let mut app = app();
    app.system(log_press_cancelled);
    app
}

/// A `Claimer` at (10, 10, 150, 150) in `win` with a 40x40 button as its
/// one flow child — window-relative rect (10, 10, 40, 40).
fn claimer_and_button(app: &mut App, win: Handle<Window>) -> (Handle<Claimer>, Handle<Leaf>) {
    let claimer = app.spawn_with(win, Claimer::default(), (at(10.0, 10.0, 150.0, 150.0),));
    let button = app.spawn_with(
        claimer,
        Leaf,
        (LayoutStyle::default().size(px(40.0), px(40.0)),),
    );
    (claimer, button)
}

fn clear_logs() {
    take_enter();
    take_exit();
    take_press();
    take_release();
    take_clicked();
    take_press_moved();
    take_press_cancelled();
    take_order();
}

#[test]
fn claim_cancels_the_rest_of_the_capture() {
    let mut app = claim_app();
    let win = app.spawn(app.root(), a_window());
    let (claimer, button) = claimer_and_button(&mut app, win);
    app.tick();

    let p = Point::new(20.0, 20.0);
    app.signal(moved(win, ContactId::Mouse, p));
    app.signal(pressed(win, ContactId::Mouse, p));
    app.flush();
    clear_logs();

    app.signal(moved(win, ContactId::Mouse, Point::new(40.0, 20.0)));
    app.flush();
    assert!(app.widget::<Claimer>(claimer).unwrap().claimed);
    assert_eq!(take_press_cancelled(), vec![button.id(), win.id()]);
    assert_eq!(take_exit(), vec![button.id(), win.id()]);

    app.signal(released(win, ContactId::Mouse, Point::new(40.0, 20.0)));
    app.flush();
    assert_eq!(take_release(), vec![claimer.id()]);
    assert_eq!(take_clicked(), vec![claimer.id()]);
}

#[test]
fn claim_before_a_queued_release_suppresses_clicked() {
    let mut app = claim_app();
    let win = app.spawn(app.root(), a_window());
    let (claimer, button) = claimer_and_button(&mut app, win);
    app.tick();

    let p = Point::new(20.0, 20.0);
    app.signal(moved(win, ContactId::Touch(1), p));
    app.signal(pressed(win, ContactId::Touch(1), p));
    app.flush();
    clear_logs();

    app.signal(moved(win, ContactId::Touch(1), Point::new(40.0, 20.0)));
    app.signal(released(win, ContactId::Touch(1), Point::new(40.0, 20.0)));
    app.flush();
    let clicked = take_clicked();
    assert!(!clicked.contains(&button.id()));
    assert_eq!(clicked, vec![claimer.id()]);
}

#[test]
fn a_claimed_contact_stops_hover() {
    let mut app = claim_app();
    let win = app.spawn(app.root(), a_window());
    let (claimer, _button) = claimer_and_button(&mut app, win);
    let _other = app.spawn_with(win, Leaf, (at(170.0, 170.0, 20.0, 20.0),));
    app.tick();

    let p = Point::new(20.0, 20.0);
    app.signal(moved(win, ContactId::Mouse, p));
    app.signal(pressed(win, ContactId::Mouse, p));
    app.signal(moved(win, ContactId::Mouse, Point::new(40.0, 20.0)));
    app.flush();
    assert!(app.widget::<Claimer>(claimer).unwrap().claimed);
    clear_logs();

    app.signal(moved(win, ContactId::Mouse, Point::new(180.0, 180.0)));
    app.signal(moved(win, ContactId::Mouse, Point::new(185.0, 185.0)));
    app.flush();
    assert!(take_enter().is_empty());
    assert!(take_exit().is_empty());
    assert_eq!(take_press_moved(), vec![claimer.id(), claimer.id()]);
}

#[test]
fn grab_presses_takes_the_press_before_children() {
    let mut app = claim_app();
    let win = app.spawn(app.root(), a_window());
    let outer = app.spawn_with(win, Leaf, (at(10.0, 10.0, 150.0, 150.0), GrabPresses(true)));
    let button = app.spawn_with(
        outer,
        Leaf,
        (LayoutStyle::default().size(px(40.0), px(40.0)),),
    );
    app.tick();

    let p = Point::new(20.0, 20.0);
    app.signal(moved(win, ContactId::Mouse, p));
    app.flush();
    clear_logs();

    app.signal(pressed(win, ContactId::Mouse, p));
    app.flush();
    assert_eq!(take_press(), vec![outer.id()]);
    assert_eq!(take_exit(), vec![button.id(), win.id()]);

    app.signal(moved(win, ContactId::Mouse, Point::new(25.0, 20.0)));
    app.signal(released(win, ContactId::Mouse, Point::new(25.0, 20.0)));
    app.flush();
    assert_eq!(take_press_moved(), vec![outer.id()]);
    assert_eq!(take_release(), vec![outer.id()]);
    assert_eq!(take_clicked(), vec![outer.id()]);
}

#[test]
fn grab_picks_the_outermost() {
    let mut app = claim_app();
    let win = app.spawn(app.root(), a_window());
    let outer = app.spawn_with(win, Leaf, (at(10.0, 10.0, 150.0, 150.0), GrabPresses(true)));
    let inner = app.spawn_with(
        outer,
        Leaf,
        (
            LayoutStyle::default().size(px(100.0), px(100.0)),
            GrabPresses(true),
        ),
    );
    let _button = app.spawn_with(
        inner,
        Leaf,
        (LayoutStyle::default().size(px(40.0), px(40.0)),),
    );
    app.tick();

    let p = Point::new(20.0, 20.0);
    app.signal(moved(win, ContactId::Mouse, p));
    app.signal(pressed(win, ContactId::Mouse, p));
    app.signal(released(win, ContactId::Mouse, p));
    app.flush();
    assert_eq!(take_press(), vec![outer.id()]);
    assert_eq!(take_clicked(), vec![outer.id()]);
}

#[test]
fn claim_without_capture_is_false() {
    let mut app = claim_app();
    let win = app.spawn(app.root(), a_window());
    let node = app.spawn_with(win, HoverClaimer::default(), (at(10.0, 10.0, 40.0, 40.0),));
    app.tick();

    app.signal(moved(win, ContactId::Mouse, Point::new(20.0, 20.0)));
    app.flush();
    assert_eq!(
        app.widget::<HoverClaimer>(node).unwrap().result,
        Some(false)
    );
    assert!(!app.resource::<Contacts>().is_pressed(ContactId::Mouse));
}

#[test]
fn a_second_press_during_a_claim_keeps_the_claim() {
    let mut app = claim_app();
    let win = app.spawn(app.root(), a_window());
    let (claimer, button) = claimer_and_button(&mut app, win);
    app.tick();

    let p = Point::new(20.0, 20.0);
    let q = Point::new(40.0, 20.0);
    app.signal(moved(win, ContactId::Mouse, p));
    app.signal(pressed(win, ContactId::Mouse, p));
    app.signal(moved(win, ContactId::Mouse, q));
    app.flush();
    assert!(app.widget::<Claimer>(claimer).unwrap().claimed);
    clear_logs();

    // A second button goes down on the same pointer.
    app.signal(pressed(win, ContactId::Mouse, q));
    app.flush();
    assert!(take_press().is_empty(), "the second press is a no-op");

    app.signal(released(win, ContactId::Mouse, q));
    app.flush();
    let clicked = take_clicked();
    assert!(!clicked.contains(&button.id()));
    assert_eq!(clicked, vec![claimer.id()]);
    assert_eq!(take_release(), vec![claimer.id()]);
}

// ── Focus ───────────────────────────────────────────────────────────────

fn tap(app: &mut App, win: Handle<Window>, at: Point) {
    app.signal(moved(win, ContactId::Touch(1), at));
    app.signal(pressed(win, ContactId::Touch(1), at));
    app.signal(released(win, ContactId::Touch(1), at));
    app.flush();
}

#[test]
fn a_press_focuses_the_nearest_focusable_and_nothing_else_moves_it() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let (card, _) = card_and_button(&mut app, win);
    let field = app.spawn_with(win, Leaf, (at(150.0, 150.0, 40.0, 40.0),));
    *app.component_mut::<Focusable>(field).unwrap() = Focusable(true);
    *app.component_mut::<Focusable>(card).unwrap() = Focusable(true);
    app.tick();
    let focused = |app: &App| app.resource::<KeyboardFocus>().focused();

    tap(&mut app, win, Point::new(160.0, 160.0));
    assert_eq!(focused(&app), Some(field.id()));

    // On the window itself, focusable nowhere: the focus stays.
    tap(&mut app, win, Point::new(190.0, 5.0));
    assert_eq!(focused(&app), Some(field.id()));

    // The button isn't focusable; the card it is in is.
    tap(&mut app, win, Point::new(30.0, 30.0));
    assert_eq!(focused(&app), Some(card.id()));
}

#[test]
fn a_cover_over_a_focusable_does_not_keep_the_focus_from_it() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let list = app.spawn_with(win, Leaf, (at(20.0, 20.0, 40.0, 40.0),));
    *app.component_mut::<Focusable>(list).unwrap() = Focusable(true);
    // Spawned later, so in front, as a container's fade is: the press
    // goes to both, and the list takes the focus.
    app.spawn_with(win, Leaf, (at(0.0, 0.0, 100.0, 100.0),));
    app.tick();

    tap(&mut app, win, Point::new(30.0, 30.0));
    assert_eq!(app.resource::<KeyboardFocus>().focused(), Some(list.id()));
}
