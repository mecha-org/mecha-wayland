//! The text focus follows the keyboard focus: a press on another
//! `Focusable` node blurs the field, a press on nothing focusable doesn't.

use app::prelude::*;
use atlas::Atlas;
use geometry::{Insets, Point};
use interactivity::prelude::*;
use layout::prelude::*;
use paint::prelude::*;
use text_input::prelude::*;
use wayland::fake::Fake;
use wayland::prelude::*;
use widgets::prelude::*;
use window::prelude::*;

struct Ui {
    f: Fake,
    win: NodeId,
    field: NodeId,
}

fn at(x: f32, y: f32, w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::default()
        .absolute()
        .inset(Insets::new(px(y), auto(), auto(), px(x)))
        .size(px(w), px(h))
}

/// A 200x200 window: a field at the top, a focusable box (a list) under
/// it, and a plain box (a key) beside that.
fn ui() -> Ui {
    ui_for(ContentPurpose::Normal)
}

fn ui_for(purpose: ContentPurpose) -> Ui {
    ui_with(purpose, 1)
}

/// With the compositor offering text-input at `version`.
fn ui_with(purpose: ContentPurpose, version: u32) -> Ui {
    let mut f = Fake::new(
        &[("wl_seat", 7), ("zwp_text_input_manager_v3", version)],
        |m| m.bind::<WlSeat>(),
    );
    let mut atlas = Atlas::new();
    let font = atlas
        .add_font(include_bytes!(
            "../../atlas/tests/fixtures/Inter-Regular.ttf"
        ))
        .unwrap();
    f.app.insert_resource(atlas);
    f.app
        .add_module(LayoutModule)
        .add_module(PaintModule)
        .add_module(WindowModule)
        .add_module(InteractivityModule)
        .add_module(TextInputModule);
    let root = f.app.root();
    let win = f
        .app
        .spawn(
            root,
            window().layout(LayoutStyle::default().size(px(200.0), px(200.0))),
        )
        .id();
    let inner = LayoutStyle::default().size(percent(100.0), percent(100.0));
    let field = f
        .app
        .spawn(win, input(font).style(inner).content_purpose(purpose))
        .id();
    *f.app.component_mut::<LayoutStyle>(field).unwrap() = at(0.0, 0.0, 200.0, 40.0);
    let list = f
        .app
        .spawn(win, div().style(at(0.0, 100.0, 100.0, 100.0)))
        .id();
    *f.app.component_mut::<Focusable>(list).unwrap() = Focusable(true);
    f.app
        .spawn(win, div().style(at(100.0, 100.0, 100.0, 100.0)));
    f.app.tick();
    Ui { f, win, field }
}

fn press(ui: &mut Ui, x: f32, y: f32) {
    for phase in [
        ContactPhase::Moved,
        ContactPhase::Pressed,
        ContactPhase::Released,
    ] {
        ui.f.app.signal(ContactInput {
            window: ui.win,
            contact: ContactId::Touch(0),
            phase,
            position: Point::new(x, y),
            time: 0,
        });
        ui.f.app.flush();
    }
    ui.f.app.tick();
    ui.f.app.flush();
}

fn text_focus(ui: &Ui) -> Option<NodeId> {
    ui.f.app.resource::<TextInput>().focused()
}

#[test]
fn a_press_on_another_focusable_blurs_the_field() {
    let mut ui = ui();
    press(&mut ui, 50.0, 20.0);
    assert_eq!(text_focus(&ui), Some(ui.field));

    press(&mut ui, 50.0, 150.0);
    assert_eq!(text_focus(&ui), None);
}

#[test]
fn a_press_on_nothing_focusable_keeps_the_field() {
    let mut ui = ui();
    press(&mut ui, 50.0, 20.0);
    press(&mut ui, 150.0, 150.0);
    assert_eq!(text_focus(&ui), Some(ui.field));
}
