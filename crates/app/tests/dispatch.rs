use app;
use app::prelude::*;

#[test]
fn dispatch_event() {
    #[derive(Debug)]
    struct TestEvent;
    impl app::Event for TestEvent {}

    let module = app::Module::new().on(|s: &mut u32, _: &TestEvent| *s += 1);
    let mut app = app::App::new(0u32).mount(module);
    app.dispatch(&TestEvent);

    assert_eq!(*app.state(), 1);
}

#[test]
fn unhandled_event_leaves_state_unchanged() {
    #[derive(Debug)]
    struct TestEvent;
    impl app::Event for TestEvent {}

    #[derive(Debug)]
    struct OtherEvent;
    impl app::Event for OtherEvent {}

    let module = app::Module::new().on(|s: &mut u32, _: &TestEvent| *s += 1);
    let mut app = app::App::new(0u32).mount(module);
    app.dispatch(&OtherEvent);

    assert_eq!(*app.state(), 0);
}

// ── Single-event emission ─────────────────────────────────────────────────────

#[test]
fn handler_emits_event() {
    #[derive(Debug)]
    struct Tick;
    impl app::Event for Tick {}
    #[derive(Debug)]
    struct Render;
    impl app::Event for Render {}

    let module = app::Module::new()
        .on(|_: &mut u32, _: &Tick| Render)
        .on(|s: &mut u32, _: &Render| *s += 1);

    let mut app = app::App::new(0u32).mount(module);
    app.dispatch(&Tick);

    assert_eq!(*app.state(), 1);
}

#[test]
fn emitted_event_chains_3_levels_deep() {
    #[derive(Debug)]
    struct EventA;
    impl app::Event for EventA {}
    #[derive(Debug)]
    struct EventB;
    impl app::Event for EventB {}
    #[derive(Debug)]
    struct EventC;
    impl app::Event for EventC {}
    #[derive(Debug)]
    struct EventD;
    impl app::Event for EventD {}

    let module = app::Module::new()
        .on(|_: &mut u32, _: &EventA| EventB)
        .on(|_: &mut u32, _: &EventB| EventC)
        .on(|_: &mut u32, _: &EventC| EventD)
        .on(|s: &mut u32, _: &EventD| *s += 1);

    let mut app = app::App::new(0u32).mount(module);
    app.dispatch(&EventA);

    assert_eq!(*app.state(), 1);
}

// ── Option emission ───────────────────────────────────────────────────────────

#[test]
fn handler_returning_none_emits_nothing() {
    #[derive(Debug)]
    struct RawResized;
    impl app::Event for RawResized {}
    #[derive(Debug)]
    struct Resized;
    impl app::Event for Resized {}

    let module = app::Module::new()
        .on(|_: &mut u32, _: &RawResized| -> Option<Resized> { None })
        .on(|s: &mut u32, _: &Resized| *s += 1);

    let mut app = app::App::new(0u32).mount(module);
    app.dispatch(&RawResized);

    assert_eq!(*app.state(), 0);
}

#[test]
fn handler_returning_some_emits_event() {
    #[derive(Debug)]
    struct RawResized;
    impl app::Event for RawResized {}
    #[derive(Debug)]
    struct Resized;
    impl app::Event for Resized {}

    let module = app::Module::new()
        .on(|_: &mut u32, _: &RawResized| Some(Resized))
        .on(|s: &mut u32, _: &Resized| *s += 1);

    let mut app = app::App::new(0u32).mount(module);
    app.dispatch(&RawResized);

    assert_eq!(*app.state(), 1);
}

// ── Many emission ─────────────────────────────────────────────────────────────

#[test]
fn many_empty_dispatches_nothing() {
    #[derive(Debug)]
    struct Trigger;
    impl app::Event for Trigger {}
    #[derive(Debug)]
    struct Step;
    impl app::Event for Step {}

    let module = app::Module::new()
        .on(|_: &mut u32, _: &Trigger| app::Many(Vec::<Step>::new()))
        .on(|s: &mut u32, _: &Step| *s += 1);

    let mut app = app::App::new(0u32).mount(module);
    app.dispatch(&Trigger);

    assert_eq!(*app.state(), 0);
}

#[test]
fn many_dispatches_all_in_order() {
    #[derive(Debug)]
    struct Trigger;
    impl app::Event for Trigger {}
    #[derive(Debug)]
    struct Step;
    impl app::Event for Step {}

    let module = app::Module::new()
        .on(|_: &mut u32, _: &Trigger| app::Many(vec![Step, Step, Step]))
        .on(|s: &mut u32, _: &Step| *s += 1);

    let mut app = app::App::new(0u32).mount(module);
    app.dispatch(&Trigger);

    assert_eq!(*app.state(), 3);
}

#[test]
fn many_chains_two_levels() {
    #[derive(Debug)]
    struct Trigger;
    impl app::Event for Trigger {}
    #[derive(Debug)]
    struct Step;
    impl app::Event for Step {}
    #[derive(Debug)]
    struct Increment;
    impl app::Event for Increment {}

    // Trigger → 3× Step → each Step → 3× Increment → total 9
    let module = app::Module::new()
        .on(|_: &mut u32, _: &Trigger| app::Many(vec![Step, Step, Step]))
        .on(|_: &mut u32, _: &Step| app::Many(vec![Increment, Increment, Increment]))
        .on(|s: &mut u32, _: &Increment| *s += 1);

    let mut app = app::App::new(0u32).mount(module);
    app.dispatch(&Trigger);

    assert_eq!(*app.state(), 9);
}

// ── HList emission ────────────────────────────────────────────────────────────

#[test]
fn hlist_dispatches_all_some_events() {
    #[derive(Debug)]
    struct Trigger;
    impl app::Event for Trigger {}
    #[derive(Debug)]
    struct EventA;
    impl app::Event for EventA {}
    #[derive(Debug)]
    struct EventB;
    impl app::Event for EventB {}

    let module = app::Module::new()
        .on(|_: &mut u32, _: &Trigger| hlist![Some(EventA), Some(EventB)])
        .on(|s: &mut u32, _: &EventA| *s += 1)
        .on(|s: &mut u32, _: &EventB| *s += 10);

    let mut app = app::App::new(0u32).mount(module);
    app.dispatch(&Trigger);

    assert_eq!(*app.state(), 11);
}

#[test]
fn hlist_none_element_suppresses_event() {
    #[derive(Debug)]
    struct Trigger;
    impl app::Event for Trigger {}
    #[derive(Debug)]
    struct EventA;
    impl app::Event for EventA {}
    #[derive(Debug)]
    struct EventB;
    impl app::Event for EventB {}

    let module = app::Module::new()
        .on(|_: &mut u32, _: &Trigger| hlist![None::<EventA>, Some(EventB)])
        .on(|s: &mut u32, _: &EventA| *s += 1)
        .on(|s: &mut u32, _: &EventB| *s += 10);

    let mut app = app::App::new(0u32).mount(module);
    app.dispatch(&Trigger);

    assert_eq!(*app.state(), 10);
}

#[test]
fn hlist_with_many_dispatches_all() {
    #[derive(Debug)]
    struct Trigger;
    impl app::Event for Trigger {}
    #[derive(Debug)]
    struct EventA;
    impl app::Event for EventA {}
    #[derive(Debug)]
    struct EventB;
    impl app::Event for EventB {}

    let module = app::Module::new()
        .on(|_: &mut u32, _: &Trigger| hlist![Some(EventA), app::Many(vec![EventB, EventB])])
        .on(|s: &mut u32, _: &EventA| *s += 1)
        .on(|s: &mut u32, _: &EventB| *s += 10);

    let mut app = app::App::new(0u32).mount(module);
    app.dispatch(&Trigger);

    assert_eq!(*app.state(), 21);
}
