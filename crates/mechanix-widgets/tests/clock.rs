//! Integration tests for Mechanix Design Clock widget.

use app::prelude::*;
use atlas::Atlas;
use chrono::Local;
use layout::prelude::*;
use mechanix_widgets::prelude::*;
use paint::prelude::*;
use theme::prelude::*;

fn setup_test_app() -> (App, atlas::FontId) {
    let mut app = App::new();
    app.add_module(MechanixTheme::dark());
    app.add_module(LayoutModule);
    app.add_module(PaintModule);
    let mut atlas = Atlas::new();
    let font_id = atlas
        .add_font(include_bytes!(
            "../../../crates/atlas/tests/fixtures/Inter-Regular.ttf"
        ))
        .expect("Inter font loads");
    app.insert_resource(atlas);
    app.insert_resource(FontBook::new(font_id));
    (app, font_id)
}

#[test]
fn test_1_clock_construction() {
    let b = clock("%H:%M:%S");
    assert_eq!(b.format, "%H:%M:%S");

    let b2 = Clock::new("%H:%M");
    assert_eq!(b2.format, "%H:%M");

    let b3 = clock("%H:%M").format("%I:%M %p");
    assert_eq!(b3.format, "%I:%M %p");
}

#[test]
fn test_2_spawns_and_renders_text_handle() {
    let (mut app, _) = setup_test_app();

    let clock_h = app.spawn(app.root(), clock("%Y"));
    app.tick();

    let clock_w = app.widget::<Clock>(clock_h).unwrap();
    assert_eq!(clock_w.format(), "%Y");

    // Access the text handle directly to verify child text
    let text_h = clock_w.text();
    let text_w = app.widget::<Text>(text_h).unwrap();
    let native_h = text_w.native();
    let native_text = app.widget::<widgets::Text>(native_h).unwrap();

    let expected_year = Local::now().format("%Y").to_string();
    assert_eq!(native_text.string, expected_year);
}

#[test]
fn test_3_multiple_clock_instances() {
    let (mut app, _) = setup_test_app();

    let clock1 = app.spawn(app.root(), clock("%Y"));
    let clock2 = app.spawn(app.root(), clock("%m"));
    app.tick();

    let text1 = app.widget::<Clock>(clock1).unwrap().text();
    let text2 = app.widget::<Clock>(clock2).unwrap().text();

    let nat1 = app.widget::<Text>(text1).unwrap().native();
    let nat2 = app.widget::<Text>(text2).unwrap().native();

    assert_eq!(
        app.widget::<widgets::Text>(nat1).unwrap().string,
        Local::now().format("%Y").to_string()
    );
    assert_eq!(
        app.widget::<widgets::Text>(nat2).unwrap().string,
        Local::now().format("%m").to_string()
    );
}

#[test]
fn test_4_widget_lifecycle_and_cleanup() {
    let (mut app, _) = setup_test_app();

    let clock_h = app.spawn(app.root(), clock("%H:%M"));
    app.tick();

    assert_eq!(app.widgets::<Clock>().count(), 1);
    assert!(app.widget::<Clock>(clock_h).is_some());

    // Remove the clock
    assert!(app.remove(clock_h));
    app.tick();

    // The widget is cleaned up
    assert_eq!(app.widgets::<Clock>().count(), 0);
    assert!(app.widget::<Clock>(clock_h).is_none());
}

#[test]
fn test_5_clock_tick_event_update() {
    let (mut app, _) = setup_test_app();

    let clock_h = app.spawn(app.root(), clock("%H:%M:%S"));
    app.tick();

    let text_h = app.widget::<Clock>(clock_h).unwrap().text();
    let native_h = app.widget::<Text>(text_h).unwrap().native();
    assert!(!app.widget::<widgets::Text>(native_h).unwrap().string.is_empty());

    // Emit ClockTick event to the widget
    app.emit(ClockTick, clock_h);
    app.flush();
    app.tick();

    assert!(!app.widget::<widgets::Text>(native_h).unwrap().string.is_empty());
}

#[test]
fn test_6_clock_context_ext_set_format() {
    let (mut app, _) = setup_test_app();

    let clock_h = app.spawn(app.root(), clock("%H:%M:%S"));
    app.tick();

    struct UpdateFormat;
    impl app::Event for UpdateFormat {}

    struct Controller;
    struct ControllerBuilder(Handle<Clock>);
    impl Build for ControllerBuilder {
        type Widget = Controller;
    }
    impl Widget for Controller {
        type Builder = ControllerBuilder;
        fn build(b: ControllerBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
            let clock_h = b.0;
            s.on::<UpdateFormat>(me, move |ctx, _| {
                if let Some(mut clock_ctx) = ctx.at(clock_h) {
                    clock_ctx.set_format("%Y");
                }
            });
            Controller
        }
    }

    let controller = app.spawn(app.root(), ControllerBuilder(clock_h));
    app.emit(UpdateFormat, controller.id());
    app.flush();
    app.tick();

    let clock_w = app.widget::<Clock>(clock_h).unwrap();
    assert_eq!(clock_w.format(), "%Y");

    let text_h = clock_w.text();
    let native_h = app.widget::<Text>(text_h).unwrap().native();
    assert_eq!(
        app.widget::<widgets::Text>(native_h).unwrap().string,
        Local::now().format("%Y").to_string()
    );
}
