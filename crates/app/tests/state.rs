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
    struct TestEvent;
    impl app::Event for TestEvent {}

    app::Module::new().on(|_: &mut u32, _: &TestEvent| {});
}

#[test]
fn mount() {
    struct TestEvent;
    impl app::Event for TestEvent {}

    let app = app::App::new(0);
    let module = app::Module::new().on(|_: &mut u32, _: &TestEvent| {});

    app.mount(|s: &mut u32| s, module);
}

#[test]
fn dispatch_event() {
    struct TestEvent;
    impl app::Event for TestEvent {}

    let module = app::Module::new().on(|s: &mut u32, _: &TestEvent| *s += 1);

    let mut app = app::App::new(0).mount(|s: &mut u32| s, module);
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

    let mut app = app::App::new(0).mount(|s: &mut u32| s, module);
    app.dispatch(&OtherEvent);

    assert_eq!(*app.state(), 0);
}

#[test]
fn handler_can_emit_event() {
    struct TestEvent;
    impl app::Event for TestEvent {}

    let module = app::Module::new().on(|s: &mut u32, _: &TestEvent| *s += 1);

    let mut app = app::App::new(0).mount(|s: &mut u32| s, module);
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
        .on(|_: &mut u32, _: &EventA| EventB)
        .on(|_: &mut u32, _: &EventB| EventC)
        .on(|_: &mut u32, _: &EventC| EventD)
        .on(|s: &mut u32, _: &EventD| *s += 1);

    let mut app = app::App::new(0u32).mount(|s: &mut u32| s, module);
    app.dispatch(&EventA);

    assert_eq!(*app.state(), 1);
}

#[test]
fn many_events_propagate_two_levels_deep() {
    struct Trigger;
    impl app::Event for Trigger {}
    struct Step;
    impl app::Event for Step {}
    struct Increment;
    impl app::Event for Increment {}

    // Trigger → 3× Step → each Step → 3× Increment → each Increment += 1 → total 9
    let module = app::Module::new()
        .on(|_: &mut u32, _: &Trigger| app::Many(vec![Step, Step, Step]))
        .on(|_: &mut u32, _: &Step| app::Many(vec![Increment, Increment, Increment]))
        .on(|s: &mut u32, _: &Increment| *s += 1);

    let mut app = app::App::new(0u32).mount(|s: &mut u32| s, module);
    app.dispatch(&Trigger);

    assert_eq!(*app.state(), 9);
}

#[test]
fn handler_returning_empty_many_dispatches_nothing() {
    struct Trigger;
    impl app::Event for Trigger {}
    struct Step;
    impl app::Event for Step {}

    let module = app::Module::new()
        .on(|_: &mut u32, _: &Trigger| app::Many(Vec::<Step>::new()))
        .on(|s: &mut u32, _: &Step| *s += 1);

    let mut app = app::App::new(0u32).mount(|s: &mut u32| s, module);
    app.dispatch(&Trigger);

    assert_eq!(*app.state(), 0);
}

#[test]
fn handler_returning_many_dispatches_all_in_order() {
    struct Trigger;
    impl app::Event for Trigger {}
    struct Step;
    impl app::Event for Step {}

    // Each Step increments by 1; Many emits 3 Steps → state should be 3
    let module = app::Module::new()
        .on(|_: &mut u32, _: &Trigger| app::Many(vec![Step, Step, Step]))
        .on(|s: &mut u32, _: &Step| *s += 1);

    let mut app = app::App::new(0u32).mount(|s: &mut u32| s, module);
    app.dispatch(&Trigger);

    assert_eq!(*app.state(), 3);
}

#[test]
fn handler_returning_none_emits_nothing() {
    struct RawResized;
    impl app::Event for RawResized {}
    struct Resized;
    impl app::Event for Resized {}

    let module = app::Module::new()
        .on(|_: &mut u32, _: &RawResized| -> Option<Resized> { None })
        .on(|s: &mut u32, _: &Resized| *s += 1);

    let mut app = app::App::new(0u32).mount(|s: &mut u32| s, module);
    app.dispatch(&RawResized);

    assert_eq!(*app.state(), 0);
}

#[test]
fn handler_returning_some_emits_event() {
    struct RawResized;
    impl app::Event for RawResized {}
    struct Resized;
    impl app::Event for Resized {}

    let module = app::Module::new()
        .on(|_: &mut u32, _: &RawResized| Some(Resized))
        .on(|s: &mut u32, _: &Resized| *s += 1);

    let mut app = app::App::new(0u32).mount(|s: &mut u32| s, module);
    app.dispatch(&RawResized);

    assert_eq!(*app.state(), 1);
}

#[test]
fn hlist_handler_dispatches_all_some_events() {
    struct Trigger;
    impl app::Event for Trigger {}
    struct EventA;
    impl app::Event for EventA {}
    struct EventB;
    impl app::Event for EventB {}

    let module = app::Module::new()
        .on(|_: &mut u32, _: &Trigger| hlist![Some(EventA), Some(EventB)])
        .on(|s: &mut u32, _: &EventA| *s += 1)
        .on(|s: &mut u32, _: &EventB| *s += 10);

    let mut app = app::App::new(0u32).mount(|s: &mut u32| s, module);
    app.dispatch(&Trigger);

    assert_eq!(*app.state(), 11);
}

#[test]
fn hlist_handler_none_element_suppresses_event() {
    struct Trigger;
    impl app::Event for Trigger {}
    struct EventA;
    impl app::Event for EventA {}
    struct EventB;
    impl app::Event for EventB {}

    let module = app::Module::new()
        .on(|_: &mut u32, _: &Trigger| hlist![None::<EventA>, Some(EventB)])
        .on(|s: &mut u32, _: &EventA| *s += 1)
        .on(|s: &mut u32, _: &EventB| *s += 10);

    let mut app = app::App::new(0u32).mount(|s: &mut u32| s, module);
    app.dispatch(&Trigger);

    assert_eq!(*app.state(), 10);
}

#[test]
fn hlist_handler_with_many_dispatches_all() {
    struct Trigger;
    impl app::Event for Trigger {}
    struct EventA;
    impl app::Event for EventA {}
    struct EventB;
    impl app::Event for EventB {}

    let module = app::Module::new()
        .on(|_: &mut u32, _: &Trigger| hlist![Some(EventA), app::Many(vec![EventB, EventB])])
        .on(|s: &mut u32, _: &EventA| *s += 1)
        .on(|s: &mut u32, _: &EventB| *s += 10);

    let mut app = app::App::new(0u32).mount(|s: &mut u32| s, module);
    app.dispatch(&Trigger);

    assert_eq!(*app.state(), 21);
}

// Cross-module propagation: emitted events reach all modules regardless of mount order.

#[test]
fn emitted_event_reaches_module_mounted_before_emitter() {
    // Module A is mounted first (deeper in HList).
    // Module B is mounted second and emits EventB when it sees EventA.
    // Module A must receive EventB even though it was mounted before B.
    struct EventA;
    impl app::Event for EventA {}
    struct EventB;
    impl app::Event for EventB {}

    let module_a = app::Module::new().on(|s: &mut (u32, u32), _: &EventB| s.0 += 1);
    let module_b = app::Module::new().on(|s: &mut (u32, u32), _: &EventA| {
        s.1 += 1;
        EventB
    });

    let mut app = app::App::new((0u32, 0u32))
        .mount(|s: &mut (u32, u32)| s, module_a)
        .mount(|s: &mut (u32, u32)| s, module_b);

    app.dispatch(&EventA);

    assert_eq!(app.state().0, 1, "module_a should have received EventB emitted by module_b");
    assert_eq!(app.state().1, 1, "module_b should have handled EventA");
}

#[test]
fn emitted_event_reaches_module_mounted_after_emitter() {
    // Module A is mounted first and emits EventB when it sees EventA.
    // Module B is mounted second and handles EventB.
    struct EventA;
    impl app::Event for EventA {}
    struct EventB;
    impl app::Event for EventB {}

    let module_a = app::Module::new().on(|s: &mut (u32, u32), _: &EventA| {
        s.0 += 1;
        EventB
    });
    let module_b = app::Module::new().on(|s: &mut (u32, u32), _: &EventB| s.1 += 1);

    let mut app = app::App::new((0u32, 0u32))
        .mount(|s: &mut (u32, u32)| s, module_a)
        .mount(|s: &mut (u32, u32)| s, module_b);

    app.dispatch(&EventA);

    assert_eq!(app.state().0, 1, "module_a should have handled EventA");
    assert_eq!(app.state().1, 1, "module_b should have received EventB emitted by module_a");
}

// Stack overflow aborts the process via SIGABRT — not catchable by #[should_panic].
// Run manually with: cargo test infinite_emission -- --ignored
#[test]
#[ignore]
fn infinite_emission_causes_stack_overflow() {
    struct PingEvent;
    impl app::Event for PingEvent {}

    // PingEvent always emits itself — infinite recursion
    let module = app::Module::new().on(|_: &mut u32, _: &PingEvent| PingEvent);

    let mut app = app::App::new(0u32).mount(|s: &mut u32| s, module);
    app.dispatch(&PingEvent);
}
