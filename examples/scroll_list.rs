//! A hundred tappable rows in a vertical `scrollable()`, for trying the
//! touch physics on a device. Run under a Wayland session:
//! `cargo run --example scroll_list`.
//!
//! Each `SCROLL_*` environment variable overrides one `scroll::Tuning`
//! field: `SCROLL_SLOP`, `SCROLL_MIN_FLING`, `SCROLL_MAX_FLING`,
//! `SCROLL_FRICTION`, `SCROLL_OVERSCROLL_FRICTION`,
//! `SCROLL_SPRING_STIFFNESS`, `SCROLL_SPRING_RATIO`,
//! `SCROLL_RUBBER_HALF_LIFE`; a value out of its field's range is
//! ignored with a warning. Logs go to stderr: `clicked row N`,
//! `fling v=... px/s` on releasing a contact that moved past the slop,
//! and `settled at ...` when the glide after it ends.

use mecha_wayland::prelude::*;
use scroll::{Tuning, VelocityTracker};

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

/// Tuning with any `SCROLL_*` variable that parses and is in range
/// applied over the default.
fn tuning_from_env() -> Tuning {
    fn var(name: &str, field: &mut f64, valid: fn(f64) -> bool, range: &str) {
        let Some(v) = std::env::var(name).ok().and_then(|v| v.parse().ok()) else {
            return;
        };
        if valid(v) {
            *field = v;
        } else {
            eprintln!("ignoring {name}={v}: must be {range}");
        }
    }
    let any = |_: f64| true;
    let positive = |v: f64| v > 0.0;
    let mut t = Tuning::default();
    var("SCROLL_SLOP", &mut t.slop, any, "a number");
    var("SCROLL_MIN_FLING", &mut t.min_fling, positive, "> 0");
    var("SCROLL_MAX_FLING", &mut t.max_fling, positive, "> 0");
    var(
        "SCROLL_FRICTION",
        &mut t.friction_drag,
        |v| v > 0.0 && v < 1.0,
        "in (0, 1)",
    );
    var(
        "SCROLL_OVERSCROLL_FRICTION",
        &mut t.overscroll_friction,
        any,
        "a number",
    );
    var(
        "SCROLL_SPRING_STIFFNESS",
        &mut t.spring_stiffness,
        positive,
        "> 0",
    );
    var("SCROLL_SPRING_RATIO", &mut t.spring_ratio, positive, "> 0");
    var(
        "SCROLL_RUBBER_HALF_LIFE",
        &mut t.rubber_band_half_life,
        positive,
        "> 0",
    );
    t
}

/// What the logger saw of the list. The `Scrollable` has no offset or
/// velocity API, so the release speed is estimated here from the same
/// events with the same tracker, and the rest read off its `Viewport`.
#[derive(Default)]
struct Motion {
    /// Each contact down on the list.
    strokes: Vec<Stroke>,
    /// A release that flung was seen; the next frames watch the glide.
    released: bool,
    gliding: bool,
    last: Option<f32>,
}

struct Stroke {
    contact: ContactId,
    origin: f32,
    moved: bool,
    tracker: VelocityTracker,
}

impl Resource for Motion {}

fn list(app: &mut App) -> Option<NodeId> {
    app.widgets::<Scrollable>().map(|(id, _)| id).next()
}

/// Whether `targets` reached the list.
fn on_list(app: &mut App, targets: &[NodeId]) -> bool {
    list(app).is_some_and(|id| targets.contains(&id))
}

fn on_press(app: &mut App, e: &Emitted<Press>) {
    if !on_list(app, &e.targets) {
        return;
    }
    let mut tracker = VelocityTracker::default();
    tracker.add(e.event.time, f64::from(e.event.position.y));
    let mut m = app.resource_mut::<Motion>();
    m.strokes.retain(|s| s.contact != e.event.contact);
    m.strokes.push(Stroke {
        contact: e.event.contact,
        origin: e.event.position.y,
        moved: false,
        tracker,
    });
}

fn on_press_moved(app: &mut App, e: &Emitted<PressMoved>) {
    if !on_list(app, &e.targets) {
        return;
    }
    let slop = app.resource::<Tuning>().slop;
    let mut m = app.resource_mut::<Motion>();
    let Some(s) = m.strokes.iter_mut().find(|s| s.contact == e.event.contact) else {
        return;
    };
    let y = e.event.position.y;
    s.tracker.add(e.event.time, f64::from(y));
    s.moved |= f64::from((y - s.origin).abs()) > slop;
}

/// A contact that moved past the slop logs its release speed as the
/// list's offset velocity: the offset moves against the finger.
fn on_release(app: &mut App, e: &Emitted<Release>) {
    let mut m = app.resource_mut::<Motion>();
    let Some(i) = m.strokes.iter().position(|s| s.contact == e.event.contact) else {
        return;
    };
    let s = m.strokes.swap_remove(i);
    if s.moved {
        let v = -s.tracker.velocity(e.event.time);
        eprintln!("fling v={v:.0} px/s");
        m.released = true;
    }
}

fn on_press_cancelled(app: &mut App, e: &Emitted<PressCancelled>) {
    if on_list(app, &e.targets) {
        let contact = e.event.contact;
        app.resource_mut::<Motion>()
            .strokes
            .retain(|s| s.contact != contact);
    }
}

/// After a release that flung, a glide that has all but stopped logs
/// where it rests.
fn on_frame(app: &mut App, _: &Frame) {
    let Some(id) = list(app) else { return };
    let y = app.component::<Viewport>(id).map_or(0.0, |v| v.scroll.y);
    let mut m = app.resource_mut::<Motion>();
    if m.released {
        // The glide starts timing on this frame; it moves from the next.
        m.released = false;
        m.gliding = true;
    } else if m.gliding && m.last.is_some_and(|prev| (y - prev).abs() < 0.05) {
        m.gliding = false;
        eprintln!("settled at {y:.1}");
    }
    m.last = Some(y);
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
    app.system(on_press)
        .system(on_press_moved)
        .system(on_release)
        .system(on_press_cancelled)
        .system(on_frame);
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
