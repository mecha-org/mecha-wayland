use app::prelude::*;

#[derive(Debug)]
struct Tick;
impl Event for Tick {}

#[derive(State)]
struct AppState {
    counter: u32,
    flag: bool,
}

#[test]
fn derive_state_generates_lens_for_each_field() {
    let counter_module = Module::new().on(|s: &mut u32, _: &Tick| *s += 1);
    let flag_module = Module::new().on(|s: &mut bool, _: &Tick| *s = true);

    let mut app = App::new(AppState { counter: 0, flag: false })
        .mount(counter_module)
        .mount(flag_module);

    app.dispatch(&Tick);

    assert_eq!(app.state().counter, 1);
    assert!(app.state().flag);
}
