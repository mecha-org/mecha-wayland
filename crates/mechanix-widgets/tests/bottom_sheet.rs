//! Integration tests for the Mechanix bottom sheet, through its public
//! API: layout, theme, setters, and closing (drag, scrim) driven by real
//! contacts through a window.

use app::{App, Build, Context, Event, Handle, NodeId, Spawner, Widget};
use geometry::{Color, Corners, Point, Rect};
use interactivity::{ContactId, ContactInput, ContactPhase, InteractivityModule, PressCancelled};
use layout::{ComputedLayout, LayoutStyle, percent, px};
use mechanix_widgets::bottom_sheet::{
    DRAG_SLOP, GRIP_HEIGHT, HANDLE_HEIGHT, HANDLE_WIDTH, SCRIM_OPACITY,
};
use mechanix_widgets::{
    BottomSheet, BottomSheetBuilder, BottomSheetContextExt, ColorSource, FontBook, SheetClosed,
    Text, bottom_sheet,
};
use paint::{Paint, Quad};
use theme::{AppThemeExt, ColorRole, MechanixTheme, ThemeReader};
use widgets::div;
use window::{WindowModule, window};

const W: f32 = 400.0;
const H: f32 = 800.0;

/// An app that lays out, paints and takes contacts, with a font, and
/// a `W` x `H` window to hold a sheet.
fn sheet_app() -> (App, NodeId) {
    let mut app = App::new();
    app.add_module(layout::LayoutModule)
        .add_module(paint::PaintModule)
        .add_module(WindowModule)
        .add_module(InteractivityModule)
        .add_module(MechanixTheme::light());
    let mut atlas = atlas::Atlas::new();
    let font = atlas
        .add_font(include_bytes!(
            "../../../crates/atlas/tests/fixtures/Inter-Regular.ttf"
        ))
        .unwrap();
    app.insert_resource(atlas);
    app.insert_resource(FontBook::new(font));
    let win = app
        .spawn(
            app.root(),
            window().layout(LayoutStyle::default().size(px(W), px(H))),
        )
        .id();
    (app, win)
}

fn settle(app: &mut App) {
    for _ in 0..4 {
        app.tick();
    }
}

fn rect(app: &App, id: impl Into<NodeId>) -> Rect {
    app.component::<ComputedLayout>(id).unwrap().rect
}

fn sheet(app: &App, h: Handle<BottomSheet>) -> &BottomSheet {
    app.widget::<BottomSheet>(h).unwrap()
}

fn surface_rect(app: &App, h: Handle<BottomSheet>) -> Rect {
    rect(app, sheet(app, h).surface())
}

fn surface_height(app: &App, h: Handle<BottomSheet>) -> f32 {
    surface_rect(app, h).height()
}

fn quad(app: &App, id: impl Into<NodeId>) -> Quad {
    match app.component::<Paint>(id).unwrap() {
        Paint::Quad(q) => *q,
        other => panic!("expected a quad, got {other:?}"),
    }
}

fn title_string(app: &App, h: Handle<BottomSheet>) -> String {
    let text = app.widget::<Text>(sheet(app, h).title()).unwrap().native();
    app.widget::<widgets::Text>(text).unwrap().string.clone()
}

/// A fixed-size box, standing in for caller content.
fn block(height: f32) -> widgets::DivBuilder {
    div().style(LayoutStyle::default().size(px(100.0), px(height)))
}

// ── builder ─────────────────────────────────────────────────────────

// ── layout ──────────────────────────────────────────────────────────

#[test]
fn surface_holds_grip_header_body_footer_in_order() {
    let (mut app, win) = sheet_app();
    let h = app.spawn(win, bottom_sheet().title("Title"));
    let s = sheet(&app, h).clone();
    let children = app.children(s.surface()).unwrap().to_vec();
    assert_eq!(
        children,
        vec![
            s.grip().id(),
            s.header().id(),
            s.body().id(),
            s.footer().id()
        ]
    );
    assert_eq!(app.parent(s.handle()), Some(s.grip().id()));
    assert_eq!(app.parent(s.title()), Some(s.header().id()));
    assert_eq!(title_string(&app, h), "Title");
}

#[test]
fn auto_height_sits_at_the_bottom_and_fits_its_content() {
    let (mut app, win) = sheet_app();
    let h = app.spawn(win, bottom_sheet().title("Title"));
    let body = sheet(&app, h).body();
    settle(&mut app);
    let empty = surface_rect(&app, h);
    assert_eq!(empty.width(), W);
    assert_eq!(empty.bottom(), H, "pinned to the bottom");
    assert!(empty.height() < H, "fits, not full height");

    app.spawn(body, block(120.0));
    settle(&mut app);
    let filled = surface_rect(&app, h);
    assert_eq!(filled.height(), empty.height() + 120.0);
    assert_eq!(filled.bottom(), H);
}

#[test]
fn auto_height_stops_at_the_max_height() {
    let (mut app, win) = sheet_app();
    let h = app.spawn(win, bottom_sheet());
    let body = sheet(&app, h).body();
    app.spawn(body, block(H * 2.0));
    settle(&mut app);
    assert_eq!(surface_height(&app, h), H);

    let (mut app, win) = sheet_app();
    let h = app.spawn(win, bottom_sheet().max_height(percent(75.0)));
    let body = sheet(&app, h).body();
    app.spawn(body, block(H * 2.0));
    settle(&mut app);
    assert_eq!(surface_height(&app, h), H * 0.75);
}

#[test]
fn height_takes_percent_and_px() {
    for (height, expected) in [
        (percent(50.0), H / 2.0),
        (percent(100.0), H),
        (px(300.0), 300.0),
    ] {
        let (mut app, win) = sheet_app();
        let h = app.spawn(win, bottom_sheet().height(height));
        settle(&mut app);
        let r = surface_rect(&app, h);
        assert_eq!((r.height(), r.bottom()), (expected, H), "{height:?}");
    }
}

#[test]
fn a_narrower_sheet_is_centered() {
    for (width, expected) in [(percent(50.0), W / 2.0), (px(320.0), 320.0)] {
        let (mut app, win) = sheet_app();
        let h = app.spawn(win, bottom_sheet().width(width));
        settle(&mut app);
        let r = surface_rect(&app, h);
        assert_eq!(r.width(), expected, "{width:?}");
        assert_eq!(r.x(), (W - expected) / 2.0, "{width:?}");
    }
}

#[test]
fn footer_stays_at_the_bottom_of_a_tall_sheet() {
    let (mut app, win) = sheet_app();
    let h = app.spawn(win, bottom_sheet().height(percent(100.0)));
    let (body, footer) = (sheet(&app, h).body(), sheet(&app, h).footer());
    app.spawn(body, block(40.0));
    app.spawn(footer, block(200.0));
    settle(&mut app);
    let f = rect(&app, footer);
    assert_eq!((f.height(), f.bottom()), (200.0, H));
    assert_eq!(f.width(), W, "edge to edge");
}

#[test]
fn empty_footer_takes_no_space() {
    let (mut app, win) = sheet_app();
    let h = app.spawn(win, bottom_sheet());
    settle(&mut app);
    assert_eq!(rect(&app, sheet(&app, h).footer()).height(), 0.0);
}

#[test]
fn handle_is_a_centered_pill_and_can_be_hidden() {
    let (mut app, win) = sheet_app();
    let h = app.spawn(win, bottom_sheet());
    settle(&mut app);
    let pill = rect(&app, sheet(&app, h).handle());
    assert_eq!((pill.width(), pill.height()), (HANDLE_WIDTH, HANDLE_HEIGHT));
    assert_eq!(pill.x() + pill.width() / 2.0, W / 2.0);
    let grip = rect(&app, sheet(&app, h).grip());
    assert_eq!((grip.width(), grip.height()), (W, GRIP_HEIGHT));
    let with_handle = surface_height(&app, h);

    let (mut app, win) = sheet_app();
    let h = app.spawn(win, bottom_sheet().handle(false));
    settle(&mut app);
    assert_eq!(with_handle - surface_height(&app, h), GRIP_HEIGHT);
}

// ── colors ──────────────────────────────────────────────────────────

#[test]
fn surface_takes_theme_colors_and_rounded_top_corners() {
    let (mut app, win) = sheet_app();
    let h = app.spawn(win, bottom_sheet());
    let s = sheet(&app, h).clone();
    let surface = quad(&app, s.surface());
    assert_eq!(surface.color, app.color(ColorRole::SurfaceContainerHighest));
    assert_eq!(surface.radii, Corners::new(28.0, 28.0, 0.0, 0.0));
    assert_eq!(
        quad(&app, s.handle()).color,
        app.color(ColorRole::OnSurface)
    );
}

#[test]
fn theme_change_repaints_the_surface() {
    let (mut app, win) = sheet_app();
    let h = app.spawn(win, bottom_sheet());
    let surface = sheet(&app, h).surface();
    let light = quad(&app, surface).color;
    app.set_theme(MechanixTheme::dark());
    settle(&mut app);
    let q = quad(&app, surface);
    assert_ne!(q.color, light);
    assert_eq!(q.color, app.color(ColorRole::SurfaceContainerHighest));
    assert_eq!(q.radii, Corners::new(28.0, 28.0, 0.0, 0.0));
}

#[test]
fn overrides_beat_the_theme() {
    let (mut app, win) = sheet_app();
    let h = app.spawn(
        win,
        bottom_sheet()
            .background(Color::BLACK)
            .handle_color(ColorRole::Primary),
    );
    let s = sheet(&app, h).clone();
    assert_eq!(quad(&app, s.surface()).color, Color::BLACK);
    assert_eq!(quad(&app, s.handle()).color, app.color(ColorRole::Primary));
}

// ── context setters ─────────────────────────────────────────────────

struct Poke;
impl Event for Poke {}

type Action = Box<dyn FnMut(&mut Context<'_, Controller>)>;

/// Runs its closure on a `Poke`, so a test can reach a `Context`.
struct Controller;
struct ControllerBuilder(Action);
impl Build for ControllerBuilder {
    type Widget = Controller;
}
impl Widget for Controller {
    type Builder = ControllerBuilder;
    fn build(b: ControllerBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        let mut action = b.0;
        s.on::<Poke>(me, move |ctx, _| action(ctx));
        Controller
    }
}

/// Run `f` on the sheet's `Context`, then let layout settle.
fn with_sheet(
    app: &mut App,
    h: Handle<BottomSheet>,
    mut f: impl FnMut(&mut Context<'_, BottomSheet>) + 'static,
) {
    let controller = app.spawn(
        app.root(),
        ControllerBuilder(Box::new(move |ctx: &mut Context<'_, Controller>| {
            f(&mut ctx.at(h).unwrap());
        })),
    );
    app.emit(Poke, controller.id());
    app.flush();
    settle(app);
    app.remove(controller);
}

#[test]
fn set_title_changes_the_header() {
    let (mut app, win) = sheet_app();
    let h = app.spawn(win, bottom_sheet().title("Before"));
    with_sheet(&mut app, h, |c| c.set_title("After"));
    assert_eq!(title_string(&app, h), "After");
}

#[test]
fn size_setters_resize_the_surface() {
    let (mut app, win) = sheet_app();
    let h = app.spawn(win, bottom_sheet());
    settle(&mut app);
    with_sheet(&mut app, h, |c| {
        c.set_sheet_height(percent(100.0));
        c.set_sheet_width(percent(50.0));
    });
    assert_eq!(sheet(&app, h).size().height, percent(100.0));
    let r = surface_rect(&app, h);
    assert_eq!((r.width(), r.height()), (W / 2.0, H));

    with_sheet(&mut app, h, |c| c.set_sheet_max_height(px(500.0)));
    assert_eq!(surface_height(&app, h), 500.0);
    with_sheet(&mut app, h, |c| {
        c.set_sheet_height(px(50.0));
        c.set_sheet_min_height(px(200.0));
    });
    assert_eq!(surface_height(&app, h), 200.0);
}

#[test]
fn set_handle_visible_toggles_the_grip() {
    let (mut app, win) = sheet_app();
    let h = app.spawn(win, bottom_sheet());
    settle(&mut app);
    let shown = surface_height(&app, h);
    with_sheet(&mut app, h, |c| c.set_handle_visible(false));
    assert!(!sheet(&app, h).is_handle_visible());
    assert_eq!(shown - surface_height(&app, h), GRIP_HEIGHT);
    with_sheet(&mut app, h, |c| c.set_handle_visible(true));
    assert_eq!(surface_height(&app, h), shown);
}

#[test]
fn color_setters_repaint_and_none_restores_the_theme() {
    let (mut app, win) = sheet_app();
    let h = app.spawn(win, bottom_sheet());
    let s = sheet(&app, h).clone();
    with_sheet(&mut app, h, |c| {
        c.set_background(Color::BLACK);
        c.set_handle_color(ColorRole::Primary);
        c.set_title_color(ColorRole::Error);
    });
    assert_eq!(quad(&app, s.surface()).color, Color::BLACK);
    assert_eq!(
        quad(&app, s.surface()).radii,
        Corners::new(28.0, 28.0, 0.0, 0.0)
    );
    assert_eq!(quad(&app, s.handle()).color, app.color(ColorRole::Primary));
    let native = app.widget::<Text>(s.title()).unwrap().native();
    assert_eq!(
        app.widget::<widgets::Text>(native).unwrap().color,
        app.color(ColorRole::Error)
    );

    with_sheet(&mut app, h, |c| c.set_background(None::<ColorSource>));
    assert_eq!(
        quad(&app, s.surface()).color,
        app.color(ColorRole::SurfaceContainerHighest)
    );
}

// ── a parent filling the slots ──────────────────────────────────────

/// Spawns a sheet and fills its body during its own build; records
/// what the sheet reports.
struct Host {
    sheet: Handle<BottomSheet>,
    closed: usize,
    title_cancelled: bool,
}
struct HostBuilder(BottomSheetBuilder);
impl Build for HostBuilder {
    type Widget = Host;
}
impl Widget for Host {
    type Builder = HostBuilder;
    fn build(b: HostBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        *s.component_mut::<LayoutStyle>(me).unwrap() =
            LayoutStyle::default().size(percent(100.0), percent(100.0));
        let sheet = s.spawn(me, b.0);
        let (body, title) = {
            let w = s.widget::<BottomSheet>(sheet).unwrap();
            (w.body(), w.title())
        };
        s.spawn(body, block(100.0));
        s.on::<SheetClosed>(sheet, |ctx, _| ctx.me().closed += 1);
        s.on::<PressCancelled>(title, |ctx, _| ctx.me().title_cancelled = true);
        Host {
            sheet,
            closed: 0,
            title_cancelled: false,
        }
    }
}

#[test]
fn a_parent_fills_the_body_during_its_build() {
    let (mut app, win) = sheet_app();
    let host = app.spawn(win, HostBuilder(bottom_sheet()));
    let sheet_h = app.widget::<Host>(host).unwrap().sheet;
    let body = sheet(&app, sheet_h).body();
    assert_eq!(app.children(body).map(<[_]>::len), Some(1));
}

// ── drag to dismiss ─────────────────────────────────────────────────

const FINGER: ContactId = ContactId::Touch(0);

fn contact(app: &mut App, win: NodeId, phase: ContactPhase, x: f32, y: f32, time: u32) {
    app.signal(ContactInput {
        window: win,
        contact: FINGER,
        phase,
        position: Point::new(x, y),
        time,
    });
    settle(app);
}

/// A finger down at `x` and the first `(y, time)`, then through the
/// rest, still down.
fn press_and_move(app: &mut App, win: NodeId, x: f32, points: &[(f32, u32)]) {
    let (y0, t0) = points[0];
    contact(app, win, ContactPhase::Moved, x, y0, t0);
    contact(app, win, ContactPhase::Pressed, x, y0, t0);
    for &(y, t) in &points[1..] {
        contact(app, win, ContactPhase::Moved, x, y, t);
    }
}

/// [`press_and_move`], lifting at the last point.
fn gesture(app: &mut App, win: NodeId, x: f32, points: &[(f32, u32)]) {
    press_and_move(app, win, x, points);
    let (y, t) = *points.last().unwrap();
    contact(app, win, ContactPhase::Released, x, y, t);
}

/// The `(y, time)` points from `from` through each y in `path`, `step`
/// ms apart.
fn timed(from: Point, path: &[f32], step: u32) -> Vec<(f32, u32)> {
    let mut points = vec![(from.y, 1_000)];
    for (i, &y) in path.iter().enumerate() {
        points.push((y, 1_000 + step * (i as u32 + 1)));
    }
    points
}

/// A slow drag: a second between moves, far too slow to flick.
fn drag(app: &mut App, win: NodeId, from: Point, path: &[f32]) {
    gesture(app, win, from.x, &timed(from, path, 1_000));
}

/// A fast drag: 10 ms between moves.
fn flick(app: &mut App, win: NodeId, from: Point, path: &[f32]) {
    gesture(app, win, from.x, &timed(from, path, 10));
}

/// The middle of `id`'s rect.
fn centre(app: &App, id: impl Into<NodeId>) -> Point {
    let r = rect(app, id);
    Point::new(r.x() + r.width() / 2.0, r.y() + r.height() / 2.0)
}

fn grip_centre(app: &App, h: Handle<BottomSheet>) -> Point {
    centre(app, sheet(app, h).grip())
}

/// How many `SheetClosed` the host has seen.
fn closed(app: &App, host: Handle<Host>) -> usize {
    app.widget::<Host>(host).unwrap().closed
}

/// A hosted sheet, laid out.
fn hosted(b: BottomSheetBuilder) -> (App, NodeId, Handle<Host>, Handle<BottomSheet>) {
    let (mut app, win) = sheet_app();
    let host = app.spawn(win, HostBuilder(b));
    let h = app.widget::<Host>(host).unwrap().sheet;
    settle(&mut app);
    (app, win, host, h)
}

// The hosted sheet is its grip, header and a padded 100 px body: 220
// px tall, so a slow drag must pass 110 px to dismiss it.

#[test]
fn the_sheet_slides_with_the_finger_without_resizing() {
    let (mut app, win, _, h) = hosted(bottom_sheet());
    let rest = surface_rect(&app, h);
    let at = grip_centre(&app, h);
    // Crossing the slop (at +20) moves nothing; the next 60 px do.
    press_and_move(
        &mut app,
        win,
        at.x,
        &timed(at, &[at.y + 20.0, at.y + 80.0], 1_000),
    );
    let moved = surface_rect(&app, h);
    assert!(sheet(&app, h).is_dragging());
    assert_eq!(moved.y(), rest.y() + 60.0);
    assert_eq!(moved.height(), rest.height(), "slides, not shrinks");
}

#[test]
fn a_short_drag_down_returns_to_rest() {
    let (mut app, win, host, h) = hosted(bottom_sheet());
    let rest = surface_rect(&app, h);
    let at = grip_centre(&app, h);
    drag(&mut app, win, at, &[at.y + 20.0, at.y + 120.0]);
    assert_eq!(surface_rect(&app, h), rest);
    assert!(!sheet(&app, h).is_closed());
    assert!(!sheet(&app, h).is_dragging());
    assert_eq!(closed(&app, host), 0);
}

#[test]
fn a_drag_past_half_the_height_closes() {
    let (mut app, win, host, h) = hosted(bottom_sheet());
    let at = grip_centre(&app, h);
    drag(&mut app, win, at, &[at.y + 20.0, at.y + 150.0]);
    assert!(sheet(&app, h).is_closed());
    assert_eq!(closed(&app, host), 1);
    assert!(surface_rect(&app, h).y() >= H, "pushed off the bottom edge");
}

#[test]
fn a_flick_down_closes() {
    let (mut app, win, host, h) = hosted(bottom_sheet());
    let at = grip_centre(&app, h);
    // 10 px every 10 ms: 1000 px/s, short of half the height.
    flick(
        &mut app,
        win,
        at,
        &[at.y + 20.0, at.y + 30.0, at.y + 40.0, at.y + 50.0],
    );
    assert_eq!(closed(&app, host), 1);
}

#[test]
fn one_jittery_last_sample_is_not_a_flick() {
    let (mut app, win, host, h) = hosted(bottom_sheet());
    let rest = surface_rect(&app, h);
    let at = grip_centre(&app, h);
    // A slow drag (100 px/s), then a 5 px jump in 1 ms just before
    // lifting: on its own that pair is 5000 px/s.
    let y = at.y;
    gesture(
        &mut app,
        win,
        at.x,
        &[
            (y, 1_000),
            (y + 20.0, 1_200),
            (y + 30.0, 1_300),
            (y + 40.0, 1_400),
            (y + 50.0, 1_500),
            (y + 55.0, 1_501),
        ],
    );
    assert_eq!(closed(&app, host), 0);
    assert_eq!(surface_rect(&app, h), rest);
}

#[test]
fn dragging_the_header_closes_too() {
    let (mut app, win, host, h) = hosted(bottom_sheet());
    let at = centre(&app, sheet(&app, h).header());
    drag(&mut app, win, at, &[at.y + 20.0, at.y + 150.0]);
    assert_eq!(closed(&app, host), 1);
}

#[test]
fn a_pause_before_lifting_is_not_a_flick() {
    let (mut app, win, host, h) = hosted(bottom_sheet());
    let rest = surface_rect(&app, h);
    let at = grip_centre(&app, h);
    let down = at.y + 50.0;
    // Fast moves, then the finger rests (no moves come) and lifts half a
    // second later.
    press_and_move(
        &mut app,
        win,
        at.x,
        &[(at.y, 1_000), (at.y + 20.0, 1_010), (down, 1_020)],
    );
    contact(&mut app, win, ContactPhase::Released, at.x, down, 1_520);
    assert_eq!(closed(&app, host), 0);
    assert_eq!(surface_rect(&app, h), rest);
}

#[test]
fn close_before_the_first_layout_still_hides_the_sheet() {
    let (mut app, win) = sheet_app();
    let host = app.spawn(win, HostBuilder(bottom_sheet()));
    let h = app.widget::<Host>(host).unwrap().sheet;
    // Never laid out yet: the sheet has no height to measure.
    with_sheet(&mut app, h, |c| c.close());
    assert!(sheet(&app, h).is_closed());
    assert!(surface_rect(&app, h).y() >= H, "below the bottom edge");
}

#[test]
fn a_closed_sheet_stays_hidden_when_its_content_grows() {
    let (mut app, _, _, h) = hosted(bottom_sheet());
    with_sheet(&mut app, h, |c| c.close());
    let body = sheet(&app, h).body();
    app.spawn(body, block(300.0));
    settle(&mut app);
    assert!(surface_rect(&app, h).y() >= H, "below the bottom edge");
}

#[test]
fn a_cancelled_drag_returns_to_rest() {
    let (mut app, win, host, h) = hosted(bottom_sheet());
    let rest = surface_rect(&app, h);
    let at = grip_centre(&app, h);
    let points = timed(at, &[at.y + 20.0, at.y + 200.0], 10);
    press_and_move(&mut app, win, at.x, &points);
    contact(
        &mut app,
        win,
        ContactPhase::Cancelled,
        at.x,
        at.y + 200.0,
        1_030,
    );
    assert_eq!(surface_rect(&app, h), rest);
    assert_eq!(closed(&app, host), 0);
}

#[test]
fn dragging_up_moves_nothing_and_takes_nothing() {
    let (mut app, win, host, h) = hosted(bottom_sheet().title("Title"));
    let rest = surface_rect(&app, h);
    let at = centre(&app, sheet(&app, h).title());
    drag(&mut app, win, at, &[at.y - 20.0, at.y - 120.0]);
    assert_eq!(surface_rect(&app, h), rest);
    assert!(!app.widget::<Host>(host).unwrap().title_cancelled);
}

#[test]
fn the_sheet_never_rises_above_rest() {
    let (mut app, win, _, h) = hosted(bottom_sheet());
    let rest = surface_rect(&app, h);
    let at = grip_centre(&app, h);
    press_and_move(
        &mut app,
        win,
        at.x,
        &timed(at, &[at.y + 20.0, at.y - 100.0], 1_000),
    );
    assert_eq!(surface_rect(&app, h), rest);
}

#[test]
fn a_move_within_the_slop_changes_nothing() {
    let (mut app, win, host, h) = hosted(bottom_sheet());
    let rest = surface_rect(&app, h);
    let at = grip_centre(&app, h);
    drag(&mut app, win, at, &[at.y + DRAG_SLOP]);
    assert_eq!(surface_rect(&app, h), rest);
    assert_eq!(closed(&app, host), 0);
}

#[test]
fn a_drag_in_the_body_changes_nothing() {
    let (mut app, win, host, h) = hosted(bottom_sheet());
    let rest = surface_rect(&app, h);
    let at = centre(&app, sheet(&app, h).body());
    flick(&mut app, win, at, &[at.y + 20.0, at.y + 150.0]);
    assert_eq!(surface_rect(&app, h), rest);
    assert_eq!(closed(&app, host), 0);
}

#[test]
fn a_sheet_that_is_not_dismissible_does_not_move() {
    let (mut app, win, host, h) = hosted(bottom_sheet().dismissible(false));
    let rest = surface_rect(&app, h);
    let at = grip_centre(&app, h);
    flick(&mut app, win, at, &[at.y + 20.0, at.y + 150.0]);
    assert_eq!(surface_rect(&app, h), rest);
    assert_eq!(closed(&app, host), 0);
}

#[test]
fn a_drag_takes_the_press_from_the_header_content() {
    let (mut app, win, host, h) = hosted(bottom_sheet().title("Title"));
    let at = centre(&app, sheet(&app, h).title());
    drag(&mut app, win, at, &[at.y + 20.0, at.y + 60.0]);
    assert!(app.widget::<Host>(host).unwrap().title_cancelled);
}

#[test]
fn a_tap_on_the_header_content_is_left_alone() {
    let (mut app, win, host, h) = hosted(bottom_sheet().title("Title"));
    let at = centre(&app, sheet(&app, h).title());
    drag(&mut app, win, at, &[at.y]);
    assert!(!app.widget::<Host>(host).unwrap().title_cancelled);
}

#[test]
fn a_closed_sheet_takes_no_drags_until_opened() {
    let (mut app, win, host, h) = hosted(bottom_sheet());
    let rest = surface_rect(&app, h);
    let at = grip_centre(&app, h);
    drag(&mut app, win, at, &[at.y + 20.0, at.y + 150.0]);
    assert!(sheet(&app, h).is_closed());

    // Where the grip used to be: nothing there takes the drag now.
    flick(&mut app, win, at, &[at.y + 20.0, at.y + 150.0]);
    assert_eq!(closed(&app, host), 1);

    with_sheet(&mut app, h, |c| c.open());
    assert!(!sheet(&app, h).is_closed());
    assert_eq!(surface_rect(&app, h), rest);
    drag(&mut app, win, at, &[at.y + 20.0, at.y + 150.0]);
    assert_eq!(closed(&app, host), 2, "draggable again");
}

#[test]
fn close_and_open_from_code_move_the_sheet_silently() {
    let (mut app, win, host, h) = hosted(bottom_sheet());
    let rest = surface_rect(&app, h);
    with_sheet(&mut app, h, |c| c.close());
    assert!(sheet(&app, h).is_closed());
    assert!(surface_rect(&app, h).y() >= H, "pushed off the bottom edge");
    assert_eq!(closed(&app, host), 0, "the owner already knows");

    // Closed: the grip's old spot takes no drag.
    let at = Point::new(rest.x() + rest.width() / 2.0, rest.y() + GRIP_HEIGHT / 2.0);
    flick(&mut app, win, at, &[at.y + 20.0, at.y + 150.0]);
    assert_eq!(closed(&app, host), 0);

    with_sheet(&mut app, h, |c| c.open());
    assert!(!sheet(&app, h).is_closed());
    assert_eq!(surface_rect(&app, h), rest);
    assert_eq!(closed(&app, host), 0);
}

#[test]
fn open_and_close_twice_are_harmless() {
    let (mut app, _, _, h) = hosted(bottom_sheet());
    let rest = surface_rect(&app, h);
    with_sheet(&mut app, h, |c| c.open());
    assert_eq!(surface_rect(&app, h), rest, "already open");
    with_sheet(&mut app, h, |c| {
        c.close();
        c.close();
    });
    assert!(sheet(&app, h).is_closed());
    with_sheet(&mut app, h, |c| {
        c.open();
        c.open();
    });
    assert_eq!(surface_rect(&app, h), rest);
}

#[test]
fn close_works_on_a_sheet_that_is_not_dismissible() {
    let (mut app, _, _, h) = hosted(bottom_sheet().dismissible(false));
    with_sheet(&mut app, h, |c| c.close());
    assert!(sheet(&app, h).is_closed());
}

#[test]
fn close_mid_drag_ends_the_drag_without_an_event() {
    let (mut app, win, host, h) = hosted(bottom_sheet());
    let at = grip_centre(&app, h);
    press_and_move(
        &mut app,
        win,
        at.x,
        &timed(at, &[at.y + 20.0, at.y + 80.0], 1_000),
    );
    with_sheet(&mut app, h, |c| c.close());
    assert!(!sheet(&app, h).is_dragging());
    // The finger lifting afterwards changes nothing more.
    contact(
        &mut app,
        win,
        ContactPhase::Released,
        at.x,
        at.y + 80.0,
        9_000,
    );
    assert!(sheet(&app, h).is_closed());
    assert_eq!(closed(&app, host), 0);
}

#[test]
fn turning_dismissible_off_mid_drag_returns_to_rest() {
    let (mut app, win, _, h) = hosted(bottom_sheet());
    let rest = surface_rect(&app, h);
    let at = grip_centre(&app, h);
    press_and_move(
        &mut app,
        win,
        at.x,
        &timed(at, &[at.y + 20.0, at.y + 80.0], 1_000),
    );
    with_sheet(&mut app, h, |c| c.set_dismissible(false));
    assert_eq!(surface_rect(&app, h), rest);
    assert!(!sheet(&app, h).is_dragging());
}

// ── header ──────────────────────────────────────────────────────

/// A press and release at `at`.
fn tap(app: &mut App, win: NodeId, at: Point) {
    gesture(app, win, at.x, &[(at.y, 1_000)]);
}

#[test]
fn the_title_starts_at_the_header_padding() {
    let (app, _, _, h) = hosted(bottom_sheet().title("Title"));
    assert_eq!(rect(&app, sheet(&app, h).title()).x(), 24.0);
}

// ── scrim ───────────────────────────────────────────────────────────

fn sheet_paint(app: &App, h: Handle<BottomSheet>) -> Paint {
    app.component::<Paint>(h).unwrap().clone()
}

#[test]
fn no_scrim_by_default() {
    let (app, _, _, h) = hosted(bottom_sheet());
    assert!(!sheet(&app, h).has_scrim());
    assert_eq!(sheet_paint(&app, h), Paint::None);
}

#[test]
fn the_scrim_dims_with_the_theme_scrim_color() {
    let (app, _, _, h) = hosted(bottom_sheet().scrim(true));
    let c = app.color(ColorRole::Scrim);
    assert_eq!(
        sheet_paint(&app, h),
        Paint::Quad(Quad::new(c.with_alpha(c.a * SCRIM_OPACITY)))
    );
}

#[test]
fn tapping_the_scrim_closes_and_hides_it() {
    let (mut app, win, host, h) = hosted(bottom_sheet().scrim(true));
    tap(&mut app, win, Point::new(W / 2.0, 40.0));
    assert!(sheet(&app, h).is_closed());
    assert_eq!(closed(&app, host), 1);
    assert_eq!(sheet_paint(&app, h), Paint::None, "no scrim while closed");

    with_sheet(&mut app, h, |c| c.open());
    assert_ne!(sheet_paint(&app, h), Paint::None, "back with the sheet");
}

#[test]
fn tapping_the_sheet_itself_is_not_a_scrim_tap() {
    let (mut app, win, host, h) = hosted(bottom_sheet().scrim(true));
    let at = centre(&app, sheet(&app, h).body());
    tap(&mut app, win, at);
    assert!(!sheet(&app, h).is_closed());
    assert_eq!(closed(&app, host), 0);
}

#[test]
fn the_scrim_does_not_close_a_sheet_that_is_not_dismissible() {
    let (mut app, win, host, h) = hosted(bottom_sheet().scrim(true).dismissible(false));
    tap(&mut app, win, Point::new(W / 2.0, 40.0));
    assert!(!sheet(&app, h).is_closed());
    assert_eq!(closed(&app, host), 0);
}

#[test]
fn without_a_scrim_a_tap_above_the_sheet_does_nothing() {
    let (mut app, win, host, h) = hosted(bottom_sheet());
    tap(&mut app, win, Point::new(W / 2.0, 40.0));
    assert!(!sheet(&app, h).is_closed());
    assert_eq!(closed(&app, host), 0);
}

#[test]
fn set_scrim_turns_it_on_and_off() {
    let (mut app, _, _, h) = hosted(bottom_sheet());
    with_sheet(&mut app, h, |c| c.set_scrim(true));
    assert_ne!(sheet_paint(&app, h), Paint::None);
    with_sheet(&mut app, h, |c| c.set_scrim(false));
    assert_eq!(sheet_paint(&app, h), Paint::None);
}
