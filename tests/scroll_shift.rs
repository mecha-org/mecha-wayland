//! A scroll drawn by shifting the last frame is the frame a full redraw
//! gives, byte for byte: the whole pipeline (layout, paint, render's walk,
//! gles) draws each case both ways and compares. Skips without a GPU.

use std::sync::Mutex;

use mecha_wayland::prelude::*;

/// One GPU context at a time.
static LOCK: Mutex<()> = Mutex::new(());

const W: f32 = 270.0;
const H: f32 = 310.0;
const GREY: Color = Color::rgb(0.12, 0.12, 0.14);
const BLUE: Color = Color::rgb(0.25, 0.5, 0.9);

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

fn answer(app: &mut App, r: &FrameRequested) {
    app.signal(Frame(r.0));
}

fn at(x: f32, y: f32, w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::default()
        .absolute()
        .inset(Insets {
            top: px(y),
            left: px(x),
            right: auto(),
            bottom: auto(),
        })
        .size(px(w), px(h))
}

/// A 32 by 32 opaque gradient with a hard diagonal edge.
fn picture(app: &mut App) -> SpriteId {
    let n = 32u32;
    let mut pixels = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let on = x > y;
            pixels.extend_from_slice(&[
                (x * 8) as u8,
                if on { 200 } else { 40 },
                (y * 8) as u8,
                255,
            ]);
        }
    }
    app.resource_mut::<Atlas>()
        .insert(
            Class::Image,
            &Bitmap {
                width: n,
                height: n,
                format: Format::Rgba8,
                pixels,
            },
        )
        .unwrap()
}

/// A Comet-half window at scale 2, cleared to grey, with a clipping list
/// at `list` (logical) on a grey background: rounded rows of text, every
/// third with an image. `axis` lays the rows out down or across.
struct Case {
    app: App,
    win: NodeId,
    list: NodeId,
}

fn case(list: LayoutStyle, horizontal: bool, header: bool) -> Case {
    let mut app = App::new();
    app.add_module(LayoutModule)
        .add_module(PaintModule)
        .add_module(WindowModule)
        .add_module(InteractivityModule)
        .add_module(AnimationModule)
        .add_module(ScrollModule)
        .add_module(RenderModule { buffers: 2 })
        .system(answer);
    app.insert_resource(Atlas::new());
    let font = app
        .resource_mut::<Atlas>()
        .add_font(include_bytes!(
            "../crates/atlas/tests/fixtures/Inter-Regular.ttf"
        ))
        .unwrap();
    let p = picture(&mut app);
    let win = app
        .spawn(
            app.root(),
            window()
                .clear(GREY)
                .layout(LayoutStyle::default().size(px(W), px(H))),
        )
        .id();
    if header {
        app.spawn_with(
            win,
            Leaf,
            (
                at(0.0, 0.0, W, 40.0),
                Paint::Quad(Quad::new(BLUE).radius(10.0)),
            ),
        );
    }
    let style = if horizontal {
        list.row()
    } else {
        list.column()
    };
    let l = app
        .spawn_with(
            win,
            Leaf,
            (
                style,
                Paint::Quad(Quad::new(GREY)),
                Viewport {
                    clip: true,
                    scroll: Point::default(),
                },
            ),
        )
        .id();
    for n in 0..24 {
        let size = if horizontal {
            LayoutStyle::default().size(px(90.0), px(H)).shrink(0.0)
        } else {
            LayoutStyle::default().size(px(W), px(47.0)).shrink(0.0)
        };
        let row = app
            .spawn_with(
                l,
                Leaf,
                (
                    size.row().padding_all(px(6.0)),
                    Paint::Quad(
                        Quad::new(Color::rgb(0.2, 0.25, 0.3 + n as f32 * 0.02)).radius(9.0),
                    ),
                ),
            )
            .id();
        if n % 3 == 0 {
            app.spawn(
                row,
                image(p)
                    .radius(6.0)
                    .style(LayoutStyle::default().size(px(30.0), px(30.0))),
            );
        }
        app.spawn(row, text(font, format!("Row {n} jumps")).size(17));
    }
    app.tick();
    app.emit(ScaleFactorChanged { scale: 2.0 }, win);
    app.flush();
    app.tick();
    Case { app, win, list: l }
}

fn list() -> LayoutStyle {
    at(0.0, 0.0, W, H)
}

impl Case {
    fn queue(&mut self) -> Queue {
        self.app
            .resource_mut::<Scenes>()
            .queue(self.win, 0)
            .expect("a scene")
            .clone()
    }

    fn scroll(&mut self, x: f32, y: f32) {
        self.app
            .component_mut::<Viewport>(self.list)
            .unwrap()
            .scroll = Point::new(x, y);
        self.app.tick();
        self.app.tick();
    }

    /// From scroll `from` to `to`: draw `from` whole into A, present it,
    /// scroll, then shift A into B and draw the strip, and draw `to` whole
    /// into C. B must equal C.
    fn check(mut self, from: (f32, f32), to: (f32, f32)) {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut d = match Device::try_open(Budget::default()) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("skip: {e:?}");
                return;
            }
        };
        self.scroll(from.0, from.1);
        let first = self.queue();
        let (w, h) = (first.size.width as u32, first.size.height as u32);
        let (a, b, c) = (
            d.target(w, h, &[]),
            d.target(w, h, &[]),
            d.target(w, h, &[]),
        );
        d.upload(&self.app.resource::<Atlas>());
        d.draw(&a, &first);
        self.app.resource_mut::<Scenes>().presented(self.win);

        self.scroll(to.0, to.1);
        let shift = self
            .app
            .resource::<Scenes>()
            .shift(self.win)
            .expect("a pure scroll offers a shift");
        let strip = self
            .app
            .resource_mut::<Scenes>()
            .queue_shifted(self.win, shift)
            .expect("a scene")
            .clone();
        let whole = self.queue();
        d.upload(&self.app.resource::<Atlas>());
        d.shift(&a, &b, shift);
        d.draw(&b, &strip);
        d.draw(&c, &whole);
        let (got, want) = (d.read(&b), d.read(&c));
        for t in [a, b, c] {
            d.destroy(t);
        }
        // Exact on the Comet's GPU, the target. Desktop GPUs (radeonsi,
        // llvmpipe) round a texture sample at an image's rounded corner one
        // step differently after an odd vertical move: allow one step there.
        let tolerance = if cfg!(target_arch = "aarch64") { 0 } else { 1 };
        let worst = (0..got.len())
            .filter(|i| i % 4 != 3)
            .map(|i| (got[i].abs_diff(want[i]), i))
            .max()
            .unwrap_or((0, 0));
        if worst.0 > tolerance {
            let p = worst.1 / 4;
            panic!(
                "{shift:?}: off by {} at ({}, {}): shifted {:?}, redrawn {:?}",
                worst.0,
                p as u32 % w,
                p as u32 / w,
                &got[p * 4..p * 4 + 4],
                &want[p * 4..p * 4 + 4]
            );
        }
    }
}

#[test]
fn scrolling_down_shifts_to_the_same_pixels_as_a_redraw() {
    case(list(), false, false).check((0.0, 40.0), (0.0, 61.0));
}

#[test]
fn scrolling_up_shifts_to_the_same_pixels_as_a_redraw() {
    case(list(), false, false).check((0.0, 140.0), (0.0, 97.5));
}

#[test]
fn a_horizontal_list_shifts_to_the_same_pixels() {
    case(list(), true, false).check((100.0, 0.0), (131.0, 0.0));
}

#[test]
fn a_diagonal_move_shifts_to_the_same_pixels() {
    // A list wider than its box as well as taller: rows 300 wide.
    let mut c = case(list(), false, false);
    let rows: Vec<NodeId> = c.app.tree().children(c.list).unwrap().to_vec();
    for r in rows {
        c.app.component_mut::<LayoutStyle>(r).map(|mut s| {
            *s = s.clone().size(px(300.0), px(47.0));
        });
    }
    c.app.tick();
    c.check((10.0, 40.0), (17.5, 52.0));
}

#[test]
fn rows_crossing_the_strip_edge_shift_to_the_same_pixels() {
    // 3.5 logical is 7 device pixels: rows, glyphs and images straddle
    // the strip's edge.
    case(list(), false, false).check((0.0, 20.0), (0.0, 23.5));
}

#[test]
fn a_list_smaller_than_the_window_keeps_the_rest_unchanged() {
    case(at(0.0, 50.0, W, 200.0), false, true).check((0.0, 30.0), (0.0, 44.0));
}

#[test]
fn a_shift_in_the_overscroll_spring_matches() {
    case(list(), false, false).check((0.0, -30.0), (0.0, -12.5));
}
