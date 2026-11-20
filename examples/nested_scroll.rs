//! Scrollables inside a scrollable, all `Overscroll::Bounce`: a vertical
//! list and a horizontal strip nested in a vertical page.
//! Run under a Wayland session: `cargo run --example nested_scroll`.
//! A tap logs `clicked <label>` to stderr.

use mecha_wayland::prelude::*;

const ROW: Color = Color::rgb(0.25, 0.5, 0.9);
const INNER: Color = Color::rgb(0.2, 0.6, 0.4);
const STRIP: Color = Color::rgb(0.75, 0.45, 0.2);
const PANEL: Color = Color::rgb(0.2, 0.2, 0.24);

/// A tappable coloured box with a label that logs its taps.
fn item(
    s: &mut Spawner<'_, Shell>,
    parent: NodeId,
    font: FontId,
    style: LayoutStyle,
    color: Color,
    label: String,
) {
    let b = s.spawn(
        parent,
        div()
            .style(style.center().padding_all(px(12.0)))
            .background(color)
            .radius(6.0),
    );
    s.spawn(b, text(font, label.clone()).size(18));
    s.on::<Clicked>(b, move |_, _| eprintln!("clicked {label}"));
}

/// A titled panel holding `scroll`; returns the scrollable.
fn panel(
    s: &mut Spawner<'_, Shell>,
    page: NodeId,
    font: FontId,
    title: &str,
    scroll: ScrollableBuilder,
) -> NodeId {
    let p = s.spawn(
        page,
        div()
            .style(LayoutStyle::default().block().padding_all(px(8.0)))
            .background(PANEL)
            .radius(8.0),
    );
    s.spawn(p, text(font, title).size(16));
    s.spawn(p, scroll).id()
}

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
        let role = Role::Layer(LayerRole {
            layer: Layer::Top,
            anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            exclusive_zone: -1,
            namespace: "nested_scroll".into(),
            keyboard_interactivity: KeyboardInteractivity::None,
        });
        let win = s.spawn_with(
            b.root,
            window()
                .title("nested_scroll")
                .clear(Color::rgb(0.12, 0.12, 0.14))
                .layout(LayoutStyle::default().column().size(auto(), auto())),
            (role,),
        );
        s.on::<CloseRequested>(win, |ctx, _| ctx.signal(Stop));

        let font = b.font;
        let page = s
            .spawn(
                win,
                scrollable().style(LayoutStyle::default().block().fill()),
            )
            .id();
        let rows = |s: &mut Spawner<'_, Shell>, from: u32, to: u32| {
            for n in from..=to {
                item(
                    s,
                    page,
                    font,
                    LayoutStyle::default(),
                    ROW,
                    format!("page {n}"),
                );
            }
        };

        rows(s, 1, 4);
        let inner = panel(
            s,
            page,
            font,
            "Inner vertical list",
            scrollable().style(LayoutStyle::default().block().height(px(300.0))),
        );
        for n in 1..=30 {
            item(
                s,
                inner,
                font,
                LayoutStyle::default(),
                INNER,
                format!("inner {n}"),
            );
        }

        rows(s, 5, 8);
        let strip = panel(
            s,
            page,
            font,
            "Horizontal strip",
            scrollable()
                .axis(Axis::Horizontal)
                .style(LayoutStyle::default().row().gap(px(8.0)).height(px(80.0))),
        );
        for n in 1..=20 {
            let style = LayoutStyle::default().shrink(0.0).width(px(120.0));
            item(s, strip, font, style, STRIP, format!("strip {n}"));
        }

        rows(s, 9, 40);
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
        .add_module(ScrollModule)
        .add_module(RenderModule::default())
        .insert_resource(Atlas::new());
    app.add_module(RingModule::default())
        .add_module(
            WaylandModule::new()
                .bind::<WlCompositor>()
                .bind::<ZwpLinuxDmabufV1>()
                .bind::<XdgWmBase>()
                .bind::<WlSeat>(),
        )
        .add_module(PresentationModule {
            app_id: "mecha.nested_scroll".into(),
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
