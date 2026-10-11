use std::time::{Duration, Instant};

use chrono::Local;

use crate::text::{Text, TextContextExt, text};
use app::{Build, Context, Handle, Spawner, Widget};
use layout::LayoutStyle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ClockTick;

impl app::Event for ClockTick {}

/// The update frequency / precision of a [`Clock`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ClockInterval {
    /// Update every second.
    #[default]
    Second,
    /// Update every minute.
    Minute,
}

impl ClockInterval {
    /// Detects whether the format requires second-level or minute-level updates.
    pub fn from_format(format: &str) -> Self {
        if format.contains("%S")
            || format.contains("%T")
            || format.contains("%X")
            || format.contains("%s")
            || format.contains("%f")
            || format.contains("%c")
            || format.contains("%+")
            || format.contains("%r")
        {
            ClockInterval::Second
        } else {
            ClockInterval::Minute
        }
    }

    /// Resolves [`ClockInterval::Auto`] to a concrete interval using `format`.
    pub fn resolve(&self, format: &str) -> Self {
        match self {
            Self::Second => Self::from_format(format),
            other => *other,
        }
    }

    /// Monotonic step duration between updates.
    pub fn duration(&self, format: &str) -> Duration {
        match self.resolve(format) {
            Self::Second => Duration::from_secs(1),
            Self::Minute => Duration::from_secs(60),
        }
    }

    /// Monotonic deadline [`Instant`] for the next tick based on an anchor [`Instant`].
    #[inline]
    pub fn next_deadline_from(&self, format: &str, last_tick: Instant) -> Instant {
        last_tick + self.duration(format)
    }

    /// Monotonic deadline [`Instant`] for the next tick from now.
    #[inline]
    pub fn next_deadline(&self, format: &str) -> Instant {
        Instant::now() + self.duration(format)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Clock {
    pub format: String,
    pub interval: ClockInterval,
    pub last_tick: Instant,
    pub(crate) text_handle: Handle<Text>,
}

impl Clock {
    #[inline]
    pub fn new(format: impl Into<String>) -> ClockBuilder {
        ClockBuilder::new(format)
    }

    #[inline]
    pub fn text(&self) -> Handle<Text> {
        self.text_handle
    }

    #[inline]
    pub fn format(&self) -> &str {
        &self.format
    }

    #[inline]
    pub fn interval(&self) -> ClockInterval {
        self.interval
    }

    #[inline]
    pub fn effective_interval(&self) -> ClockInterval {
        self.interval.resolve(&self.format)
    }

    /// Monotonic interval duration between ticks.
    #[inline]
    pub fn duration(&self) -> Duration {
        self.interval.duration(&self.format)
    }

    /// Monotonic deadline [`Instant`] for the next scheduled tick, anchored to `self.last_tick + self.duration()`.
    #[inline]
    pub fn next_deadline(&self) -> Instant {
        self.last_tick + self.duration()
    }

    /// Checks whether the scheduled deadline for the next tick has arrived.
    #[inline]
    pub fn is_due(&self) -> bool {
        Instant::now() + Duration::from_millis(20) >= self.next_deadline()
    }

    /// Monotonic duration remaining until the next scheduled tick deadline.
    #[inline]
    pub fn remaining_until_deadline(&self) -> Duration {
        self.next_deadline()
            .saturating_duration_since(Instant::now())
    }

    /// Monotonic instant of the last tick update.
    #[inline]
    pub fn last_tick(&self) -> Instant {
        self.last_tick
    }

    /// Monotonic duration elapsed since the last tick update.
    #[inline]
    pub fn elapsed(&self) -> Duration {
        self.last_tick.elapsed()
    }
}

#[inline]
pub fn clock(format: impl Into<String>) -> ClockBuilder {
    ClockBuilder::new(format)
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClockBuilder {
    pub format: String,
    pub interval: ClockInterval,
}

impl ClockBuilder {
    pub fn new(format: impl Into<String>) -> Self {
        Self {
            format: format.into(),
            interval: ClockInterval::Second,
        }
    }

    pub fn format(mut self, format: impl Into<String>) -> Self {
        self.format = format.into();
        self
    }

    pub fn interval(mut self, interval: ClockInterval) -> Self {
        self.interval = interval;
        self
    }

    pub fn every_second(self) -> Self {
        self.interval(ClockInterval::Second)
    }

    pub fn every_minute(self) -> Self {
        self.interval(ClockInterval::Minute)
    }
}

impl Default for ClockBuilder {
    fn default() -> Self {
        Self::new("%H:%M:%S")
    }
}

impl Build for ClockBuilder {
    type Widget = Clock;
}

impl Widget for Clock {
    type Builder = ClockBuilder;

    fn build(b: ClockBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        *s.component_mut::<LayoutStyle>(me).unwrap() = LayoutStyle::default();

        let initial_formatted = Local::now().format(&b.format).to_string();
        let text_handle = s.spawn(me, text(initial_formatted));

        s.on::<ClockTick>(me, |ctx, _| {
            ctx.tick();
        });

        Clock {
            format: b.format,
            interval: b.interval,
            last_tick: Instant::now(),
            text_handle,
        }
    }
}

pub trait ClockContextExt {
    /// Ticks the clock if the scheduled deadline based on `self.duration()` has arrived.
    /// Returns `true` if the clock was updated, or `false` if it was not due yet.
    fn tick(&mut self) -> bool;

    /// Re-evaluates current time, sets `last_tick` to now, and updates the text widget unconditionally.
    fn update_time(&mut self);

    /// Directly updates the displayed string on the text widget.
    fn set_text(&mut self, text: impl Into<String>);

    /// Updates the format string and re-evaluates the time.
    fn set_format(&mut self, format: impl Into<String>);

    /// Updates the interval.
    fn set_interval(&mut self, interval: ClockInterval);
}

impl ClockContextExt for Context<'_, Clock> {
    fn tick(&mut self) -> bool {
        if self.me().is_due() {
            self.update_time();
            true
        } else {
            false
        }
    }

    fn update_time(&mut self) {
        let (format, text_handle) = {
            let me = self.me();
            (me.format.clone(), me.text_handle)
        };
        self.me().last_tick = Instant::now();
        let formatted = Local::now().format(&format).to_string();
        if let Some(mut text_ctx) = self.at(text_handle) {
            text_ctx.set_text(formatted);
        }
    }

    fn set_text(&mut self, text: impl Into<String>) {
        let text_handle = self.me().text_handle;
        self.me().last_tick = Instant::now();
        if let Some(mut text_ctx) = self.at(text_handle) {
            text_ctx.set_text(text);
        }
    }

    fn set_format(&mut self, format: impl Into<String>) {
        self.me().format = format.into();
        self.update_time();
    }

    fn set_interval(&mut self, interval: ClockInterval) {
        self.me().interval = interval;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::FontBook;
    use app::App;
    use atlas::Atlas;
    use theme::ColorRole;
    use theme::prelude::*;

    fn setup_app() -> (App, atlas::FontId) {
        let mut app = App::new();
        app.add_module(MechanixTheme::dark());
        app.add_module(layout::LayoutModule);
        app.add_module(paint::PaintModule);
        let mut atlas = Atlas::new();
        let font_id = atlas
            .add_font(include_bytes!(
                "../../../crates/atlas/tests/fixtures/Inter-Regular.ttf"
            ))
            .unwrap();
        app.insert_resource(atlas);
        app.insert_resource(FontBook::new(font_id));
        (app, font_id)
    }

    #[test]
    fn test_clock_builder_format() {
        let b = clock("%H:%M");
        assert_eq!(b.format, "%H:%M");
        assert_eq!(b.interval, ClockInterval::Second);

        let b2 = Clock::new("%H:%M:%S");
        assert_eq!(b2.format, "%H:%M:%S");

        let b3 = clock("%H:%M").format("%I:%M %p");
        assert_eq!(b3.format, "%I:%M %p");
    }

    #[test]
    fn test_clock_interval_auto_detection() {
        assert_eq!(
            ClockInterval::from_format("%H:%M:%S"),
            ClockInterval::Second
        );
        assert_eq!(
            ClockInterval::from_format("%I:%M %p"),
            ClockInterval::Minute
        );
        assert_eq!(ClockInterval::from_format("%H:%M"), ClockInterval::Minute);
        assert_eq!(ClockInterval::from_format("%Y"), ClockInterval::Minute);
    }

    #[test]
    fn test_clock_interval_explicit_configuration() {
        let c1 = clock("%H:%M").every_second();
        assert_eq!(c1.interval, ClockInterval::Second);

        let c2 = clock("%H:%M:%S").every_minute();
        assert_eq!(c2.interval, ClockInterval::Minute);
    }

    #[test]
    fn test_clock_monotonic_deadlines() {
        let c_sec = clock("%H:%M:%S");
        assert_eq!(
            c_sec.interval.duration(&c_sec.format),
            Duration::from_secs(1)
        );
        let now = Instant::now();
        assert_eq!(
            c_sec.interval.next_deadline_from(&c_sec.format, now),
            now + Duration::from_secs(1)
        );

        let c_min = clock("%H:%M");
        assert_eq!(
            c_min.interval.duration(&c_min.format),
            Duration::from_secs(60)
        );
        assert_eq!(
            c_min.interval.next_deadline_from(&c_min.format, now),
            now + Duration::from_secs(60)
        );
    }

    #[test]
    fn test_clock_spawns_and_renders_text() {
        let (mut app, _) = setup_app();
        let clock_h = app.spawn(app.root(), clock("%Y"));
        app.tick();

        let clock_w = app.widget::<Clock>(clock_h).unwrap();
        assert_eq!(clock_w.format(), "%Y");
        assert_eq!(clock_w.effective_interval(), ClockInterval::Minute);
        let d1 = clock_w.next_deadline();
        let d2 = clock_w.next_deadline();
        // Deadline is stable and anchored to self.last_tick + duration, not recomputed against now()
        assert_eq!(d1, d2);
        assert!(clock_w.next_deadline() > Instant::now());
        assert!(!clock_w.is_due());
        assert!(clock_w.elapsed() < Duration::from_secs(1));

        let text_h = clock_w.text();
        let text_w = app.widget::<Text>(text_h).unwrap();
        let native_h = text_w.native();
        let native_text = app.widget::<widgets::Text>(native_h).unwrap();

        let expected_year = Local::now().format("%Y").to_string();
        assert_eq!(native_text.string, expected_year);
    }

    #[test]
    fn test_clock_tick_event_update() {
        let (mut app, _) = setup_app();
        let clock_h = app.spawn(app.root(), clock("%H:%M:%S"));
        app.tick();

        let before = app.widget::<Clock>(clock_h).unwrap().last_tick();
        // 1. Tick before deadline does NOT advance last_tick
        app.emit(ClockTick, clock_h);
        app.flush();
        app.tick();
        assert_eq!(app.widget::<Clock>(clock_h).unwrap().last_tick(), before);

        // 2. Setting last_tick into the past so that the deadline has passed
        let past = Instant::now() - Duration::from_secs(2);
        app.widget_mut::<Clock>(clock_h).unwrap().last_tick = past;
        assert!(app.widget::<Clock>(clock_h).unwrap().is_due());

        app.emit(ClockTick, clock_h);
        app.flush();
        app.tick();

        let clock_w = app.widget::<Clock>(clock_h).unwrap();
        assert_eq!(clock_w.format(), "%H:%M:%S");
        assert!(clock_w.last_tick() > past);
    }

    #[test]
    fn test_clock_context_ext_and_set_text() {
        let (mut app, _) = setup_app();
        let clock_h = app.spawn(app.root(), clock("%H:%M"));
        app.tick();

        struct Poke;
        impl app::Event for Poke {}

        struct Controller;
        struct ControllerBuilder(Box<dyn FnMut(&mut Context<'_, Controller>)>);
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

        let controller = app.spawn(
            app.root(),
            ControllerBuilder(Box::new(move |ctx: &mut Context<'_, Controller>| {
                let mut clock_ctx = ctx.at(clock_h).unwrap();
                clock_ctx.set_text("12:34 PM");
            })),
        );

        app.emit(Poke, controller.id());
        app.flush();
        app.tick();

        let text_h = app.widget::<Clock>(clock_h).unwrap().text();
        let native_h = app.widget::<Text>(text_h).unwrap().native();
        let native_w = app.widget::<widgets::Text>(native_h).unwrap();
        assert_eq!(native_w.string, "12:34 PM");
    }

    #[test]
    fn test_clock_context_ext_and_text_styling_via_handle() {
        let (mut app, _) = setup_app();
        let clock_h = app.spawn(app.root(), clock("%H:%M"));
        app.tick();

        struct Poke;
        impl app::Event for Poke {}

        struct Controller;
        struct ControllerBuilder(Box<dyn FnMut(&mut Context<'_, Controller>)>);
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

        let controller = app.spawn(
            app.root(),
            ControllerBuilder(Box::new(move |ctx: &mut Context<'_, Controller>| {
                let mut clock_ctx = ctx.at(clock_h).unwrap();
                clock_ctx.set_format("%Y");
                clock_ctx.set_interval(ClockInterval::Minute);
                let text_h = clock_ctx.me().text();

                let mut text_ctx = ctx.at(text_h).unwrap();
                text_ctx.set_size(36.0);
                text_ctx.set_weight(FontWeight::Bold);
                text_ctx.set_color(ColorRole::Secondary);
            })),
        );

        app.emit(Poke, controller.id());
        app.flush();
        app.tick();

        let clock_w = app.widget::<Clock>(clock_h).unwrap();
        assert_eq!(clock_w.format(), "%Y");
        assert_eq!(clock_w.interval(), ClockInterval::Minute);

        let text_h = clock_w.text();
        let native_h = app.widget::<Text>(text_h).unwrap().native();
        let native_w = app.widget::<widgets::Text>(native_h).unwrap();

        let expected_year = Local::now().format("%Y").to_string();
        assert_eq!(native_w.string, expected_year);
        assert_eq!(native_w.px, 36);
        assert_eq!(native_w.color, app.color(ColorRole::Secondary));
    }
}
