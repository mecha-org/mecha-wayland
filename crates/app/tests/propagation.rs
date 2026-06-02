use app;
use app::prelude::*;

// ── Cross-module propagation ──────────────────────────────────────────────────

#[test]
fn emitted_event_reaches_module_mounted_before_emitter() {
    // Module A mounted first; module B emits EventB. A must still receive it.
    #[derive(Debug)]
    struct EventA;
    impl app::Event for EventA {}
    #[derive(Debug)]
    struct EventB;
    impl app::Event for EventB {}

    let module_a = app::Module::new().on(|s: &mut (u32, u32), _: &EventB| s.0 += 1);
    let module_b = app::Module::new().on(|s: &mut (u32, u32), _: &EventA| {
        s.1 += 1;
        EventB
    });

    let mut app = app::App::new((0u32, 0u32)).mount(module_a).mount(module_b);
    app.dispatch(&EventA);

    assert_eq!(app.state().0, 1, "module_a should have received EventB emitted by module_b");
    assert_eq!(app.state().1, 1, "module_b should have handled EventA");
}

#[test]
fn emitted_event_reaches_module_mounted_after_emitter() {
    // Module A emits EventB; module B mounted after must still receive it.
    #[derive(Debug)]
    struct EventA;
    impl app::Event for EventA {}
    #[derive(Debug)]
    struct EventB;
    impl app::Event for EventB {}

    let module_a = app::Module::new().on(|s: &mut (u32, u32), _: &EventA| {
        s.0 += 1;
        EventB
    });
    let module_b = app::Module::new().on(|s: &mut (u32, u32), _: &EventB| s.1 += 1);

    let mut app = app::App::new((0u32, 0u32)).mount(module_a).mount(module_b);
    app.dispatch(&EventA);

    assert_eq!(app.state().0, 1, "module_a should have handled EventA");
    assert_eq!(app.state().1, 1, "module_b should have received EventB emitted by module_a");
}

// ── Submodule propagation ─────────────────────────────────────────────────────

#[test]
fn submodule_receives_dispatched_event() {
    #[derive(Debug)]
    struct Tick;
    impl app::Event for Tick {}

    #[derive(Debug)]
    struct ChildState(u32);

    struct State { child: ChildState }

    unsafe impl app::Lens<ChildState> for State {
        fn lens(&mut self) -> &mut ChildState { &mut self.child }
    }

    let child = app::Module::new().on(|s: &mut ChildState, _: &Tick| s.0 += 1);
    let parent = app::Module::<State, _, _>::new().mount(child);
    let mut app = app::App::new(State { child: ChildState(0) }).mount(parent);

    app.dispatch(&Tick);

    assert_eq!(app.state().child.0, 1);
}

#[test]
fn submodule_emitted_event_reaches_app_root() {
    #[derive(Debug)]
    struct Trigger;
    impl app::Event for Trigger {}
    #[derive(Debug)]
    struct Response;
    impl app::Event for Response {}

    #[derive(Debug)]
    struct ChildState(u32);

    struct State { child: ChildState, root: u32 }

    unsafe impl app::Lens<ChildState> for State {
        fn lens(&mut self) -> &mut ChildState { &mut self.child }
    }

    let child = app::Module::new().on(|_: &mut ChildState, _: &Trigger| Response);
    let root_module = app::Module::new().on(|s: &mut State, _: &Response| s.root += 1);
    let parent = app::Module::<State, _, _>::new().mount(child);

    let mut app = app::App::new(State { child: ChildState(0), root: 0 })
        .mount(parent)
        .mount(root_module);

    app.dispatch(&Trigger);

    assert_eq!(app.state().root, 1, "Response emitted by submodule should reach root module");
}

#[test]
fn app_module_emitted_event_reaches_submodule() {
    #[derive(Debug)]
    struct Trigger;
    impl app::Event for Trigger {}
    #[derive(Debug)]
    struct Response;
    impl app::Event for Response {}

    #[derive(Debug)]
    struct ChildState(u32);

    struct State { child: ChildState, root: u32 }

    unsafe impl app::Lens<ChildState> for State {
        fn lens(&mut self) -> &mut ChildState { &mut self.child }
    }

    let root_module = app::Module::new().on(|s: &mut State, _: &Trigger| {
        s.root += 1;
        Response
    });
    let child = app::Module::new().on(|s: &mut ChildState, _: &Response| s.0 += 1);
    let parent = app::Module::<State, _, _>::new().mount(child);

    let mut app = app::App::new(State { child: ChildState(0), root: 0 })
        .mount(root_module)
        .mount(parent);

    app.dispatch(&Trigger);

    assert_eq!(app.state().child.0, 1, "Response emitted by root module should reach submodule");
}

#[test]
fn grandchild_emitted_event_reaches_app_root() {
    #[derive(Debug)]
    struct Tick;
    impl app::Event for Tick {}
    #[derive(Debug)]
    struct Done;
    impl app::Event for Done {}

    struct GrandchildState { count: u32 }
    struct ChildState { grandchild: GrandchildState }
    struct AppState { child: ChildState, done_count: u32 }

    unsafe impl app::Lens<GrandchildState> for ChildState {
        fn lens(&mut self) -> &mut GrandchildState { &mut self.grandchild }
    }
    unsafe impl app::Lens<ChildState> for AppState {
        fn lens(&mut self) -> &mut ChildState { &mut self.child }
    }

    let grandchild = app::Module::new().on(|_: &mut GrandchildState, _: &Tick| Done);
    let child = app::Module::<ChildState, _, _>::new().mount(grandchild);
    let parent = app::Module::<AppState, _, _>::new().mount(child);
    let counter = app::Module::new().on(|s: &mut AppState, _: &Done| s.done_count += 1);

    let mut app = app::App::new(AppState {
        child: ChildState { grandchild: GrandchildState { count: 0 } },
        done_count: 0,
    })
    .mount(parent)
    .mount(counter);

    app.dispatch(&Tick);

    assert_eq!(app.state().done_count, 1, "Done emitted by grandchild should reach root counter");
}

// Stack overflow aborts the process via SIGABRT — not catchable by #[should_panic].
// Run manually with: cargo test infinite_emission -- --ignored
#[test]
#[ignore]
fn infinite_emission_causes_stack_overflow() {
    #[derive(Debug)]
    struct PingEvent;
    impl app::Event for PingEvent {}

    let module = app::Module::new().on(|_: &mut u32, _: &PingEvent| PingEvent);
    let mut app = app::App::new(0u32).mount(module);
    app.dispatch(&PingEvent);
}
