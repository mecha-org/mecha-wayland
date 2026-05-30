use app;

#[test]
fn app_state_should_exist() {
    let x = 0u32;
    let app = app::App::new(x);
    assert_eq!(*app.state(), 0u32);
}

#[test]
fn module_should_exist() {
    app::Module::<(), _, _, _>::new();
}

#[test]
fn attached_handler_to_module() {
    struct TestEvent;
    impl app::Event for TestEvent {}

    app::Module::new().on(|_: &mut u32, _: &TestEvent| {});
}

#[test]
fn register_module() {
    struct TestEvent;
    impl app::Event for TestEvent {}

    let app = app::App::new(0);
    let module = app::Module::new().on(|_: &mut u32, _: &TestEvent| {});

    app.register_module(|s: &mut u32| s, module);
}

#[test]
fn dispatch_event() {
    struct TestEvent;
    impl app::Event for TestEvent {}

    let module = app::Module::new().on(|s: &mut u32, _: &TestEvent| *s += 1);

    let mut app = app::App::new(0).register_module(|s: &mut u32| s, module);
    app.dispatch(&TestEvent);

    assert_eq!(*app.state(), 1);
}

#[test]
fn dispatch_other_event() {
    struct TestEvent;
    impl app::Event for TestEvent {}

    struct OtherEvent;
    impl app::Event for OtherEvent {}

    let module = app::Module::new().on(|s: &mut u32, _: &TestEvent| *s += 1);

    let mut app = app::App::new(0).register_module(|s: &mut u32| s, module);
    app.dispatch(&OtherEvent);

    assert_eq!(*app.state(), 0);
}

#[test]
fn dispatch_emiting_event() {
    struct TestEvent;
    impl app::Event for TestEvent {}

    let module = app::Module::new().emit(|s: &mut u32, _: &TestEvent| *s += 1);

    let mut app = app::App::new(0).register_module(|s: &mut u32| s, module);
    app.dispatch(&TestEvent);

    assert_eq!(*app.state(), 1);
}

#[test]
fn emitted_event_propagates_3_levels_deep() {
    struct EventA;
    impl app::Event for EventA {}
    struct EventB;
    impl app::Event for EventB {}
    struct EventC;
    impl app::Event for EventC {}
    struct EventD;
    impl app::Event for EventD {}

    // EventA → emits EventB → emits EventC → emits EventD → increments state
    let module = app::Module::new()
        .emit(|_: &mut u32, _: &EventA| EventB)
        .emit(|_: &mut u32, _: &EventB| EventC)
        .emit(|_: &mut u32, _: &EventC| EventD)
        .on(|s: &mut u32, _: &EventD| *s += 1);

    let mut app = app::App::new(0u32).register_module(|s: &mut u32| s, module);
    app.dispatch(&EventA);

    assert_eq!(*app.state(), 1);
}

// Stack overflow aborts the process via SIGABRT — not catchable by #[should_panic].
// Run manually with: cargo test infinite_emission -- --ignored
#[test]
fn infinite_emission_causes_stack_overflow() {
    struct PingEvent;
    impl app::Event for PingEvent {}

    // PingEvent always emits itself — infinite recursion
    let module = app::Module::new().emit(|_: &mut u32, _: &PingEvent| PingEvent);

    let mut app = app::App::new(0u32).register_module(|s: &mut u32| s, module);
    app.dispatch(&PingEvent);
}
