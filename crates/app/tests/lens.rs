use app;
use app::prelude::*;

#[test]
fn identity_blanket_impl() {
    // When module state equals app state, no manual Lens impl is needed.
    #[derive(Debug)]
    struct Tick;
    impl app::Event for Tick {}

    let module = app::Module::new().on(|s: &mut u32, _: &Tick| *s += 1);
    let mut app = app::App::new(0u32).mount(module);
    app.dispatch(&Tick);

    assert_eq!(*app.state(), 1);
}

#[test]
fn field_extraction() {
    // Lens<T> returns only the declared field; other fields are untouched.
    #[derive(Debug)]
    struct Tick;
    impl app::Event for Tick {}

    struct State { a: u32, b: u32 }

    unsafe impl app::Lens<u32> for State {
        fn lens(&mut self) -> &mut u32 { &mut self.a }
    }

    let module = app::Module::new().on(|s: &mut u32, _: &Tick| *s += 1);
    let mut app = app::App::new(State { a: 0, b: 10 }).mount(module);
    app.dispatch(&Tick);

    assert_eq!(app.state().a, 1);
    assert_eq!(app.state().b, 10);
}

#[test]
fn two_modules_on_different_fields() {
    // Two modules owning distinct newtypes; each only affects its own field.
    #[derive(Debug)]
    struct TickA;
    impl app::Event for TickA {}
    #[derive(Debug)]
    struct TickB;
    impl app::Event for TickB {}

    struct Counter(u32);
    struct Timer(u32);
    struct State { counter: Counter, timer: Timer }

    unsafe impl app::Lens<Counter> for State {
        fn lens(&mut self) -> &mut Counter { &mut self.counter }
    }
    unsafe impl app::Lens<Timer> for State {
        fn lens(&mut self) -> &mut Timer { &mut self.timer }
    }

    let counter_module = app::Module::new().on(|s: &mut Counter, _: &TickA| s.0 += 1);
    let timer_module = app::Module::new().on(|s: &mut Timer, _: &TickB| s.0 += 10);

    let mut app = app::App::new(State { counter: Counter(0), timer: Timer(0) })
        .mount(counter_module)
        .mount(timer_module);

    app.dispatch(&TickA);
    app.dispatch(&TickB);

    assert_eq!(app.state().counter.0, 1);
    assert_eq!(app.state().timer.0, 10);
}
