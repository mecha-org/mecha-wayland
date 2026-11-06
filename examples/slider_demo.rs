//! A slider example demonstrating the Mechanix Design tick-bar slider with multiple sizes,
//! horizontal and vertical orientations, press-and-drag interaction, and theme-aware styling.
//! Run under a Wayland session: `cargo run --example slider_demo`.

use mecha_wayland::mechanix_widgets::prelude::{
    ButtonVariant, FontBook, TextContextExt, button, text,
};
use mecha_wayland::prelude::*;
use mechanix_widgets::{SliderChange, SliderSize, slider};
use theme::{ColorRole, TextVariant};

struct DemoView;

struct DemoViewBuilder;

impl Build for DemoViewBuilder {
    type Widget = DemoView;
}

impl Widget for DemoView {
    type Builder = DemoViewBuilder;

    fn build(_b: DemoViewBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        *s.component_mut::<LayoutStyle>(me).unwrap() = LayoutStyle::default()
            .column()
            .center()
            .fill()
            .padding_all(px(20.0))
            .gap(px(14.0));
        *s.component_mut::<Paint>(me).unwrap() =
            Paint::Quad(Quad::new(s.color(ColorRole::Surface)));

        // Title
        s.spawn(
            me,
            text("Mechanix Slider Variants")
                .variant(TextVariant::TitleLarge)
                .color(ColorRole::OnSurface),
        );

        // Section 1: Horizontal Sliders (Different Sizes)
        let h_section = s.spawn(
            me,
            div().style(
                LayoutStyle::default()
                    .column()
                    .center()
                    .gap(px(10.0))
                    .width(percent(100.0)),
            ),
        );

        // 1. XSmall (16 px)
        let row_xs = s.spawn(
            h_section,
            div().style(LayoutStyle::default().column().center().gap(px(4.0))),
        );
        let label_xs = s.spawn(
            row_xs,
            text("XSmall (16 px): 25%")
                .variant(TextVariant::LabelMedium)
                .color(ColorRole::OnSurface),
        );
        let s_xs = s.spawn(
            row_xs,
            slider()
                .size(SliderSize::XSmall)
                .value(0.25)
                .range(0.0, 1.0)
                .length(px(200.0)),
        );
        s.on::<SliderChange>(s_xs, move |ctx, e| {
            let pct = (e.value * 100.0).round() as i32;
            if let Some(mut t) = ctx.at(label_xs) {
                t.set_text(format!("XSmall (16 px): {}%", pct));
            }
        });

        // 2. Small (24 px)
        let row_sm = s.spawn(
            h_section,
            div().style(LayoutStyle::default().column().center().gap(px(4.0))),
        );
        let label_sm = s.spawn(
            row_sm,
            text("Small (24 px): 50%")
                .variant(TextVariant::LabelMedium)
                .color(ColorRole::OnSurface),
        );
        let s_sm = s.spawn(
            row_sm,
            slider()
                .size(SliderSize::Small)
                .value(0.50)
                .range(0.0, 1.0)
                .length(px(240.0))
                .active_color(ColorRole::Primary),
        );
        s.on::<SliderChange>(s_sm, move |ctx, e| {
            let pct = (e.value * 100.0).round() as i32;
            if let Some(mut t) = ctx.at(label_sm) {
                t.set_text(format!("Small (24 px): {}%", pct));
            }
        });

        // 3. Medium (40 px) - Default theme colors (on-secondary-fixed / secondary-fixed-dim)
        // Full-width row so the slider's percent length resolves against it.
        let row_md = s.spawn(
            h_section,
            div().style(
                LayoutStyle::default()
                    .column()
                    .center()
                    .gap(px(4.0))
                    .width(percent(100.0)),
            ),
        );
        let label_md = s.spawn(
            row_md,
            text("Medium (40 px, 80% wide): 70%")
                .variant(TextVariant::LabelMedium)
                .color(ColorRole::OnSurface),
        );
        let s_md = s.spawn(
            row_md,
            slider()
                .size(SliderSize::Medium)
                .value(0.70)
                .range(0.0, 1.0)
                .length(percent(80.0)),
        );
        s.on::<SliderChange>(s_md, move |ctx, e| {
            let pct = (e.value * 100.0).round() as i32;
            if let Some(mut t) = ctx.at(label_md) {
                t.set_text(format!("Medium (40 px, 80% wide): {}%", pct));
            }
        });

        // 4. Large (56 px)
        let row_lg = s.spawn(
            h_section,
            div().style(LayoutStyle::default().column().center().gap(px(4.0))),
        );
        let label_lg = s.spawn(
            row_lg,
            text("Large (56 px): 85%")
                .variant(TextVariant::LabelMedium)
                .color(ColorRole::OnSurface),
        );
        let s_lg = s.spawn(
            row_lg,
            slider()
                .size(SliderSize::Large)
                .value(0.85)
                .range(0.0, 1.0)
                .length(px(320.0))
                .active_color(ColorRole::Primary),
        );
        s.on::<SliderChange>(s_lg, move |ctx, e| {
            let pct = (e.value * 100.0).round() as i32;
            if let Some(mut t) = ctx.at(label_lg) {
                t.set_text(format!("Large (56 px): {}%", pct));
            }
        });

        // Section 2: Vertical Sliders Side-by-Side (Audio Mixer / Equalizer)
        s.spawn(
            me,
            text("Vertical Sliders (Audio Mixer)")
                .variant(TextVariant::TitleMedium)
                .color(ColorRole::OnSurface),
        );

        let v_row = s.spawn(
            me,
            div().style(LayoutStyle::default().row().center().gap(px(24.0))),
        );

        // Vertical 1: Small (24px wide)
        let v1_col = s.spawn(
            v_row,
            div().style(LayoutStyle::default().column().center().gap(px(6.0))),
        );
        let v1_lbl = s.spawn(
            v1_col,
            text("Vol: 45%")
                .variant(TextVariant::LabelSmall)
                .color(ColorRole::OnSurface),
        );
        let v1 = s.spawn(
            v1_col,
            slider()
                .size(SliderSize::Small)
                .vertical()
                .value(0.45)
                .range(0.0, 1.0)
                .length(px(180.0)),
        );
        s.on::<SliderChange>(v1, move |ctx, e| {
            let pct = (e.value * 100.0).round() as i32;
            if let Some(mut t) = ctx.at(v1_lbl) {
                t.set_text(format!("Vol: {}%", pct));
            }
        });

        // Vertical 2: Medium (40px wide)
        let v2_col = s.spawn(
            v_row,
            div().style(LayoutStyle::default().column().center().gap(px(6.0))),
        );
        let v2_lbl = s.spawn(
            v2_col,
            text("Bass: 60%")
                .variant(TextVariant::LabelSmall)
                .color(ColorRole::OnSurface),
        );
        let v2 = s.spawn(
            v2_col,
            slider()
                .size(SliderSize::Medium)
                .vertical()
                .value(0.60)
                .range(0.0, 1.0)
                .length(px(180.0))
                .active_color(ColorRole::Primary),
        );
        s.on::<SliderChange>(v2, move |ctx, e| {
            let pct = (e.value * 100.0).round() as i32;
            if let Some(mut t) = ctx.at(v2_lbl) {
                t.set_text(format!("Bass: {}%", pct));
            }
        });

        // Vertical 3: Large (56px wide)
        let v3_col = s.spawn(
            v_row,
            div().style(LayoutStyle::default().column().center().gap(px(6.0))),
        );
        let v3_lbl = s.spawn(
            v3_col,
            text("Gain: 75%")
                .variant(TextVariant::LabelSmall)
                .color(ColorRole::OnSurface),
        );
        let v3 = s.spawn(
            v3_col,
            slider()
                .size(SliderSize::Large)
                .vertical()
                .value(0.75)
                .range(0.0, 1.0)
                .length(px(180.0))
                .active_color(ColorRole::Secondary),
        );
        s.on::<SliderChange>(v3, move |ctx, e| {
            let pct = (e.value * 100.0).round() as i32;
            if let Some(mut t) = ctx.at(v3_lbl) {
                t.set_text(format!("Gain: {}%", pct));
            }
        });

        // Section 3: Theme Toggle Button
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
                .layout(LayoutStyle::default().center().size(px(520.0), px(760.0))),
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
