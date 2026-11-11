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

/// `zwp_text_input_v3`'s `enable`, `set_surrounding_text` and
/// `show_input_panel` requests, and its `enter` event.
const ENABLE: u16 = 1;
const SET_SURROUNDING_TEXT: u16 = 3;
const SHOW_INPUT_PANEL: u16 = 9;
const ENTER: u16 = 0;

/// The text input's id, from the `get_text_input` that made it, and the
/// compositor's text-input focus on the window: `enable` waits for it.
fn enter(ui: &mut Ui) -> u32 {
    ui.f.turn();
    let made =
        ui.f.requests()
            .into_iter()
            .find(|r| r.opcode == 1 && r.body.len() == 8)
            .expect("get_text_input");
    let id = made.reader().object().expect("the new id").0;
    ui.f.send(id, ENTER, |w| w.object(ObjectId(100)));
    ui.f.turn();
    id
}

/// How many of `opcode` the text input sent since last asked.
fn sent(ui: &mut Ui, id: u32, opcode: u16) -> usize {
    ui.f.turn();
    ui.f.requests()
        .iter()
        .filter(|r| r.sender == ObjectId(id) && r.opcode == opcode)
        .count()
}

fn enables(ui: &mut Ui, id: u32) -> usize {
    sent(ui, id, ENABLE)
}

fn type_a(ui: &mut Ui) {
    let key = KeyPress {
        meaning: KeyMeaning::Text("a".into()),
        modifiers: Default::default(),
    };
    ui.f.app.emit(key, ui.field);
    ui.f.app.tick();
}

#[test]
fn a_password_field_never_sends_its_text() {
    for (purpose, sends) in [
        (ContentPurpose::Normal, true),
        (ContentPurpose::Password, false),
    ] {
        let mut ui = ui_for(purpose);
        let id = enter(&mut ui);
        press(&mut ui, 50.0, 20.0);
        type_a(&mut ui);
        assert_eq!(
            sent(&mut ui, id, SET_SURROUNDING_TEXT) > 0,
            sends,
            "{purpose:?}"
        );
    }
}

#[test]
fn a_press_on_the_focused_field_enables_it_again_and_a_key_does_not() {
    let mut ui = ui();
    let id = enter(&mut ui);
    press(&mut ui, 50.0, 20.0);
    assert_eq!(enables(&mut ui, id), 1);

    // The input method shows itself again, after the user hid it.
    press(&mut ui, 50.0, 20.0);
    assert_eq!(enables(&mut ui, id), 1);

    // Typing on a hardware keyboard leaves a hidden one hidden.
    type_a(&mut ui);
    assert_eq!(enables(&mut ui, id), 0);
    assert_eq!(ui.f.app.widget::<Input>(ui.field).unwrap().text(), "a");
}

#[test]
fn from_version_2_a_press_on_the_focused_field_shows_the_panel_and_resets_nothing() {
    let mut ui = ui_with(ContentPurpose::Normal, 2);
    let id = enter(&mut ui);
    press(&mut ui, 50.0, 20.0);
    assert_eq!(enables(&mut ui, id), 1);

    press(&mut ui, 50.0, 20.0);
    ui.f.turn();
    let sent: Vec<u16> =
        ui.f.requests()
            .iter()
            .filter(|r| r.sender == ObjectId(id))
            .map(|r| r.opcode)
            .collect();
    assert_eq!(
        sent.iter().filter(|&&op| op == SHOW_INPUT_PANEL).count(),
        1,
        "{sent:?}"
    );
    assert!(!sent.contains(&ENABLE), "no reset: {sent:?}");
}

#[test]
fn a_press_on_nothing_focusable_keeps_the_field() {
    let mut ui = ui();
    press(&mut ui, 50.0, 20.0);
    press(&mut ui, 150.0, 150.0);
    assert_eq!(text_focus(&ui), Some(ui.field));
}
