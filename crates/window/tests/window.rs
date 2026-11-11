//! Windows as nodes: the widget, membership, the live list, resize, scale,
//! and the frame request loop, end to end on an `App`.

use std::cell::RefCell;

use app::prelude::*;
use geometry::{Color, Rect, Size};
use layout::prelude::*;
use window::prelude::*;

// ── fixtures ────────────────────────────────────────────────────────────

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

/// A node whose build spawns one `Leaf` child, so a test can see a child
/// whose `Spawned` arrives before its parent's.
struct Panel {
    child: Handle<Leaf>,
}
struct PanelBuilder;
impl Build for PanelBuilder {
    type Widget = Panel;
}
impl Widget for Panel {
    type Builder = PanelBuilder;
    fn build(_: PanelBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        let child = s.spawn(me, Leaf);
        Panel { child }
    }
}

/// A node outside every window, under the app root, with one child. Neither
/// is in a window.
struct Owner {
    child: Handle<Leaf>,
}
struct OwnerBuilder;
impl Build for OwnerBuilder {
    type Widget = Owner;
}
impl Widget for Owner {
    type Builder = OwnerBuilder;
    fn build(_: OwnerBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        let child = s.spawn(me, Leaf);
        Owner { child }
    }
}

// Systems cannot capture, so the tests log through thread-locals. Each
// test runs on its own thread, so logs never mix.
thread_local! {
    /// Every `FrameRequested` seen, in order.
    static REQUESTED: RefCell<Vec<NodeId>> = const { RefCell::new(Vec::new()) };
    /// Every `Frame` seen, in order.
    static FRAMES: RefCell<Vec<NodeId>> = const { RefCell::new(Vec::new()) };
    /// How many `OnChanged<Windows>` were seen.
    static WINDOWS_CHANGED: RefCell<u32> = const { RefCell::new(0) };
    /// Every `LayoutDone` seen, in order: its `roots`.
    static DONE: RefCell<Vec<Vec<NodeId>>> = const { RefCell::new(Vec::new()) };
}

fn log_requested(_: &mut App, r: &FrameRequested) {
    REQUESTED.with(|l| l.borrow_mut().push(r.0));
}
fn log_frame(_: &mut App, f: &Frame) {
    FRAMES.with(|l| l.borrow_mut().push(f.0));
}
fn log_windows_changed(_: &mut App, _: &OnChanged<Windows>) {
    WINDOWS_CHANGED.with(|n| *n.borrow_mut() += 1);
}
fn log_done(_: &mut App, d: &LayoutDone) {
    DONE.with(|l| l.borrow_mut().push(d.roots.clone()));
}
fn take_done() -> Vec<Vec<NodeId>> {
    DONE.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

fn take_requested() -> Vec<NodeId> {
    REQUESTED.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

fn take_frames() -> Vec<NodeId> {
    FRAMES.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

fn take_windows_changed() -> u32 {
    WINDOWS_CHANGED.with(|n| std::mem::take(&mut *n.borrow_mut()))
}

/// The WSI stand-in: answers every `FrameRequested` with a `Frame`.
fn answer(app: &mut App, r: &FrameRequested) {
    app.signal(Frame(r.0));
}

/// An app with layout, window and the logging systems. The loop is open:
/// nothing answers `FrameRequested`.
fn app() -> App {
    let mut app = App::new();
    app.add_module(LayoutModule)
        .add_module(WindowModule)
        .system(log_requested)
        .system(log_frame)
        .system(log_windows_changed)
        .system(log_done);
    app
}

/// `app()` with the loop closed by `answer`.
fn app_closed() -> App {
    let mut app = app();
    app.system(answer);
    app
}

/// A window asking for a fixed box.
fn a_window() -> WindowBuilder {
    window()
        .title("w")
        .layout(LayoutStyle::default().size(px(480.0), px(240.0)))
}

// ── 1: the widget ───────────────────────────────────────────────────────

#[test]
fn a_spawned_window_is_a_root_with_the_builders_style_and_is_laid_out() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    app.tick();

    assert_eq!(app.widget::<Window>(win).unwrap().title, "w");
    assert_eq!(app.component::<LayoutRoot>(win), Some(&LayoutRoot(true)));
    let style = app.component::<LayoutStyle>(win).unwrap();
    assert_eq!((style.width, style.height), (px(480.0), px(240.0)));
    assert_eq!(
        app.component::<InWindow>(win),
        Some(&InWindow(Some(win.id())))
    );
    assert_eq!(
        app.component::<ComputedLayout>(win).unwrap().rect,
        Rect::new(0.0, 0.0, 480.0, 240.0)
    );
    assert_eq!(take_done(), vec![vec![win.id()]]);
    assert!(!app.widget::<Window>(win).unwrap().is_pending());
    assert_eq!(app.widget::<Window>(win).unwrap().scale(), 1.0);
}

#[test]
fn the_default_style_is_a_column_sized_to_content() {
    let mut app = app();
    let win = app.spawn(app.root(), window());
    app.spawn_with(win, Leaf, (Measure::fixed(Size::new(48.0, 32.0)),));
    app.spawn_with(win, Leaf, (Measure::fixed(Size::new(20.0, 10.0)),));
    app.tick();

    let style = app.component::<LayoutStyle>(win).unwrap();
    assert_eq!(style.direction, Direction::Column);
    assert_eq!((style.width, style.height), (auto(), auto()));
    assert_eq!(
        app.component::<ComputedLayout>(win).unwrap().rect,
        Rect::new(0.0, 0.0, 48.0, 42.0),
        "as wide as the widest child, as tall as both stacked"
    );
}

// ── 2: InWindow ─────────────────────────────────────────────────────────

#[test]
fn in_window_is_filled_for_every_node_under_a_window_and_none_elsewhere() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let panel = app.spawn(win, PanelBuilder);
    let grandchild = app.widget::<Panel>(panel).unwrap().child;
    let owner = app.spawn(app.root(), OwnerBuilder);
    let owned = app.widget::<Owner>(owner).unwrap().child;
    app.flush();
    let later = app.spawn(win, Leaf);
    let outside = app.spawn(app.root(), Leaf);
    app.flush();

    let in_window = |id: NodeId| app.component::<InWindow>(id).unwrap().0;
    assert_eq!(in_window(win.id()), Some(win.id()), "a window is in itself");
    assert_eq!(
        in_window(panel.id()),
        Some(win.id()),
        "spawned under the window"
    );
    assert_eq!(
        in_window(grandchild.id()),
        Some(win.id()),
        "spawned in the panel's build, so its Spawned came before the panel's"
    );
    assert_eq!(
        in_window(later.id()),
        Some(win.id()),
        "spawned after a flush"
    );
    assert_eq!(in_window(owner.id()), None, "an owner under the app root");
    assert_eq!(in_window(owned.id()), None, "the owner's child");
    assert_eq!(in_window(outside.id()), None, "a leaf under the app root");
    assert_eq!(in_window(app.root()), None, "the app root");
}

#[test]
fn a_moved_subtree_is_in_its_new_window_and_both_windows_draw() {
    let mut app = app();
    let first = app.spawn(app.root(), a_window());
    let second = app.spawn(app.root(), a_window());
    let panel = app.spawn(first, PanelBuilder);
    let child = app.widget::<Panel>(panel).unwrap().child;
    app.tick();
    take_requested();

    app.move_to(panel, second);
    app.flush();

    let in_window = |id: NodeId| app.component::<InWindow>(id).unwrap().0;
    assert_eq!(in_window(panel.id()), Some(second.id()));
    assert_eq!(
        in_window(child.id()),
        Some(second.id()),
        "the whole subtree"
    );
    assert_eq!(take_requested(), [first.id(), second.id()], "old, then new");
}

#[test]
#[should_panic(expected = "a window cannot be moved")]
fn moving_a_window_panics() {
    let mut app = app();
    let first = app.spawn(app.root(), a_window());
    let second = app.spawn(app.root(), a_window());
    app.flush();
    app.move_to(first, second);
    app.flush();
}

// ── 3: Windows ──────────────────────────────────────────────────────────

fn windows(app: &App) -> Vec<NodeId> {
    app.resource::<Windows>().iter().collect()
}

#[test]
fn windows_lists_live_windows_in_spawn_order_and_signals_only_on_change() {
    let mut app = app();
    let a = app.spawn(app.root(), a_window());
    let b = app.spawn(app.root(), a_window());
    let leaf = app.spawn(a, Leaf);
    app.tick();
    assert_eq!(windows(&app), vec![a.id(), b.id()]);
    assert_eq!(app.resource::<Windows>().len(), 2);
    assert!(app.resource::<Windows>().contains(a.id()));
    assert_eq!(take_windows_changed(), 1, "the two pushes, drained once");

    assert!(app.remove(leaf));
    app.tick();
    assert_eq!(windows(&app), vec![a.id(), b.id()]);
    assert_eq!(
        take_windows_changed(),
        0,
        "a non-window removal writes nothing"
    );

    assert!(app.remove(a));
    app.tick();
    assert_eq!(windows(&app), vec![b.id()]);
    assert_eq!(take_windows_changed(), 1);

    app.tick();
    assert_eq!(take_windows_changed(), 0, "nothing changed");
}

#[test]
fn removing_a_subtree_under_a_window_and_removing_two_windows_at_once() {
    let mut app = app();
    let a = app.spawn(app.root(), a_window());
    let b = app.spawn(app.root(), a_window());
    let panel = app.spawn(a, PanelBuilder);
    app.tick();
    assert_eq!(windows(&app), vec![a.id(), b.id()]);
    take_windows_changed();

    assert!(app.remove(panel));
    app.tick();
    assert_eq!(windows(&app), vec![a.id(), b.id()]);
    assert_eq!(take_windows_changed(), 0, "a subtree with no window in it");

    assert!(app.remove(a));
    assert!(app.remove(b));
    app.tick();
    assert!(windows(&app).is_empty());
    assert!(app.resource::<Windows>().is_empty());
    assert_eq!(take_windows_changed(), 1, "two removals, one drain");
}

// ── 4, 5, 6, 9: the loop ────────────────────────────────────────────────

#[test]
fn requests_coalesce_into_one_frame_requested_and_frame_clears_pending() {
    let mut app = app_closed();
    let win = app.spawn(app.root(), a_window());
    app.tick();

    app.signal(RequestFrame(win.id()));
    app.flush();
    assert_eq!(take_requested(), vec![win.id()]);
    assert_eq!(take_frames(), vec![win.id()]);
    assert!(!app.widget::<Window>(win).unwrap().is_pending());

    app.signal(RequestFrame(win.id()));
    app.signal(RequestFrame(win.id()));
    app.signal(RequestFrame(win.id()));
    app.flush();
    assert_eq!(
        take_requested(),
        vec![win.id()],
        "three requests, one cycle"
    );
    assert_eq!(take_frames(), vec![win.id()]);
}

#[test]
fn a_request_while_pending_is_swallowed_until_the_frame_comes_back() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    app.tick();

    app.signal(RequestFrame(win.id()));
    app.flush();
    assert_eq!(take_requested(), vec![win.id()]);
    assert!(app.widget::<Window>(win).unwrap().is_pending());

    app.signal(RequestFrame(win.id()));
    app.flush();
    assert!(take_requested().is_empty(), "pending: nothing sent");

    app.signal(Frame(win.id()));
    app.flush();
    assert_eq!(take_frames(), vec![win.id()]);
    assert!(!app.widget::<Window>(win).unwrap().is_pending());

    app.signal(RequestFrame(win.id()));
    app.flush();
    assert_eq!(take_requested(), vec![win.id()], "a new cycle");
}

thread_local! {
    /// Set by a test that wants `rerequest` to fire once.
    static REREQUEST_ONCE: RefCell<bool> = const { RefCell::new(false) };
}

/// A stand-in for animation: on `Frame`, ask for another, once.
fn rerequest(app: &mut App, f: &Frame) {
    let fire = REREQUEST_ONCE.with(|b| std::mem::take(&mut *b.borrow_mut()));
    if fire {
        app.signal(RequestFrame(f.0));
    }
}

#[test]
fn a_request_raised_by_a_frame_system_opens_the_next_cycle_in_the_same_flush() {
    let mut app = app();
    app.system(rerequest);
    let win = app.spawn(app.root(), a_window());
    app.tick();

    app.signal(RequestFrame(win.id()));
    app.flush();
    assert_eq!(take_requested(), vec![win.id()]);

    REREQUEST_ONCE.with(|b| *b.borrow_mut() = true);
    app.signal(Frame(win.id()));
    app.flush();
    assert_eq!(take_frames(), vec![win.id()]);
    assert_eq!(
        take_requested(),
        vec![win.id()],
        "the Frame cleared pending, then the re-request opened a cycle"
    );
    assert!(app.widget::<Window>(win).unwrap().is_pending());
}

#[test]
fn stale_and_non_window_ids_are_ignored_and_a_frame_when_not_pending_is_harmless() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let leaf = app.spawn(win, Leaf);
    let gone = app.spawn(app.root(), a_window());
    app.tick();
    assert!(app.remove(gone));
    app.flush();

    app.signal(RequestFrame(gone.id()));
    app.signal(RequestFrame(leaf.id()));
    app.signal(Frame(gone.id()));
    app.signal(Frame(leaf.id()));
    app.flush();
    assert!(take_requested().is_empty());
    assert_eq!(
        take_frames(),
        vec![gone.id(), leaf.id()],
        "logged, but ignored by window"
    );

    app.signal(Frame(win.id()));
    app.flush();
    assert!(!app.widget::<Window>(win).unwrap().is_pending());
    app.signal(RequestFrame(win.id()));
    app.flush();
    assert_eq!(take_requested(), vec![win.id()], "still works afterwards");
}

// ── 7, 8: the WSI's events ──────────────────────────────────────────────

#[test]
fn resized_rewrites_the_style_only_when_the_size_differs() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    app.tick();
    take_done();

    app.emit(
        Resized {
            size: Size::new(640.0, 360.0),
        },
        win,
    );
    app.tick();
    let style = app.component::<LayoutStyle>(win).unwrap();
    assert_eq!((style.width, style.height), (px(640.0), px(360.0)));
    assert_eq!(
        app.component::<ComputedLayout>(win).unwrap().rect,
        Rect::new(0.0, 0.0, 640.0, 360.0)
    );
    assert_eq!(take_done(), vec![vec![win.id()]], "relaid out that tick");

    app.emit(
        Resized {
            size: Size::new(640.0, 360.0),
        },
        win,
    );
    app.tick();
    assert_eq!(
        take_done(),
        vec![vec![]],
        "same size: no style write, nothing dirty"
    );
    assert!(
        take_requested().is_empty(),
        "Resized never asks for a frame"
    );
}

#[test]
fn scale_factor_changed_stores_the_scale_and_asks_for_a_frame_once() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    app.tick();

    app.emit(ScaleFactorChanged { scale: 2.0 }, win);
    app.flush();
    assert_eq!(app.widget::<Window>(win).unwrap().scale(), 2.0);
    assert_eq!(take_requested(), vec![win.id()]);

    app.signal(Frame(win.id()));
    app.flush();
    app.emit(ScaleFactorChanged { scale: 2.0 }, win);
    app.flush();
    assert_eq!(app.widget::<Window>(win).unwrap().scale(), 2.0);
    assert!(
        take_requested().is_empty(),
        "the same scale again asks nothing"
    );
}

/// A spawn site's own handler on `CloseRequested`: removes the window. A
/// `Spawner` cannot name the app root, so the window is spawned by the
/// test and handed to the builder; `Spawner::on` accepts any live target.
struct Closer;
struct CloserBuilder {
    win: Handle<Window>,
}
impl Build for CloserBuilder {
    type Widget = Closer;
}
impl Widget for Closer {
    type Builder = CloserBuilder;
    fn build(b: CloserBuilder, _: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        let win = b.win;
        s.on::<CloseRequested>(win, move |ctx, _| {
            ctx.remove(win);
        });
        Closer
    }
}

#[test]
fn close_requested_is_the_spawn_sites_to_handle() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    app.spawn(app.root(), CloserBuilder { win });
    app.tick();
    assert_eq!(windows(&app), vec![win.id()]);

    app.emit(CloseRequested, win);
    app.tick();
    assert!(!app.is_live(win));
    assert!(windows(&app).is_empty());
}

// ── the clear colour ─────────────────────────────────────────────────────

#[test]
fn a_window_clears_to_black_unless_told_otherwise() {
    let mut app = app();
    let plain = app.spawn(app.root(), a_window());
    let tinted = app.spawn(app.root(), a_window().clear(Color::from_rgb8(10, 20, 30)));
    assert_eq!(app.widget::<Window>(plain).unwrap().clear(), Color::BLACK);
    assert_eq!(
        app.widget::<Window>(tinted).unwrap().clear(),
        Color::from_rgb8(10, 20, 30)
    );
}

#[test]
fn a_clear_with_no_alpha_is_transparent_whatever_its_rgb() {
    let mut app = app();
    let w = app.spawn(
        app.root(),
        a_window().clear(Color::rgba(1.0, 0.5, 0.25, 0.0)),
    );
    assert_eq!(
        app.widget::<Window>(w).unwrap().clear(),
        Color::TRANSPARENT,
        "its rgb would otherwise leak into every edge composited onto it"
    );
}
