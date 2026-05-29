use app;

#[test]
fn app_state_should_exist() {
    let x = 0u32;
    let app = app::App::new(x);
    assert_eq!(*app.state(), 0u32);
}

#[test]
fn module_should_exist() {
    app::Module::<(), _>::new();
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
