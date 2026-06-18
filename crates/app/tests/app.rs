use app;
use app::prelude::*;

#[test]
fn app_state_should_exist() {
    let x = 0u32;
    let app = app::App::new(x);
    assert_eq!(*app.state(), 0u32);
}

#[test]
fn module_should_exist() {
    app::Module::<(), _, _>::new();
}

#[test]
fn attached_handler_to_module() {
    #[derive(Debug)]
    struct TestEvent;
    impl app::Event for TestEvent {}

    app::Module::new().on(|_: &mut u32, _: &TestEvent| {});
}

#[test]
fn mount() {
    #[derive(Debug)]
    struct TestEvent;
    impl app::Event for TestEvent {}

    let app = app::App::new(0u32);
    let module = app::Module::new().on(|_: &mut u32, _: &TestEvent| {});

    app.mount(module);
}
