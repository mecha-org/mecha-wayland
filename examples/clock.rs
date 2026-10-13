//! Demonstrates `Clock` in `mechanix-widgets`:
//! - Digital clocks used directly as widgets without any module installation
//! - Formats: 12-hour (with AM/PM), 24-hour, with/without seconds
//! - Full date and time formatting via chrono strftime patterns
//! - UTC world clock mode
//! - Text properties customized via the clock's inner text handle (`clock.text()`)
//! - Dynamic context updates via `ClockContextExt` (toggling format, seconds)
//! - Runtime theme switching (Dark / Light)
//!
//! Run under a Wayland session: `cargo run --example clock`.

use gles::Budget;
use mecha_wayland::mechanix_widgets::prelude::{
    ButtonVariant, ClockContextExt, FontBook, TextContextExt, button, clock, text,
};
use mecha_wayland::prelude::*;
use mechanix_widgets::ClockInterval;
use theme::{ColorRole, FontWeight, TextVariant};

struct ClockShowcase {
    is_12h: bool,
    show_seconds: bool,
}

struct ClockShowcaseBuilder;

impl Build for ClockShowcaseBuilder {
    type Widget = ClockShowcase;
}

impl Widget for ClockShowcase {
    type Builder = ClockShowcaseBuilder;

    fn build(_b: ClockShowcaseBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        *s.component_mut::<LayoutStyle>(me).unwrap() = LayoutStyle::default()
            .column()
            .center()
            .fill()
            .padding_all(px(24.0))
            .gap(px(16.0));
        *s.component_mut::<Paint>(me).unwrap() =
            Paint::Quad(Quad::new(s.color(ColorRole::Surface)));

        // Title and Header
        s.spawn(
            me,
            text("Mechanix Design - Clock Showcase")
                .variant(TextVariant::HeadlineSmall)
                .color(ColorRole::OnSurface),
        );

        s.spawn(
            me,
            text("Time-focused clock widgets with chrono formatting and text styling via text handle")
                .variant(TextVariant::BodyMedium)
                .color(ColorRole::OnSurfaceVariant),
        );

        // Header controls (Theme toggle)
        let theme_btn = s.spawn(
            me,
            button()
                .variant(ButtonVariant::Outlined)
                .label("Toggle Theme (Dark / Light)"),
        );

        // ── Card 1: Hero Large Live Clock ──────────────────────────────────
        let hero_card = s.spawn(
            me,
            div().style(
                LayoutStyle::default()
                    .column()
                    .center()
                    .padding_all(px(16.0))
                    .gap(px(6.0)),
            ),
        );
        s.spawn(
            hero_card,
            text("LOCAL TIME (STYLED VIA TEXT HANDLE)")
                .variant(TextVariant::LabelMedium)
                .color(ColorRole::Outline),
        );
        let hero_clock = s.spawn(hero_card, clock("%H:%M:%S"));

        // ── Card 2: Row of Variations (12h, 24h, UTC) ─────────────────────
        let row = s.spawn(
            me,
            div().style(LayoutStyle::default().row().center().gap(px(20.0))),
        );

        // 12-Hour Clock
        let c1 = s.spawn(
            row,
            div().style(
                LayoutStyle::default()
                    .column()
                    .center()
                    .padding_all(px(12.0))
                    .gap(px(4.0)),
            ),
        );

        s.spawn(
            c1,
            text("12-Hour Format")
                .variant(TextVariant::LabelSmall)
                .color(ColorRole::Outline),
        );
        let clock_12h = s.spawn(c1, clock("%H:%M").every_second());

        // 24-Hour Clock
        let c2 = s.spawn(
            row,
            div().style(
                LayoutStyle::default()
                    .column()
                    .center()
                    .padding_all(px(12.0))
                    .gap(px(4.0)),
            ),
        );

        s.spawn(
            c2,
            text("24-Hour Format")
                .variant(TextVariant::LabelSmall)
                .color(ColorRole::Outline),
        );
        let clock_24h = s.spawn(c2, clock("%H:%M:%S").interval(ClockInterval::Second));

        // UTC Clock
        let c3 = s.spawn(
            row,
            div().style(
                LayoutStyle::default()
                    .column()
                    .center()
                    .padding_all(px(12.0))
                    .gap(px(4.0)),
            ),
        );
        s.spawn(
            c3,
            text("UTC World Time")
                .variant(TextVariant::LabelSmall)
                .color(ColorRole::Outline),
        );
        let clock_utc = s.spawn(c3, clock("%H:%M:%S"));

        // ── Card 3: Full Date & Calendar Format ──────────────────────────
        let date_card = s.spawn(
            me,
            div().style(
                LayoutStyle::default()
                    .column()
                    .center()
                    .padding_all(px(12.0))
                    .gap(px(4.0)),
            ),
        );
        s.spawn(
            date_card,
            text("Calendar & Date Format")
                .variant(TextVariant::LabelSmall)
                .color(ColorRole::Outline),
        );
        let date_clock = s.spawn(date_card, clock("%A, %B %d, %Y • %I:%M:%S %p"));

        // ── Card 4: Interactive Clock Controls ───────────────────────────
        let interactive_card = s.spawn(
            me,
            div().style(
                LayoutStyle::default()
                    .column()
                    .center()
                    .padding_all(px(16.0))
                    .gap(px(8.0)),
            ),
        );
        s.spawn(
            interactive_card,
            text("Interactive Clock (Dynamic Time Updates via Context)")
                .variant(TextVariant::LabelSmall)
                .color(ColorRole::Outline),
        );

        let interactive_clock = s.spawn(interactive_card, clock("%I:%M:%S %p"));

        let btn_row = s.spawn(
            interactive_card,
            div().style(LayoutStyle::default().row().center().gap(px(8.0))),
        );
        let toggle_mode_btn = s.spawn(
            btn_row,
            button()
                .variant(ButtonVariant::Filled)
                .label("Toggle 12h / 24h"),
        );
        let toggle_sec_btn = s.spawn(
            btn_row,
            button()
                .variant(ButtonVariant::Tonal)
                .label("Toggle Seconds"),
        );

        // Theme toggle action
        s.on::<Clicked>(theme_btn, |ctx, _| {
            let next = match ctx.theme().mode() {
                ThemeMode::Dark => MechanixTheme::light(),
                ThemeMode::Light => MechanixTheme::dark(),
            };
            ctx.set_theme(next);
        });

        // Interactive clock format toggling
        s.on::<Clicked>(toggle_mode_btn, move |ctx, _| {
            ctx.me().is_12h = !ctx.me().is_12h;
            let is_12h = ctx.me().is_12h;
            let show_sec = ctx.me().show_seconds;

            let fmt = match (is_12h, show_sec) {
                (true, true) => "%I:%M:%S %p",
                (true, false) => "%I:%M %p",
                (false, true) => "%H:%M:%S",
                (false, false) => "%H:%M",
            };

            if let Some(mut clock_ctx) = ctx.at(interactive_clock) {
                clock_ctx.set_text("as");
                clock_ctx.set_format(fmt);
            }
        });

        s.on::<Clicked>(toggle_sec_btn, move |ctx, _| {
            ctx.me().show_seconds = !ctx.me().show_seconds;
            let is_12h = ctx.me().is_12h;
            let show_sec = ctx.me().show_seconds;

            let fmt = match (is_12h, show_sec) {
                (true, true) => "%I:%M:%S %p",
                (true, false) => "%I:%M %p",
                (false, true) => "%H:%M:%S",
                (false, false) => "%H:%M",
            };

            if let Some(mut clock_ctx) = ctx.at(interactive_clock) {
                clock_ctx.set_format(fmt);
            }
        });

        // Theme listener updates container background AND customizes clocks' text handles
        s.on_theme(me, move |ctx| {
            let bg = ctx.color(ColorRole::Surface);
            ctx.set_paint(Paint::Quad(Quad::new(bg)));

            // Style Hero Clock using its text handle:
            let hero_text_h = ctx.at(hero_clock).unwrap().me().text();
            if let Some(mut t) = ctx.at(hero_text_h) {
                t.set_variant(TextVariant::DisplayMedium);
                t.set_color(ColorRole::Primary);
                t.set_emphasised(true);
            }

            // Style 12-Hour Clock using its text handle:
            let c12_text_h = ctx.at(clock_12h).unwrap().me().text();
            if let Some(mut t) = ctx.at(c12_text_h) {
                t.set_variant(TextVariant::TitleLarge);
                t.set_color(ColorRole::OnSurface);
            }

            // Style 24-Hour Clock using its text handle:
            let c24_text_h = ctx.at(clock_24h).unwrap().me().text();
            if let Some(mut t) = ctx.at(c24_text_h) {
                t.set_variant(TextVariant::TitleLarge);
                t.set_color(ColorRole::OnSurfaceVariant);
            }

            // Style UTC Clock using its text handle:
            let utc_text_h = ctx.at(clock_utc).unwrap().me().text();
            if let Some(mut t) = ctx.at(utc_text_h) {
                t.set_variant(TextVariant::TitleLarge);
                t.set_color(ColorRole::Secondary);
            }

            // Style Calendar Clock using its text handle:
            let date_text_h = ctx.at(date_clock).unwrap().me().text();
            if let Some(mut t) = ctx.at(date_text_h) {
                t.set_variant(TextVariant::TitleMedium);
                t.set_color(ColorRole::OnSurface);
            }

            // Style Interactive Clock using its text handle:
            let inter_text_h = ctx.at(interactive_clock).unwrap().me().text();
            if let Some(mut t) = ctx.at(inter_text_h) {
                t.set_variant(TextVariant::HeadlineSmall);
                t.set_color(ColorRole::Secondary);
                t.set_weight(FontWeight::Bold);
            }
        });

        ClockShowcase {
            is_12h: true,
            show_seconds: true,
        }
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
                .title("Mechanix Design - Clock Widget")
                .layout(LayoutStyle::default().center().size(px(600.0), px(540.0))),
        );
        s.on::<CloseRequested>(win, |ctx, _| ctx.signal(Stop));
        s.spawn(win, ClockShowcaseBuilder);
        Shell
    }
}

fn main() {
    let mut app = App::new();
    app.add_module(LayoutModule)
        .add_module(PaintModule)
        .add_module(WindowModule)
        .add_module(InteractivityModule)
        .add_module(RenderModule::default())
        .insert_resource(Atlas::new());

    // Baseline dark theme
    app.add_module(MechanixTheme::dark());

    // Load font
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
            app_id: "mecha.clock".into(),
            budget: Budget::default(),
        });

    let root = app.root();
    app.spawn(root, ShellBuilder { root });
    app.run();
}
