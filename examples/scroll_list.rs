//! A hundred tappable rows in a vertical `scrollable()`, for trying the
//! touch physics on a device. Run under a Wayland session:
//! `cargo run --example scroll_list`.
//!
//! Each `SCROLL_*` environment variable overrides one `scroll::Tuning`
//! field: `SCROLL_SLOP`, `SCROLL_MIN_FLING`, `SCROLL_MAX_FLING`,
//! `SCROLL_FRICTION`, `SCROLL_OVERSCROLL_FRICTION`,
//! `SCROLL_SPRING_STIFFNESS`, `SCROLL_SPRING_RATIO`,
//! `SCROLL_RUBBER_HALF_LIFE`. Logs go to stderr: `clicked row N`,
//! `fling v=... px/s` after a release and `settled at ...` when the glide
//! ends.

use std::time::Instant;

use mecha_wayland::prelude::*;
use scroll::Tuning;

/// A clickable box with a text label. It does nothing on its own;
/// the caller wires `Clicked` on the handle `button` returns.
struct Button;

fn button(font: FontId, label: impl Into<String>) -> ButtonBuilder {
    ButtonBuilder {
        font,
        label: label.into(),
    }
}

struct ButtonBuilder {
    font: FontId,
    label: String,
}

impl Build for ButtonBuilder {
    type Widget = Button;
}

impl Widget for Button {
    type Builder = ButtonBuilder;
    fn build(b: ButtonBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        *s.component_mut::<LayoutStyle>(me).unwrap() =
            LayoutStyle::default().center().padding_all(px(12.0));
        *s.component_mut::<Paint>(me).unwrap() =
            Paint::Quad(Quad::new(Color::rgb(0.25, 0.5, 0.9)).radius(6.0));
        s.spawn(me, text(b.font, b.label).size(18));
        Button
    }
}

/// Tuning with any `SCROLL_*` variable that parses applied over the default.
fn tuning_from_env() -> Tuning {
    fn var(name: &str, field: &mut f64) {
        if let Some(v) = std::env::var(name).ok().and_then(|v| v.parse().ok()) {
            *field = v;
        }
    }
    let mut t = Tuning::default();
    var("SCROLL_SLOP", &mut t.slop);
    var("SCROLL_MIN_FLING", &mut t.min_fling);
    var("SCROLL_MAX_FLING", &mut t.max_fling);
    var("SCROLL_FRICTION", &mut t.friction_drag);
    var("SCROLL_OVERSCROLL_FRICTION", &mut t.overscroll_friction);
    var("SCROLL_SPRING_STIFFNESS", &mut t.spring_stiffness);
    var("SCROLL_SPRING_RATIO", &mut t.spring_ratio);
    var("SCROLL_RUBBER_HALF_LIFE", &mut t.rubber_band_half_life);
    t
}

/// What the logger saw of the list on the last frame. The `Scrollable`
/// has no offset or event API, so motion is read off its `Viewport`.
#[derive(Default)]
struct Motion {
    last: Option<(Instant, f32)>,
    released: bool,
    moving: bool,
}

impl Resource for Motion {}

fn list(app: &mut App) -> Option<NodeId> {
    app.widgets::<Scrollable>().map(|(id, _)| id).next()
}

fn on_release(app: &mut App, _: &Emitted<Release>) {
    app.resource_mut::<Motion>().released = true;
}

/// Speed between frames stands in for the release velocity: the first
/// frame after a release logs it, and a glide that has all but stopped
/// logs where it rests.
fn on_frame(app: &mut App, _: &Frame) {
    let Some(id) = list(app) else { return };
    let y = app.component::<Viewport>(id).map_or(0.0, |v| v.scroll.y);
    let now = Instant::now();
    let mut m = app.resource_mut::<Motion>();
    if let Some((then, prev)) = m.last {
        let dt = now.duration_since(then).as_secs_f32();
        let v = if dt > 0.0 { (y - prev) / dt } else { 0.0 };
        if m.released {
            m.released = false;
            m.moving = true;
            eprintln!("fling v={v:.0} px/s");
        } else if m.moving && (y - prev).abs() < 0.05 {
            m.moving = false;
            eprintln!("settled at {y:.1}");
        }
    }
    m.last = Some((now, y));
}

/// Spawns the window and the list; stops the app on close.
struct Shell;
struct ShellBuilder {
    root: NodeId,
    font: FontId,
}
impl Build for ShellBuilder {
    type Widget = Shell;
}
impl Widget for Shell {
    type Builder = ShellBuilder;
    fn build(b: ShellBuilder, _me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        // A layer surface anchored to all four edges with no size of its
        // own: the compositor stretches it to the output, in logical px.
        let role = Role::Layer(LayerRole {
            layer: Layer::Top,
            anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            exclusive_zone: -1,
            namespace: "scroll_list".into(),
            keyboard_interactivity: KeyboardInteractivity::None,
        });
        let win = s.spawn_with(
            b.root,
            window()
                .title("scroll_list")
                .clear(Color::rgb(0.12, 0.12, 0.14))
                .layout(LayoutStyle::default().column().size(auto(), auto())),
            (role,),
        );
        s.on::<CloseRequested>(win, |ctx, _| ctx.signal(Stop));
        let list = s.spawn(
            win,
            scrollable().style(LayoutStyle::default().block().fill()),
        );
        for n in 1..=100 {
            let row = s.spawn(list, button(b.font, format!("Row {n}")));
            s.on::<Clicked>(row, move |_, _| eprintln!("clicked row {n}"));
        }
        Shell
    }
}

fn main() {
    let mut app = App::new();
    app.add_module(LayoutModule)
        .add_module(PaintModule)
        .add_module(WindowModule)
        .add_module(InteractivityModule)
        .add_module(AnimationModule)
        .insert_resource(tuning_from_env());
    app.add_module(ScrollModule)
        .add_module(RenderModule::default())
        .insert_resource(Atlas::new());
    app.insert_resource(Motion::default());
    app.system(on_release).system(on_frame);
    app.add_module(RingModule::default())
        .add_module(
            WaylandModule::new()
                .bind::<WlCompositor>()
                .bind::<ZwpLinuxDmabufV1>()
                .bind::<XdgWmBase>()
                .bind::<WlSeat>(),
        )
        .add_module(PresentationModule {
            app_id: "mecha.scroll_list".into(),
            budget: Budget::default(),
        });

    let font = app
        .resource_mut::<Atlas>()
        .add_font(include_bytes!(
            "../crates/atlas/tests/fixtures/Inter-Regular.ttf"
        ))
        .expect("Inter loads");

    let root = app.root();
    app.spawn(root, ShellBuilder { root, font });
    app.run();
}
