//! The Mechanix tick-bar slider, in three sliders that each show one thing:
//! a fixed-size slider, a responsive one whose track and ticks are sized in
//! `percent` (resize the window), and a vertical one with custom `px` ticks.
//! Drag any slider; the theme button re-themes them.
//! Run under a Wayland session: `cargo run --example slider_demo`.

use mecha_wayland::mechanix_widgets::prelude::{
    ButtonVariant, FontBook, TextContextExt, button, text,
};
use mecha_wayland::prelude::*;
use mechanix_widgets::{SliderBuilder, SliderChange, SliderSize, slider};
use theme::{ColorRole, TextVariant};

struct DemoView;

struct DemoViewBuilder;

impl Build for DemoViewBuilder {
    type Widget = DemoView;
}

/// Spawns `slider` under a label that tracks its value as a percent.
fn labeled_slider(
    s: &mut Spawner<'_, DemoView>,
    parent: impl Into<NodeId>,
    name: &'static str,
    slider: SliderBuilder,
) {
    let percent_of = |v: f32| (v * 100.0).round() as i32;
    // Full width, so a `percent` slider length resolves against the view.
    let col = s.spawn(
        parent,
        div().style(
            LayoutStyle::default()
                .column()
                .center()
                .gap(px(6.0))
                .width(percent(100.0)),
        ),
    );
    let label = s.spawn(
        col,
        text(format!("{name}: {}%", percent_of(slider.value)))
            .variant(TextVariant::LabelMedium)
            .color(ColorRole::OnSurface),
    );
    let handle = s.spawn(col, slider);
    s.on::<SliderChange>(handle, move |ctx, e| {
        if let Some(mut t) = ctx.at(label) {
            t.set_text(format!("{name}: {}%", percent_of(e.value)));
        }
    });
}

impl Widget for DemoView {
    type Builder = DemoViewBuilder;

    fn build(_b: DemoViewBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        *s.component_mut::<LayoutStyle>(me).unwrap() = LayoutStyle::default()
            .column()
            .center()
            .fill()
            .padding_all(px(20.0))
            .gap(px(24.0));
        *s.component_mut::<Paint>(me).unwrap() =
            Paint::Quad(Quad::new(s.color(ColorRole::Surface)));

        s.spawn(
            me,
            text("Mechanix Slider")
                .variant(TextVariant::TitleLarge)
                .color(ColorRole::OnSurface),
        );

        // 1. Fixed: a size variant on a px track; the defaults do the rest.
        labeled_slider(
            s,
            me,
            "Brightness",
            slider()
                .size(SliderSize::Medium)
                .length(px(320.0))
                .value(0.6),
        );

        // 2. Responsive: the track is 80% of the view, each tick 1% of the
        // track and 75% of the slider's height. All re-resolve on resize;
        // tick widths snap to whole pixels so they stay even.
        labeled_slider(
            s,
            me,
            "Volume (responsive)",
            slider()
                .length(percent(80.0))
                .tick_width(percent(1.0))
                .tick_height(percent(75.0))
                .tick_gap(6.0)
                .tick_radius(2.0)
                // A percent tick height needs a definite slider height.
                .style(LayoutStyle::default().height(px(48.0)))
                .value(0.4),
        );

        // 3. Vertical with custom px ticks. Explicit tick values win over the
        // size variant whatever the call order.
        labeled_slider(
            s,
            me,
            "Gain (vertical)",
            slider()
                .tick_size(px(32.0), px(4.0))
                .tick_radius(2.0)
                .size(SliderSize::Large)
                .vertical()
                .length(px(200.0))
                .value(0.75),
        );

        let toggle = s.spawn(
            me,
            button()
                .variant(ButtonVariant::Outlined)
                .label("Toggle Theme"),
        );

        s.on::<Clicked>(toggle, move |ctx, _| {
            let next = match ctx.theme().mode() {
                ThemeMode::Dark => MechanixTheme::light(),
                ThemeMode::Light => MechanixTheme::dark(),
            };
            ctx.set_theme(next);
        });

        s.on_theme(me, move |ctx| {
            let bg = ctx.color(ColorRole::Surface);
            ctx.set_paint(Paint::Quad(Quad::new(bg)));
        });

        DemoView
    }
}

struct Shell;

struct ShellBuilder {
    root: NodeId,
}

impl Build for ShellBuilder {
    type Widget = Shell;
}

impl Widget for Shell {
    type Builder = ShellBuilder;

    fn build(b: ShellBuilder, _me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        let win = s.spawn(
            b.root,
            window()
                .title("Mechanix Slider Demo")
                .layout(LayoutStyle::default().center().size(px(520.0), px(640.0))),
        );
        s.on::<CloseRequested>(win, |ctx, _| ctx.signal(Stop));
        s.spawn(win, DemoViewBuilder);
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
        .add_module(RenderModule::default())
        .insert_resource(Atlas::new());

    app.add_module(MechanixTheme::dark());

    let font = app
        .resource_mut::<Atlas>()
        .add_font(include_bytes!(
            "../crates/atlas/tests/fixtures/Inter-Regular.ttf"
        ))
        .expect("Inter loads");
    app.insert_resource(FontBook::new(font));

    app.add_module(RingModule::default())
        .add_module(
            WaylandModule::new()
                .bind::<WlCompositor>()
                .bind::<ZwpLinuxDmabufV1>()
                .bind::<XdgWmBase>()
                .bind::<WlSeat>(),
        )
        .add_module(PresentationModule {
            app_id: "mecha.slider_demo".into(),
            budget: Budget::default(),
        });

    let root = app.root();
    app.spawn(root, ShellBuilder { root });
    app.run();
}
