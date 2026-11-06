//! Demonstrates `Clock` in `mechanix-widgets`:
//! - A time clock ticking every second (`ClockInterval::Second`)
//! - A date clock ticking on day change (`ClockInterval::Day`)
//! - Toggling 12h / 24h and seconds at runtime via `ClockContextExt`
//!
//! Run under a Wayland session: `cargo run --example clock`.

use gles::Budget;
use mecha_wayland::mechanix_widgets::prelude::{
    ButtonVariant, Clock, ClockContextExt, FontBook, button, clock,
};
use mecha_wayland::prelude::*;
use mechanix_widgets::ClockInterval;
use theme::{ColorRole, TextVariant};

struct ClockShowcase {
    is_12h: bool,
    show_seconds: bool,
}

impl ClockShowcase {
    fn format(&self) -> &'static str {
        match (self.is_12h, self.show_seconds) {
            (true, true) => "%I:%M:%S %p",
            (true, false) => "%I:%M %p",
            (false, true) => "%H:%M:%S",
            (false, false) => "%H:%M",
        }
    }

    /// Without seconds on screen, a tick per minute is enough.
    fn interval(&self) -> ClockInterval {
        if self.show_seconds {
            ClockInterval::Second
        } else {
            ClockInterval::Minute
        }
    }
}

fn apply_time_format(ctx: &mut Context<'_, ClockShowcase>, time_clock: Handle<Clock>) {
    let (format, interval) = (ctx.me().format(), ctx.me().interval());
    if let Some(mut clock_ctx) = ctx.at(time_clock) {
        clock_ctx.set_format(format);
        clock_ctx.set_interval(interval);
    }
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
            .gap(px(12.0));
        *s.component_mut::<Paint>(me).unwrap() =
            Paint::Quad(Quad::new(s.color(ColorRole::Surface)));

        let state = ClockShowcase {
            is_12h: false,
            show_seconds: true,
        };

        let time_clock = s.spawn(
            me,
            clock(state.format())
                .interval(state.interval())
                .variant(TextVariant::DisplayMedium)
                .color(ColorRole::Primary),
        );

        // Date only: re-renders once, at local midnight.
        s.spawn(
            me,
            clock("%A, %d %B %Y")
                .interval(ClockInterval::Day)
                .variant(TextVariant::TitleMedium)
                .color(ColorRole::OnSurfaceVariant),
        );

        let btn_row = s.spawn(
            me,
            div().style(LayoutStyle::default().row().center().gap(px(8.0))),
        );
        let toggle_hour_btn = s.spawn(
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

        s.on::<Clicked>(toggle_hour_btn, move |ctx, _| {
            ctx.me().is_12h = !ctx.me().is_12h;
            apply_time_format(ctx, time_clock);
        });

        s.on::<Clicked>(toggle_sec_btn, move |ctx, _| {
            ctx.me().show_seconds = !ctx.me().show_seconds;
            apply_time_format(ctx, time_clock);
        });

        state
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
                .layout(LayoutStyle::default().center().size(px(420.0), px(260.0))),
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
        .add_module(TimerModule)
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
