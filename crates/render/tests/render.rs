//! Frames as a backend sees them: the queue for a window after layout,
//! paint, window and render have run, end to end on an `App`.

use std::cell::RefCell;

use animation::{AnimatedPaint, AnimationModule, AnimationTime, Layout, PaintAnimationSettings};
use app::prelude::*;
use geometry::{Color, Corners, Insets, Point, Rect, Size};
use layout::prelude::*;
use paint::prelude::*;
use render::prelude::*;
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

thread_local! {
    /// Every `FrameRequested` seen, in order.
    static REQUESTED: RefCell<Vec<NodeId>> = const { RefCell::new(Vec::new()) };
}

fn log_requested(_: &mut App, r: &FrameRequested) {
    REQUESTED.with(|l| l.borrow_mut().push(r.0));
}

fn take_requested() -> Vec<NodeId> {
    REQUESTED.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

/// The WSI stand-in: answers every `FrameRequested` with a `Frame` in the
/// same flush, so a tick that changes anything also draws.
fn answer(app: &mut App, r: &FrameRequested) {
    app.signal(Frame(r.0));
}

const RED: Color = Color::rgb(1.0, 0.0, 0.0);
const GREEN: Color = Color::rgb(0.0, 1.0, 0.0);
const BLUE: Color = Color::rgb(0.0, 0.0, 1.0);
const HALF_BLUE: Color = Color::rgba(0.0, 0.0, 1.0, 0.5);

/// Layout, paint, animation, window and render, `buffers` frames of damage, the loop
/// closed by `answer`, requests logged.
fn app_with(buffers: usize) -> App {
    let mut app = App::new();
    app.add_module(LayoutModule)
        .add_module(PaintModule)
        .add_module(WindowModule)
        .add_module(AnimationModule)
        .add_module(RenderModule { buffers })
        .system(log_requested)
        .system(answer);
    app
}

fn app() -> App {
    app_with(2)
}

/// A 200 by 100 window cleared to black: everything at the top level has
/// a known solid behind it.
fn a_window() -> WindowBuilder {
    window().layout(LayoutStyle::default().column().size(px(200.0), px(100.0)))
}

/// The same window cleared to a translucent colour: nothing is known
/// behind anything at the top level.
fn a_window_with_no_solid() -> WindowBuilder {
    a_window().clear(Color::rgba(0.0, 0.0, 0.0, 0.5))
}

fn window_rect() -> Rect {
    Rect::new(0.0, 0.0, 200.0, 100.0)
}

fn boxed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::default().size(px(w), px(h))
}

fn tile(n: u32) -> AtlasTile {
    AtlasTile {
        atlas: AtlasId(n),
        bounds: Rect::new(0.0, 0.0, 8.0, 8.0),
    }
}

/// An 8 by 8 glyph at `(x, y)` from the content box's corner.
fn glyph(x: f32, y: f32, color: Color) -> MonochromeSprite {
    MonochromeSprite::new(tile(1), Point::new(x, y), Size::new(8.0, 8.0), color)
}

fn frame(app: &mut App, w: NodeId) {
    app.signal(Frame(w));
    app.flush();
}

/// A copy of the queue, so the app is free again.
fn queue(app: &mut App, w: NodeId, age: usize) -> Queue {
    app.resource_mut::<Scenes>()
        .queue(w, age)
        .expect("the window has a scene")
        .clone()
}

fn zs(commands: &[Command]) -> Vec<f32> {
    commands.iter().map(|c| c.z).collect()
}

// ── 1, 2: passes, z and order ───────────────────────────────────────────

#[test]
fn a_rounded_quad_under_the_black_window_is_an_opaque_box_and_its_interior() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    app.spawn_with(
        win,
        Leaf,
        (boxed(50.0, 20.0), Paint::Quad(Quad::new(RED).radius(4.0))),
    );
    app.tick();
    assert_eq!(
        take_requested(),
        vec![win.id()],
        "the spawn's changes asked for a frame"
    );

    let q = queue(&mut app, win.id(), 1);
    assert_eq!(q.size, Size::new(200.0, 100.0));
    assert_eq!(q.scale, 1.0);
    assert_eq!(q.clear, Color::BLACK);
    assert_eq!(q.depth, 4.0, "the window and the quad");
    assert_eq!(q.scissor, vec![window_rect()], "a first frame");
    assert_eq!(q.opaque.scissor, vec![window_rect()]);
    assert!(q.translucent.commands.is_empty());
    assert!(q.translucent.scissor.is_empty());
    assert_eq!(
        zs(&q.opaque.commands),
        vec![3.0, 2.0],
        "the interior is nearer"
    );
    let inner = q.opaque.commands[0];
    assert_eq!(
        inner.rect,
        Rect::new(4.0, 4.0, 42.0, 12.0),
        "inset by the radius"
    );
    assert_eq!(
        (inner.radii, inner.border),
        (Corners::all(0.0), Insets::all(0.0))
    );
    assert_eq!(
        (inner.color, inner.border_color, inner.background),
        (RED, RED, RED)
    );
    assert!(inner.is_opaque());
    let c = q.opaque.commands[1];
    assert_eq!(c.rect, Rect::new(0.0, 0.0, 50.0, 20.0));
    assert_eq!(c.z, 2.0);
    assert_eq!(c.color, RED);
    assert_eq!(c.background, Color::BLACK, "the window's clear colour");
    assert_eq!(c.radii, Corners::all(4.0));
    assert_eq!(c.border, Insets::all(0.0));
    assert_eq!(c.kind(), Command::QUAD);
    assert!(c.is_opaque());
    assert_eq!(c.opacity(), 1.0);
}

#[test]
fn z_follows_preorder_and_each_pass_is_sorted_its_way() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window_with_no_solid());
    let a = app.spawn_with(
        win,
        Leaf,
        (
            boxed(100.0, 50.0),
            Paint::Quad(Quad::new(Color::rgba(1.0, 0.0, 0.0, 0.5))),
        ),
    );
    app.spawn_with(
        a,
        Leaf,
        (
            boxed(40.0, 20.0),
            Paint::Quad(Quad::new(Color::rgba(0.0, 1.0, 0.0, 0.5))),
        ),
    );
    let c = app.spawn_with(win, Leaf, (boxed(60.0, 30.0), Paint::Quad(Quad::new(BLUE))));
    app.spawn_with(c, Leaf, (boxed(10.0, 10.0), Paint::Quad(Quad::new(GREEN))));
    app.tick();

    let q = queue(&mut app, win.id(), 1);
    assert_eq!(q.depth, 10.0, "five nodes");
    assert_eq!(
        zs(&q.translucent.commands),
        vec![2.0, 4.0],
        "A then B, back to front"
    );
    assert_eq!(
        zs(&q.opaque.commands),
        vec![8.0, 6.0],
        "D then C, front to back"
    );
    assert_eq!(
        q.opaque.commands[0].background, BLUE,
        "D sits inside C's solid"
    );
    assert_eq!(
        q.opaque.commands[1].background, BLUE,
        "C is an edge-free opaque fill"
    );
    assert_eq!(q.opaque.commands[1].rect, Rect::new(0.0, 50.0, 60.0, 30.0));
    assert_eq!(q.opaque.commands[0].rect, Rect::new(0.0, 50.0, 10.0, 10.0));
}

// ── 3: the quad's three routes ──────────────────────────────────────────

#[test]
fn a_quad_with_edges_and_nothing_behind_it_splits_into_edge_and_interior() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window_with_no_solid());
    let bordered = Quad::new(RED).radius(6.0).border(2.0, GREEN);
    app.spawn_with(win, Leaf, (boxed(100.0, 50.0), Paint::Quad(bordered)));
    app.spawn_with(
        win,
        Leaf,
        (boxed(100.0, 50.0), Paint::Quad(bordered.opaque(false))),
    );
    app.tick();

    let q = queue(&mut app, win.id(), 1);
    assert_eq!(
        zs(&q.translucent.commands),
        vec![2.0, 4.0],
        "both quads blend their edges"
    );
    let edge = q.translucent.commands[0];
    assert_eq!(edge.rect, Rect::new(0.0, 0.0, 100.0, 50.0));
    assert_eq!((edge.color, edge.border_color), (RED, GREEN));
    assert_eq!(
        (edge.radii, edge.border),
        (Corners::all(6.0), Insets::all(2.0))
    );
    assert!(!edge.is_opaque());
    assert_eq!(
        zs(&q.opaque.commands),
        vec![3.0],
        "one interior, for the quad that allowed it"
    );
    let inner = q.opaque.commands[0];
    assert_eq!(
        inner.rect,
        Rect::new(6.0, 6.0, 88.0, 38.0),
        "inset by the larger of 6 and 2"
    );
    assert_eq!((inner.color, inner.background), (RED, RED));
    assert_eq!(
        (inner.radii, inner.border),
        (Corners::all(0.0), Insets::all(0.0))
    );
    assert_eq!(inner.kind(), Command::QUAD);
    assert!(inner.is_opaque());
}

#[test]
fn a_border_alone_blends_and_an_invisible_quad_only_takes_a_z() {
    // Three 30-tall quads stack inside the 100-tall window; a command
    // below the window would be outside every scissor rect.
    let mut app = app();
    let win = app.spawn(app.root(), a_window_with_no_solid());
    app.spawn_with(
        win,
        Leaf,
        (
            boxed(100.0, 30.0),
            Paint::Quad(Quad::new(Color::TRANSPARENT).border(2.0, GREEN)),
        ),
    );
    app.spawn_with(
        win,
        Leaf,
        (boxed(100.0, 30.0), Paint::Quad(Quad::default())),
    );
    app.spawn_with(
        win,
        Leaf,
        (boxed(100.0, 30.0), Paint::Quad(Quad::new(BLUE))),
    );
    app.tick();

    let q = queue(&mut app, win.id(), 1);
    assert_eq!(zs(&q.translucent.commands), vec![2.0], "the ring");
    assert_eq!(q.translucent.commands[0].border_color, GREEN);
    assert_eq!(
        zs(&q.opaque.commands),
        vec![6.0],
        "the invisible quad took z 4 and drew nothing"
    );
    assert_eq!(q.opaque.commands[0].rect, Rect::new(0.0, 60.0, 100.0, 30.0));
    assert_eq!(q.depth, 8.0);
}

#[test]
fn inside_a_solid_a_quad_is_opaque_whatever_its_edges_or_alpha() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    app.spawn_with(
        win,
        Leaf,
        (
            boxed(100.0, 50.0),
            Paint::Quad(Quad::new(RED).radius(6.0).border(2.0, GREEN)),
        ),
    );
    app.spawn_with(
        win,
        Leaf,
        (boxed(100.0, 50.0), Paint::Quad(Quad::new(HALF_BLUE))),
    );
    app.tick();

    let q = queue(&mut app, win.id(), 1);
    assert!(q.translucent.commands.is_empty());
    assert_eq!(
        zs(&q.opaque.commands),
        vec![4.0, 3.0, 2.0],
        "the flat half-blue has no interior; the rounded red has one at 3"
    );
    assert_eq!(q.opaque.commands[1].rect, Rect::new(6.0, 6.0, 88.0, 38.0));
    let rounded = q.opaque.commands[2];
    assert_eq!(
        (rounded.radii, rounded.border),
        (Corners::all(6.0), Insets::all(2.0))
    );
    assert_eq!(
        (rounded.color, rounded.border_color, rounded.background),
        (RED, GREEN, Color::BLACK)
    );
    let half = q.opaque.commands[0];
    assert_eq!((half.color, half.background), (HALF_BLUE, Color::BLACK));
    assert!(half.is_opaque());
}

#[test]
fn a_pill_inside_a_solid_has_no_interior_command() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    app.spawn_with(
        win,
        Leaf,
        (boxed(100.0, 20.0), Paint::Quad(Quad::new(RED).radius(10.0))),
    );
    app.tick();

    let q = queue(&mut app, win.id(), 1);
    assert_eq!(
        zs(&q.opaque.commands),
        vec![2.0],
        "nothing is flat inside a pill"
    );
}

#[test]
fn a_borderless_quad_carries_its_fill_as_border_colour_and_zero_widths() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    // Widths with no colour: nothing a renderer would draw.
    app.spawn_with(
        win,
        Leaf,
        (
            boxed(50.0, 20.0),
            Paint::Quad(Quad::new(RED).border_widths(Insets::all(2.0))),
        ),
    );
    app.spawn_with(
        win,
        Leaf,
        (
            boxed(50.0, 20.0),
            Paint::Quad(Quad::new(RED).border(2.0, BLUE)),
        ),
    );
    app.tick();

    let q = queue(&mut app, win.id(), 1);
    assert_eq!(
        zs(&q.opaque.commands),
        vec![5.0, 4.0, 2.0],
        "the bordered's interior, the bordered, then the borderless"
    );
    assert_eq!(q.opaque.commands[0].rect, Rect::new(2.0, 22.0, 46.0, 16.0));
    let bare = q.opaque.commands[2];
    assert_eq!(
        (bare.color, bare.border_color),
        (RED, RED),
        "no border: the fill is the border colour"
    );
    assert_eq!(bare.border, Insets::all(0.0), "and no width either");
    assert_eq!(bare.background, Color::BLACK);
    let ringed = q.opaque.commands[1];
    assert_eq!((ringed.color, ringed.border_color), (RED, BLUE));
    assert_eq!(ringed.border, Insets::all(2.0), "a visible border is kept");
}

// ── 4: sprites ──────────────────────────────────────────────────────────

/// A red panel with 4 of padding under `win`, and a text node inside it
/// painted with `run`. Returns the text node.
fn panel_with_text(
    app: &mut App,
    win: Handle<Window>,
    panel: Quad,
    run: Vec<MonochromeSprite>,
) -> Handle<Leaf> {
    let p = app.spawn_with(
        win,
        Leaf,
        (boxed(100.0, 40.0).padding_all(px(4.0)), Paint::Quad(panel)),
    );
    app.spawn_with(p, Leaf, (boxed(60.0, 20.0), Paint::Monochrome(run)))
}

#[test]
fn sprites_inside_a_solid_go_opaque_with_that_background() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    panel_with_text(
        &mut app,
        win,
        Quad::new(RED),
        vec![
            glyph(2.0, 3.0, Color::WHITE),
            glyph(12.0, 3.0, Color::WHITE),
        ],
    );
    app.tick();

    let q = queue(&mut app, win.id(), 1);
    assert!(q.translucent.commands.is_empty());
    assert_eq!(
        zs(&q.opaque.commands),
        vec![4.0, 4.0, 2.0],
        "two glyphs, then the panel"
    );
    let (g1, g2) = (q.opaque.commands[0], q.opaque.commands[1]);
    assert_eq!(
        g1.rect,
        Rect::new(6.0, 7.0, 8.0, 8.0),
        "content corner (4, 4) plus (2, 3)"
    );
    assert_eq!(g2.rect, Rect::new(16.0, 7.0, 8.0, 8.0));
    assert_eq!(
        (g1.kind(), g1.color, g1.background),
        (Command::SPRITE, Color::WHITE, RED)
    );
    assert_eq!(g1.tile, tile(1));
    assert!(g1.is_opaque());
}

#[test]
fn sprites_blend_with_no_solid_and_composite_through_a_translucent_quad() {
    let mut app = app();
    let bare = app.spawn(app.root(), a_window_with_no_solid());
    app.spawn_with(
        bare,
        Leaf,
        (
            boxed(60.0, 20.0),
            Paint::Monochrome(vec![glyph(2.0, 3.0, Color::WHITE)]),
        ),
    );
    let black = app.spawn(app.root(), a_window());
    app.spawn_with(
        black,
        Leaf,
        (
            boxed(60.0, 20.0),
            Paint::Monochrome(vec![glyph(2.0, 3.0, Color::WHITE)]),
        ),
    );
    let layered = app.spawn(app.root(), a_window());
    let red = app.spawn_with(
        layered,
        Leaf,
        (boxed(100.0, 40.0), Paint::Quad(Quad::new(RED))),
    );
    let tint = app.spawn_with(
        red,
        Leaf,
        (
            boxed(80.0, 30.0).padding_all(px(4.0)),
            Paint::Quad(Quad::new(HALF_BLUE)),
        ),
    );
    app.spawn_with(
        tint,
        Leaf,
        (
            boxed(60.0, 20.0),
            Paint::Monochrome(vec![glyph(2.0, 3.0, Color::WHITE)]),
        ),
    );
    app.tick();

    let q = queue(&mut app, bare.id(), 1);
    assert_eq!(
        zs(&q.translucent.commands),
        vec![2.0],
        "no solid: the glyph blends"
    );
    assert_eq!(
        q.translucent.commands[0].rect,
        Rect::new(2.0, 3.0, 8.0, 8.0)
    );
    assert!(!q.translucent.commands[0].is_opaque());

    let q = queue(&mut app, black.id(), 1);
    assert_eq!(
        q.opaque.commands[0].background,
        Color::BLACK,
        "the bare window is a solid"
    );

    let q = queue(&mut app, layered.id(), 1);
    let g = q.opaque.commands[0];
    assert_eq!(g.kind(), Command::SPRITE);
    assert_eq!(
        g.background,
        HALF_BLUE.over(RED),
        "the tint composited onto the panel"
    );
}

#[test]
fn a_sprite_over_a_rounded_corner_or_opted_out_blends() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let corner = app.spawn_with(
        win,
        Leaf,
        (boxed(100.0, 40.0), Paint::Quad(Quad::new(RED).radius(10.0))),
    );
    app.spawn_with(
        corner,
        Leaf,
        (
            boxed(60.0, 20.0),
            Paint::Monochrome(vec![glyph(0.0, 0.0, Color::WHITE)]),
        ),
    );
    panel_with_text(
        &mut app,
        win,
        Quad::new(RED),
        vec![glyph(2.0, 3.0, Color::WHITE).opaque(false)],
    );
    app.tick();

    let q = queue(&mut app, win.id(), 1);
    assert_eq!(zs(&q.translucent.commands), vec![4.0, 8.0]);
    assert_eq!(
        q.translucent.commands[0].rect,
        Rect::new(0.0, 0.0, 8.0, 8.0),
        "crosses the interior's edge"
    );
    assert_eq!(
        q.translucent.commands[1].rect,
        Rect::new(6.0, 47.0, 8.0, 8.0),
        "opted out"
    );
    assert_eq!(
        zs(&q.opaque.commands),
        vec![6.0, 3.0, 2.0],
        "the two panels and the rounded one's interior"
    );
}

// ── 5: scale ────────────────────────────────────────────────────────────

#[test]
fn scale_two_doubles_every_rect_radius_and_width_and_the_window() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    panel_with_text(
        &mut app,
        win,
        Quad::new(RED).radius(3.0).border(1.0, GREEN),
        vec![glyph(2.0, 3.0, Color::WHITE)],
    );
    app.tick();
    take_requested();

    app.emit(ScaleFactorChanged { scale: 2.0 }, win);
    app.flush();
    assert_eq!(
        take_requested(),
        vec![win.id()],
        "the window asked for a frame itself"
    );

    let q = queue(&mut app, win.id(), 1);
    assert_eq!((q.size, q.scale), (Size::new(400.0, 200.0), 2.0));
    assert_eq!(
        q.scissor,
        vec![Rect::new(0.0, 0.0, 400.0, 200.0)],
        "a new scale damages everything"
    );
    let g = q.opaque.commands[0];
    assert_eq!(g.rect, Rect::new(12.0, 14.0, 16.0, 16.0));
    let inner = q.opaque.commands[1];
    assert_eq!(
        inner.rect,
        Rect::new(6.0, 6.0, 188.0, 68.0),
        "inset by the scaled radius"
    );
    let p = q.opaque.commands[2];
    assert_eq!(p.rect, Rect::new(0.0, 0.0, 200.0, 80.0));
    assert_eq!((p.radii, p.border), (Corners::all(6.0), Insets::all(2.0)));
    assert_eq!(
        zs(&q.opaque.commands),
        vec![4.0, 3.0, 2.0],
        "z does not scale"
    );
}

// ── 10: images ──────────────────────────────────────────────────────────

fn an_image() -> PolychromeSprite {
    PolychromeSprite::new(tile(2))
        .radius(3.0)
        .opacity(0.5)
        .grayscale(true)
}

#[test]
fn an_image_blends_with_no_solid_and_goes_opaque_inside_one() {
    let mut app = app();
    let bare = app.spawn(app.root(), a_window_with_no_solid());
    app.spawn_with(
        bare,
        Leaf,
        (boxed(40.0, 30.0), Paint::Polychrome(an_image())),
    );
    let black = app.spawn(app.root(), a_window());
    let panel = app.spawn_with(
        black,
        Leaf,
        (boxed(100.0, 40.0), Paint::Quad(Quad::new(RED))),
    );
    app.spawn_with(
        panel,
        Leaf,
        (boxed(40.0, 30.0), Paint::Polychrome(an_image())),
    );
    app.spawn_with(
        panel,
        Leaf,
        (
            boxed(40.0, 30.0),
            Paint::Polychrome(an_image().opaque(false)),
        ),
    );
    app.tick();

    let q = queue(&mut app, bare.id(), 1);
    assert_eq!(zs(&q.translucent.commands), vec![2.0]);
    let i = q.translucent.commands[0];
    assert_eq!(i.rect, Rect::new(0.0, 0.0, 40.0, 30.0), "the content box");
    assert_eq!(
        (i.kind(), i.color, i.tile),
        (Command::IMAGE, Color::WHITE, tile(2))
    );
    assert_eq!(i.radii, Corners::all(3.0));
    assert!((i.opacity() - 0.5).abs() < 0.01);
    assert!(i.is_grayscale());
    assert!(!i.is_opaque());

    let q = queue(&mut app, black.id(), 1);
    assert_eq!(
        zs(&q.opaque.commands),
        vec![4.0, 2.0],
        "the image inside the panel, then the panel"
    );
    let i = q.opaque.commands[0];
    assert_eq!((i.kind(), i.background), (Command::IMAGE, RED));
    assert_eq!(
        i.radii,
        Corners::all(3.0),
        "radii are fine over a known solid"
    );
    assert!(i.is_opaque());
    assert_eq!(
        zs(&q.translucent.commands),
        vec![6.0],
        "the one that opted out"
    );
}

// ── 6, 7: damage ────────────────────────────────────────────────────────

/// A black window with two 50 by 20 quads stacked at the top, drawn once.
/// Returns the window and the two quads.
fn two_quads(app: &mut App) -> (Handle<Window>, Handle<Leaf>, Handle<Leaf>) {
    let win = app.spawn(app.root(), a_window());
    let a = app.spawn_with(win, Leaf, (boxed(50.0, 20.0), Paint::Quad(Quad::new(RED))));
    let b = app.spawn_with(
        win,
        Leaf,
        (boxed(50.0, 20.0), Paint::Quad(Quad::new(GREEN))),
    );
    app.tick();
    take_requested();
    (win, a, b)
}

const A_RECT: Rect = Rect::new(0.0, 0.0, 50.0, 20.0);
const B_RECT: Rect = Rect::new(0.0, 20.0, 50.0, 20.0);

#[test]
fn a_paint_change_damages_that_node_and_queues_only_what_touches_it() {
    let mut app = app();
    let (win, a, _) = two_quads(&mut app);

    app.component_mut::<AnimatedPaint>(a).unwrap().0 = Paint::Quad(Quad::new(BLUE));
    app.tick();
    assert_eq!(take_requested(), vec![win.id()]);

    let q = queue(&mut app, win.id(), 1);
    assert_eq!(q.scissor, vec![A_RECT]);
    assert_eq!(q.opaque.scissor, vec![A_RECT]);
    assert_eq!(q.opaque.commands.len(), 1, "B touches the damage nowhere");
    assert_eq!(q.opaque.commands[0].color, BLUE);
    assert!(q.translucent.scissor.is_empty());
}

#[test]
fn renderer_reads_displayed_paint_not_its_target() {
    use std::time::Duration;

    let mut app = app();
    let (win, a, _) = two_quads(&mut app);
    *app.component_mut::<PaintAnimationSettings>(a).unwrap() =
        PaintAnimationSettings::custom(AnimationTime::Duration(Duration::from_secs(60)), |t| t);
    *app.component_mut::<Paint>(a).unwrap() = Paint::Quad(Quad::new(BLUE));
    app.tick();

    let Paint::Quad(displayed) = app.component::<AnimatedPaint>(a).unwrap().0 else {
        panic!("the quad remains displayed during its transition");
    };
    assert_ne!(displayed.color, BLUE);
    let q = queue(&mut app, win.id(), 0);
    assert_eq!(
        q.opaque
            .commands
            .iter()
            .find(|c| c.rect == A_RECT)
            .unwrap()
            .color,
        displayed.color
    );
}

#[test]
fn target_paint_changes_receive_damage_after_the_displayed_change_notification() {
    let mut app = app();
    let (win, a, _) = two_quads(&mut app);
    *app.component_mut::<Paint>(a).unwrap() = Paint::Quad(Quad::new(BLUE));
    app.tick();
    assert_eq!(take_requested(), vec![win.id()]);
    assert!(queue(&mut app, win.id(), 1).scissor.is_empty());

    app.tick();
    assert_eq!(take_requested(), vec![win.id()]);
    let q = queue(&mut app, win.id(), 1);
    assert_eq!(q.scissor, vec![A_RECT]);
    assert_eq!(q.opaque.commands[0].color, BLUE);
}

#[test]
fn a_move_damages_old_and_new_for_everything_that_moved() {
    let mut app = app();
    let (win, a, _) = two_quads(&mut app);

    *app.component_mut::<LayoutStyle>(a).unwrap() =
        boxed(50.0, 20.0).margin(Insets::new(px(10.0), px(0.0), px(0.0), px(0.0)));
    app.tick();
    assert_eq!(take_requested(), vec![win.id()]);

    let q = queue(&mut app, win.id(), 1);
    assert_eq!(
        q.scissor,
        vec![
            A_RECT,
            Rect::new(0.0, 10.0, 50.0, 20.0),
            B_RECT,
            Rect::new(0.0, 30.0, 50.0, 20.0),
        ],
        "A old, A new, B old, B new"
    );
}

#[test]
fn a_removal_damages_what_it_drew_and_asks_for_a_frame() {
    let mut app = app();
    let (win, _, b) = two_quads(&mut app);

    assert!(app.remove(b));
    app.tick();
    assert_eq!(
        take_requested(),
        vec![win.id()],
        "nothing moved, yet a frame is needed"
    );

    let q = queue(&mut app, win.id(), 1);
    assert_eq!(q.scissor, vec![B_RECT], "the hole is cleared");
    assert!(q.opaque.commands.is_empty(), "A touches it nowhere");
    assert!(q.opaque.scissor.is_empty());
}

#[test]
fn a_spawn_into_a_drawn_window_damages_only_its_own_rect() {
    let mut app = app();
    let (win, _, _) = two_quads(&mut app);

    let c = app.spawn_with(win, Leaf, (boxed(50.0, 20.0), Paint::Quad(Quad::new(BLUE))));
    app.tick();
    assert_eq!(
        take_requested(),
        vec![win.id()],
        "the spawn asks for a frame"
    );

    let c_rect = Rect::new(0.0, 40.0, 50.0, 20.0);
    let q = queue(&mut app, win.id(), 1);
    assert_eq!(
        q.scissor,
        vec![c_rect],
        "no previous rect, and no sibling moved"
    );
    assert_eq!(
        q.opaque.commands.len(),
        1,
        "only the new quad touches the damage"
    );
    assert_eq!(q.opaque.commands[0].rect, c_rect);
    assert_eq!(q.opaque.commands[0].color, BLUE);
    assert_eq!(
        zs(&q.opaque.commands),
        vec![6.0],
        "third child: preorder index 3"
    );
    assert!(app.is_live(c));
}

#[test]
fn a_resize_damages_the_whole_window() {
    let mut app = app();
    let (win, _, _) = two_quads(&mut app);

    app.emit(
        Resized {
            size: Size::new(300.0, 100.0),
        },
        win,
    );
    app.tick();
    assert_eq!(take_requested(), vec![win.id()]);

    let q = queue(&mut app, win.id(), 1);
    assert_eq!(q.size, Size::new(300.0, 100.0));
    assert_eq!(q.scissor, vec![Rect::new(0.0, 0.0, 300.0, 100.0)]);
    assert_eq!(q.opaque.commands.len(), 2);
}

#[test]
fn a_hidden_node_damages_its_old_rect_and_draws_nothing() {
    let mut app = app();
    let (win, a, _) = two_quads(&mut app);

    *app.component_mut::<LayoutStyle>(a).unwrap() = boxed(50.0, 20.0).hidden();
    app.tick();

    let q = queue(&mut app, win.id(), 1);
    assert_eq!(
        q.scissor,
        vec![A_RECT, B_RECT],
        "A's old; B's old and new, and B's new is A's old"
    );
    assert_eq!(q.opaque.commands.len(), 1);
    assert_eq!(q.opaque.commands[0].color, GREEN, "B, now at the top");
}

#[test]
fn a_write_after_the_drain_is_drawn_and_damaged_by_the_frame_that_draws_it() {
    let mut app = app_with(2);
    let (win, a, _) = two_quads(&mut app);

    // A `Layout` written between the drain and the frame: nothing marked
    // the node, yet the frame draws it at its new bounds.
    let moved = Rect::new(0.0, 0.0, 60.0, 20.0);
    app.component_mut::<Layout>(a).unwrap().rect = moved;
    frame(&mut app, win.id());
    assert_eq!(
        queue(&mut app, win.id(), 1).scissor,
        vec![A_RECT, moved],
        "old and new, though nothing marked it"
    );

    app.tick();
    assert_eq!(take_requested(), vec![win.id()]);
    assert_eq!(
        queue(&mut app, win.id(), 1).scissor,
        vec![moved],
        "the late notification redraws the current bounds"
    );
}

#[test]
fn renderer_reads_displayed_window_and_content_geometry_not_targets() {
    let mut app = app_with(2);
    let win = app.spawn(app.root(), a_window());
    let image = app.spawn_with(
        win,
        Leaf,
        (boxed(40.0, 30.0), Paint::Polychrome(an_image())),
    );
    app.tick();
    take_requested();

    app.component_mut::<ComputedLayout>(win).unwrap().rect = Rect::new(0.0, 0.0, 900.0, 700.0);
    app.component_mut::<ComputedLayout>(image).unwrap().rect = Rect::ZERO;
    frame(&mut app, win.id());
    assert_eq!(queue(&mut app, win.id(), 0).size, Size::new(200.0, 100.0));
    assert_eq!(
        queue(&mut app, win.id(), 0).opaque.commands[0].rect,
        Rect::new(0.0, 0.0, 40.0, 30.0)
    );

    app.component_mut::<Layout>(win).unwrap().rect = Rect::new(0.0, 0.0, 250.0, 120.0);
    let displayed = Layout {
        rect: Rect::new(10.0, 20.0, 60.0, 40.0),
        padding: Insets::new(1.0, 2.0, 3.0, 4.0),
        border: Insets::all(1.0),
    };
    *app.component_mut::<Layout>(image).unwrap() = displayed;
    frame(&mut app, win.id());
    let q = queue(&mut app, win.id(), 1);
    assert_eq!(q.size, Size::new(250.0, 120.0));
    assert_eq!(q.opaque.commands[0].rect, displayed.content());
}

#[test]
fn frame_time_layout_changes_damage_equal_bounds() {
    #[derive(Default)]
    struct NextLayout(Option<(NodeId, Layout)>);
    impl Resource for NextLayout {}

    fn animate(app: &mut App, _: &Frame) {
        let next = app.resource_mut::<NextLayout>().0.take();
        if let Some((id, layout)) = next {
            *app.component_mut::<Layout>(id).unwrap() = layout;
        }
    }

    let mut app = App::new();
    app.add_module(LayoutModule)
        .add_module(PaintModule)
        .add_module(WindowModule)
        .add_module(AnimationModule)
        .init_resource::<NextLayout>()
        .system(animate)
        .add_module(RenderModule::default())
        .system(log_requested)
        .system(answer);
    let (win, a, _) = two_quads(&mut app);
    let initial = *app.component::<Layout>(a).unwrap();
    for layout in [
        Layout {
            padding: Insets::all(2.0),
            ..initial
        },
        Layout {
            border: Insets::all(1.0),
            ..initial
        },
        Layout {
            rect: Rect::new(0.1, 0.1, 50.0, 20.0),
            ..initial
        },
    ] {
        app.resource_mut::<NextLayout>().0 = Some((a.id(), layout));
        frame(&mut app, win.id());
        let q = queue(&mut app, win.id(), 1);
        assert_eq!(
            q.scissor,
            vec![A_RECT],
            "same-frame damage despite equal bounds"
        );
        assert_eq!(q.opaque.commands[0].rect, A_RECT);
        assert!(take_requested().is_empty());

        app.tick();
        assert_eq!(take_requested(), vec![win.id()]);
        assert_eq!(
            queue(&mut app, win.id(), 1).scissor,
            vec![A_RECT],
            "the late notification also marks damage"
        );
    }
}

#[test]
fn a_clean_frame_has_nothing_to_do_and_a_clean_tick_asks_for_nothing() {
    let mut app = app();
    let (win, _, _) = two_quads(&mut app);

    frame(&mut app, win.id());
    let q = queue(&mut app, win.id(), 1);
    assert!(q.scissor.is_empty());
    assert!(q.opaque.commands.is_empty() && q.opaque.scissor.is_empty());
    assert!(q.translucent.commands.is_empty() && q.translucent.scissor.is_empty());
    assert_eq!(q.depth, 6.0, "the scene is still there");

    app.tick();
    assert!(take_requested().is_empty());
}

#[test]
fn ages_union_the_frames_held_and_anything_else_is_the_window() {
    let mut app = app_with(2);
    let (win, a, b) = two_quads(&mut app);

    app.component_mut::<AnimatedPaint>(a).unwrap().0 = Paint::Quad(Quad::new(BLUE));
    app.tick();
    app.component_mut::<AnimatedPaint>(b).unwrap().0 = Paint::Quad(Quad::new(BLUE));
    app.tick();

    assert_eq!(queue(&mut app, win.id(), 1).scissor, vec![B_RECT]);
    assert_eq!(
        queue(&mut app, win.id(), 2).scissor,
        vec![B_RECT, A_RECT],
        "newest first"
    );
    assert_eq!(
        queue(&mut app, win.id(), 3).scissor,
        vec![window_rect()],
        "older than held"
    );
    assert_eq!(
        queue(&mut app, win.id(), 0).scissor,
        vec![window_rect()],
        "unknown"
    );
    assert_eq!(
        queue(&mut app, win.id(), 2).opaque.commands.len(),
        2,
        "both touch the union"
    );
}

// ── 8, 9: requests and windows ──────────────────────────────────────────

#[test]
fn changes_request_one_frame_per_window_and_none_outside_any() {
    let mut app = app();
    let w1 = app.spawn(app.root(), a_window());
    let c1 = app.spawn_with(w1, Leaf, (boxed(10.0, 10.0), Paint::Quad(Quad::new(RED))));
    let w2 = app.spawn(app.root(), a_window());
    let c2 = app.spawn_with(w2, Leaf, (boxed(10.0, 10.0), Paint::Quad(Quad::new(RED))));
    let stray = app.spawn_with(
        app.root(),
        Leaf,
        (boxed(10.0, 10.0), Paint::Quad(Quad::new(RED))),
    );
    app.tick();
    assert_eq!(take_requested(), vec![w1.id(), w2.id()]);

    app.component_mut::<AnimatedPaint>(c1).unwrap().0 = Paint::Quad(Quad::new(GREEN));
    app.component_mut::<AnimatedPaint>(c2).unwrap().0 = Paint::Quad(Quad::new(GREEN));
    app.component_mut::<AnimatedPaint>(c1).unwrap().0 = Paint::Quad(Quad::new(BLUE));
    app.tick();
    assert_eq!(
        take_requested(),
        vec![w1.id(), w2.id()],
        "once each, in first-write order"
    );

    app.component_mut::<AnimatedPaint>(stray).unwrap().0 = Paint::Quad(Quad::new(GREEN));
    app.tick();
    assert!(take_requested().is_empty(), "outside every window");
}

#[test]
fn a_removed_window_loses_its_scene_and_odd_frames_are_ignored() {
    let mut app = app();
    let (win, _, _) = two_quads(&mut app);
    let other = app.spawn(app.root(), a_window());
    let leaf = app.spawn(other, Leaf);
    app.tick();
    take_requested();

    assert!(app.remove(win));
    app.flush();
    assert!(app.resource_mut::<Scenes>().queue(win.id(), 1).is_none());
    assert!(
        app.resource_mut::<Scenes>().queue(other.id(), 1).is_some(),
        "the other keeps its scene"
    );

    frame(&mut app, win.id());
    frame(&mut app, leaf.id());
    assert!(
        app.resource_mut::<Scenes>().queue(leaf.id(), 1).is_none(),
        "a leaf is not a window"
    );
    assert!(
        take_requested().is_empty(),
        "a frame never asks for another"
    );
}

// ── 11: viewports ───────────────────────────────────────────────────────

/// A 100 by 50 list with a 2 px border at the window's corner, its
/// `Viewport` set to `clip` and no scroll, and `rows` 96 by 20 green rows
/// stacked from its content box at (2, 2). The list paints `list`.
/// Returns the list and its rows.
fn a_list(
    app: &mut App,
    win: Handle<Window>,
    list: Paint,
    clip: bool,
    rows: usize,
) -> (Handle<Leaf>, Vec<Handle<Leaf>>) {
    let l = app.spawn_with(
        win,
        Leaf,
        (
            boxed(100.0, 50.0)
                .column()
                .shrink(0.0)
                .border(Insets::all(px(2.0))),
            list,
            Viewport {
                clip,
                scroll: Point::default(),
            },
        ),
    );
    let rows = (0..rows)
        .map(|_| {
            app.spawn_with(
                l,
                Leaf,
                (boxed(96.0, 20.0).shrink(0.0), Paint::Quad(Quad::new(GREEN))),
            )
        })
        .collect();
    (l, rows)
}

fn scroll_to(app: &mut App, id: impl Into<NodeId>, x: f32, y: f32) {
    app.component_mut::<Viewport>(id).unwrap().scroll = Point::new(x, y);
}

/// Every command of both passes with that z.
fn at_z(q: &Queue, z: f32) -> Vec<Command> {
    q.opaque
        .commands
        .iter()
        .chain(&q.translucent.commands)
        .filter(|c| c.z == z)
        .copied()
        .collect()
}

#[test]
fn scroll_shifts_children_commands() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let (list, _) = a_list(&mut app, win, Paint::Quad(Quad::new(RED)), false, 3);
    app.tick();
    app.emit(ScaleFactorChanged { scale: 2.0 }, win);
    app.flush();
    scroll_to(&mut app, list, 0.0, 10.0);
    app.tick();

    let q = queue(&mut app, win.id(), 1);
    let window = Rect::new(0.0, 0.0, 400.0, 200.0);
    let l = at_z(&q, 2.0);
    assert_eq!(l.len(), 1);
    assert_eq!(
        l[0].rect,
        Rect::new(0.0, 0.0, 200.0, 100.0),
        "the list itself is not shifted"
    );
    assert_eq!(l[0].clip, window, "no clip in effect: the window");
    // Row 1's layout is (2, 22, 96, 20); less the scroll, times two.
    let r = at_z(&q, 6.0);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].rect, Rect::new(4.0, 24.0, 192.0, 40.0));
    assert_eq!(
        r[0].clip, window,
        "an unclipped viewport hands down its clip"
    );
    // Row 0 is shifted above the list and, unclipped, still drawn there.
    assert_eq!(at_z(&q, 4.0)[0].rect, Rect::new(4.0, -16.0, 192.0, 40.0));
}

#[test]
fn clip_is_the_padding_box_and_intersects() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let (list, rows) = a_list(&mut app, win, Paint::Quad(Quad::new(RED)), true, 2);
    // Row 0 clips too, around a 10 by 20 child at its corner.
    *app.component_mut::<Viewport>(rows[0]).unwrap() = Viewport {
        clip: true,
        scroll: Point::default(),
    };
    app.spawn_with(
        rows[0],
        Leaf,
        (boxed(10.0, 20.0), Paint::Quad(Quad::new(BLUE))),
    );
    app.tick();

    // z: window 0, list 2, row 0 4, its child 6, row 1 8.
    let padding_box = Rect::new(2.0, 2.0, 96.0, 46.0);
    let q = queue(&mut app, win.id(), 1);
    assert_eq!(
        at_z(&q, 2.0)[0].clip,
        window_rect(),
        "the list is not clipped"
    );
    assert_eq!(at_z(&q, 4.0)[0].clip, padding_box, "inside the border");
    assert_eq!(at_z(&q, 8.0)[0].clip, padding_box);
    assert_eq!(
        at_z(&q, 6.0)[0].clip,
        Rect::new(2.0, 2.0, 96.0, 20.0),
        "row 0's box, inside the list's"
    );

    // Scrolled 10 down, row 0 sits at (2, -8): its box and the list's
    // overlap in (2, 2, 96, 10).
    scroll_to(&mut app, list, 0.0, 10.0);
    app.tick();
    let q = queue(&mut app, win.id(), 1);
    let row = at_z(&q, 4.0)[0];
    assert_eq!(
        (row.rect, row.clip),
        (Rect::new(2.0, -8.0, 96.0, 20.0), padding_box)
    );
    let child = at_z(&q, 6.0)[0];
    assert_eq!(child.rect, Rect::new(2.0, -8.0, 10.0, 20.0));
    assert_eq!(
        child.clip,
        Rect::new(2.0, 2.0, 96.0, 10.0),
        "the intersection"
    );
}

#[test]
fn fully_clipped_nodes_emit_nothing_and_damage_their_old_bounds() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    // The list paints nothing, so only the rows damage anything.
    let (list, _) = a_list(&mut app, win, Paint::None, true, 4);
    app.tick();

    // z: window 0, list 2, rows 4, 6, 8, 10 at y 2, 22, 42, 62.
    let q = queue(&mut app, win.id(), 1);
    assert_eq!(
        zs(&q.opaque.commands),
        vec![8.0, 6.0, 4.0],
        "row 3 starts below the clip and emits nothing"
    );
    assert!(q.translucent.commands.is_empty());

    take_requested();
    scroll_to(&mut app, list, 0.0, 40.0);
    app.tick();
    let q = queue(&mut app, win.id(), 1);
    assert_eq!(
        zs(&q.opaque.commands),
        vec![10.0, 8.0],
        "rows 0 and 1 are above the clip now"
    );
    assert_eq!(
        q.scissor,
        vec![
            Rect::new(2.0, 2.0, 96.0, 20.0),
            Rect::new(2.0, 22.0, 96.0, 20.0),
            Rect::new(2.0, 42.0, 96.0, 6.0),
        ],
        "rows 0 and 1's old bounds, and row 2's old, clipped; \
         the new bounds of rows 2 and 3 lie inside the first two"
    );
}

#[test]
fn viewport_change_requests_a_frame() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let (list, _) = a_list(&mut app, win, Paint::None, true, 1);
    app.tick();
    take_requested();

    scroll_to(&mut app, list, 0.0, 5.0);
    app.tick();
    assert_eq!(take_requested(), vec![win.id()]);
}

#[test]
fn a_half_pixel_scroll_moves_a_row_by_whole_device_pixels_and_keeps_its_height() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let (list, _) = a_list(&mut app, win, Paint::None, true, 2);
    app.tick();
    // Row 0's top lands on -0.5: rounded per edge it would grow to 21.
    scroll_to(&mut app, list, 0.0, 2.5);
    app.tick();
    let q = queue(&mut app, win.id(), 1);
    assert_eq!(
        at_z(&q, 4.0)[0].rect,
        Rect::new(2.0, -1.0, 96.0, 20.0),
        "the scroll rounds to 3 device pixels, the row keeps its height"
    );
}

#[test]
fn a_scroll_snaps_in_device_pixels_at_scale_two() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let (list, _) = a_list(&mut app, win, Paint::None, true, 2);
    app.tick();
    app.emit(ScaleFactorChanged { scale: 2.0 }, win);
    app.flush();
    // 2.25 logical is 4.5 device, rounded to 5.
    scroll_to(&mut app, list, 0.0, 2.25);
    app.tick();
    let q = queue(&mut app, win.id(), 1);
    assert_eq!(at_z(&q, 4.0)[0].rect, Rect::new(4.0, -1.0, 192.0, 40.0));
    // Overscroll: -0.25 logical is -0.5 device, rounded away from zero.
    scroll_to(&mut app, list, 0.0, -0.25);
    app.tick();
    let q = queue(&mut app, win.id(), 1);
    assert_eq!(at_z(&q, 4.0)[0].rect, Rect::new(4.0, 5.0, 192.0, 40.0));
}

const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);

/// The rows of a white list scrolled 5 down: each half under its top edge.
fn rows_under_the_edge(rows: &[Paint]) -> (App, Handle<Window>) {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let (list, _) = a_list(&mut app, win, Paint::Quad(Quad::new(WHITE)), true, 0);
    for p in rows {
        app.spawn_with(list, Leaf, (boxed(96.0, 20.0).shrink(0.0), p.clone()));
    }
    app.tick();
    scroll_to(&mut app, list, 0.0, 5.0);
    app.tick();
    (app, win)
}

#[test]
fn a_rounded_row_half_under_the_viewport_edge_stays_in_the_opaque_pass() {
    let (mut app, win) = rows_under_the_edge(&[Paint::Quad(Quad::new(RED).radius(4.0))]);
    let q = queue(&mut app, win.id(), 0);
    // z: window 0, list 2, row 4.
    let row: Vec<_> = q.opaque.commands.iter().filter(|c| c.z == 4.0).collect();
    assert_eq!(row.len(), 1, "the row's box is opaque");
    assert_eq!(row[0].background, WHITE);
    assert!(q.translucent.commands.iter().all(|c| c.z != 4.0));
}

#[test]
fn a_glyph_and_an_image_half_under_the_edge_stay_opaque() {
    let opaque_image = PolychromeSprite::new(tile(2)).radius(3.0);
    for row in [
        Paint::Monochrome(vec![glyph(0.0, 0.0, RED)]),
        Paint::Polychrome(opaque_image),
    ] {
        let (mut app, win) = rows_under_the_edge(&[row.clone()]);
        let q = queue(&mut app, win.id(), 0);
        let c = q.opaque.commands.iter().find(|c| c.z == 4.0);
        assert_eq!(c.map(|c| c.background), Some(WHITE), "{row:?}");
        assert!(q.translucent.commands.is_empty(), "{row:?}");
    }
}

#[test]
fn a_scroll_damages_the_rows_that_moved_not_the_viewport() {
    let mut app = app();
    let win = app.spawn(app.root(), a_window());
    let (list, _) = a_list(&mut app, win, Paint::Quad(Quad::new(WHITE)), true, 3);
    app.tick();
    scroll_to(&mut app, list, 0.0, 10.0);
    app.tick();
    let q = queue(&mut app, win.id(), 1);
    let padding_box = Rect::new(2.0, 2.0, 96.0, 46.0);
    assert!(!q.scissor.is_empty());
    assert!(
        q.scissor.iter().all(|r| rect_within(*r, padding_box)),
        "only what moved inside the list: {:?}",
        q.scissor
    );
}

fn rect_within(r: Rect, outer: Rect) -> bool {
    r.x() >= outer.x()
        && r.y() >= outer.y()
        && r.right() <= outer.right()
        && r.bottom() <= outer.bottom()
}
