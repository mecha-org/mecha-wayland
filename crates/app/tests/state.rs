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
fn dispatch_other_event() {
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

#[test]
fn handler_can_emit_event() {
    #[derive(Debug)]
    struct TestEvent;
    impl app::Event for TestEvent {}

    let module = app::Module::new().on(|s: &mut u32, _: &TestEvent| *s += 1);

    let mut app = app::App::new(0u32).mount(module);
    app.dispatch(&TestEvent);

    assert_eq!(*app.state(), 1);
}

#[test]
fn emitted_event_propagates_3_levels_deep() {
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

    // EventA → emits EventB → emits EventC → emits EventD → increments state
    let module = app::Module::new()
        .on(|_: &mut u32, _: &EventA| EventB)
        .on(|_: &mut u32, _: &EventB| EventC)
        .on(|_: &mut u32, _: &EventC| EventD)
        .on(|s: &mut u32, _: &EventD| *s += 1);

    let mut app = app::App::new(0u32).mount(module);
    app.dispatch(&EventA);

    assert_eq!(*app.state(), 1);
}

#[test]
fn many_events_propagate_two_levels_deep() {
    #[derive(Debug)]
    struct Trigger;
    impl app::Event for Trigger {}
    #[derive(Debug)]
    struct Step;
    impl app::Event for Step {}
    #[derive(Debug)]
    struct Increment;
    impl app::Event for Increment {}

    // Trigger → 3× Step → each Step → 3× Increment → each Increment += 1 → total 9
    let module = app::Module::new()
        .on(|_: &mut u32, _: &Trigger| app::Many(vec![Step, Step, Step]))
        .on(|_: &mut u32, _: &Step| app::Many(vec![Increment, Increment, Increment]))
        .on(|s: &mut u32, _: &Increment| *s += 1);

    let mut app = app::App::new(0u32).mount(module);
    app.dispatch(&Trigger);

    assert_eq!(*app.state(), 9);
}

#[test]
fn handler_returning_empty_many_dispatches_nothing() {
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
fn handler_returning_many_dispatches_all_in_order() {
    #[derive(Debug)]
    struct Trigger;
    impl app::Event for Trigger {}
    #[derive(Debug)]
    struct Step;
    impl app::Event for Step {}

    // Each Step increments by 1; Many emits 3 Steps → state should be 3
    let module = app::Module::new()
        .on(|_: &mut u32, _: &Trigger| app::Many(vec![Step, Step, Step]))
        .on(|s: &mut u32, _: &Step| *s += 1);

    let mut app = app::App::new(0u32).mount(module);
    app.dispatch(&Trigger);

    assert_eq!(*app.state(), 3);
}

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

#[test]
fn hlist_handler_dispatches_all_some_events() {
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
fn hlist_handler_none_element_suppresses_event() {
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
fn hlist_handler_with_many_dispatches_all() {
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

// Cross-module propagation: emitted events reach all modules regardless of mount order.

#[test]
fn emitted_event_reaches_module_mounted_before_emitter() {
    // Module A is mounted first (deeper in HList).
    // Module B is mounted second and emits EventB when it sees EventA.
    // Module A must receive EventB even though it was mounted before B.
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

    let mut app = app::App::new((0u32, 0u32))
        .mount(module_a)
        .mount(module_b);

    app.dispatch(&EventA);

    assert_eq!(
        app.state().0,
        1,
        "module_a should have received EventB emitted by module_b"
    );
    assert_eq!(app.state().1, 1, "module_b should have handled EventA");
}

#[test]
fn emitted_event_reaches_module_mounted_after_emitter() {
    // Module A is mounted first and emits EventB when it sees EventA.
    // Module B is mounted second and handles EventB.
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

    let mut app = app::App::new((0u32, 0u32))
        .mount(module_a)
        .mount(module_b);

    app.dispatch(&EventA);

    assert_eq!(app.state().0, 1, "module_a should have handled EventA");
    assert_eq!(
        app.state().1,
        1,
        "module_b should have received EventB emitted by module_a"
    );
}

// Submodule tests

#[test]
fn submodule_receives_dispatched_event() {
    #[derive(Debug)]
    struct Tick;
    impl app::Event for Tick {}

    #[derive(Debug)]
    struct ChildState(u32);

    #[derive(Debug)]
    struct State {
        child: ChildState,
    }

    impl app::Lens<ChildState> for State {
        fn lens(&mut self) -> &mut ChildState {
            &mut self.child
        }
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

    #[derive(Debug)]
    struct State {
        child: ChildState,
        root: u32,
    }

    impl app::Lens<ChildState> for State {
        fn lens(&mut self) -> &mut ChildState {
            &mut self.child
        }
    }

    // Child emits Response when it sees Trigger
    let child = app::Module::new().on(|_: &mut ChildState, _: &Trigger| Response);

    // Root module handles Response (mounted separately at app level)
    let root_module = app::Module::new().on(|s: &mut State, _: &Response| s.root += 1);

    let parent = app::Module::<State, _, _>::new().mount(child);

    let mut app = app::App::new(State { child: ChildState(0), root: 0 })
        .mount(parent)
        .mount(root_module);

    app.dispatch(&Trigger);

    assert_eq!(
        app.state().root,
        1,
        "Response emitted by submodule should reach root module"
    );
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

    #[derive(Debug)]
    struct State {
        child: ChildState,
        root: u32,
    }

    impl app::Lens<ChildState> for State {
        fn lens(&mut self) -> &mut ChildState {
            &mut self.child
        }
    }

    // Root module emits Response when it sees Trigger
    let root_module = app::Module::new().on(|s: &mut State, _: &Trigger| {
        s.root += 1;
        Response
    });

    // Child handles Response
    let child = app::Module::new().on(|s: &mut ChildState, _: &Response| s.0 += 1);
    let parent = app::Module::<State, _, _>::new().mount(child);

    let mut app = app::App::new(State { child: ChildState(0), root: 0 })
        .mount(root_module)
        .mount(parent);

    app.dispatch(&Trigger);

    assert_eq!(
        app.state().child.0,
        1,
        "Response emitted by root module should reach submodule"
    );
}

#[test]
fn grandchild_emitted_event_reaches_app_root() {
    #[derive(Debug)]
    struct Tick;
    impl app::Event for Tick {}
    #[derive(Debug)]
    struct Done;
    impl app::Event for Done {}

    #[derive(Debug)]
    struct GrandchildState {
        count: u32,
    }
    #[derive(Debug)]
    struct ChildState {
        grandchild: GrandchildState,
    }
    #[derive(Debug)]
    struct AppState {
        child: ChildState,
        done_count: u32,
    }

    impl app::Lens<GrandchildState> for ChildState {
        fn lens(&mut self) -> &mut GrandchildState {
            &mut self.grandchild
        }
    }

    impl app::Lens<ChildState> for AppState {
        fn lens(&mut self) -> &mut ChildState {
            &mut self.child
        }
    }

    // Grandchild emits Done on Tick
    let grandchild = app::Module::new().on(|_: &mut GrandchildState, _: &Tick| Done);

    // Child just forwards — no handlers, only a mounted grandchild
    let child = app::Module::<ChildState, _, _>::new().mount(grandchild);

    // Parent mounts child
    let parent = app::Module::<AppState, _, _>::new().mount(child);

    // Separate root module counts Done events
    let counter = app::Module::new().on(|s: &mut AppState, _: &Done| s.done_count += 1);

    let mut app = app::App::new(AppState {
        child: ChildState {
            grandchild: GrandchildState { count: 0 },
        },
        done_count: 0,
    })
    .mount(parent)
    .mount(counter);

    app.dispatch(&Tick);

    assert_eq!(
        app.state().done_count,
        1,
        "Done emitted by grandchild should reach root counter"
    );
}

// Lens tests

#[test]
fn lens_identity_blanket_impl() {
    // When the module state equals the app state, no manual Lens impl is needed.
    #[derive(Debug)]
    struct Tick;
    impl app::Event for Tick {}

    let module = app::Module::new().on(|s: &mut u32, _: &Tick| *s += 1);
    let mut app = app::App::new(0u32).mount(module);
    app.dispatch(&Tick);

    assert_eq!(*app.state(), 1);
}

#[test]
fn lens_field_extraction() {
    // Lens<T> impl on a struct extracts the correct field.
    #[derive(Debug)]
    struct Tick;
    impl app::Event for Tick {}

    struct State {
        a: u32,
        b: u32,
    }

    impl app::Lens<u32> for State {
        fn lens(&mut self) -> &mut u32 {
            &mut self.a
        }
    }

    // Only `a` should be incremented; `b` is untouched.
    let module = app::Module::new().on(|s: &mut u32, _: &Tick| *s += 1);
    let mut app = app::App::new(State { a: 0, b: 10 }).mount(module);
    app.dispatch(&Tick);

    assert_eq!(app.state().a, 1);
    assert_eq!(app.state().b, 10);
}

#[test]
fn lens_two_modules_on_different_fields() {
    // Two modules each owning a distinct field via separate Lens impls.
    #[derive(Debug)]
    struct TickA;
    impl app::Event for TickA {}
    #[derive(Debug)]
    struct TickB;
    impl app::Event for TickB {}

    struct Counter(u32);
    struct Timer(u32);

    struct State {
        counter: Counter,
        timer: Timer,
    }

    impl app::Lens<Counter> for State {
        fn lens(&mut self) -> &mut Counter {
            &mut self.counter
        }
    }

    impl app::Lens<Timer> for State {
        fn lens(&mut self) -> &mut Timer {
            &mut self.timer
        }
    }

    let counter_module = app::Module::new().on(|s: &mut Counter, _: &TickA| s.0 += 1);
    let timer_module = app::Module::new().on(|s: &mut Timer, _: &TickB| s.0 += 10);

    let mut app = app::App::new(State {
        counter: Counter(0),
        timer: Timer(0),
    })
    .mount(counter_module)
    .mount(timer_module);

    app.dispatch(&TickA);
    app.dispatch(&TickB);

    assert_eq!(app.state().counter.0, 1);
    assert_eq!(app.state().timer.0, 10);
}

// Stack overflow aborts the process via SIGABRT — not catchable by #[should_panic].
// Run manually with: cargo test infinite_emission -- --ignored
#[test]
#[ignore]
fn infinite_emission_causes_stack_overflow() {
    #[derive(Debug)]
    struct PingEvent;
    impl app::Event for PingEvent {}

    // PingEvent always emits itself — infinite recursion
    let module = app::Module::new().on(|_: &mut u32, _: &PingEvent| PingEvent);

    let mut app = app::App::new(0u32).mount(module);
    app.dispatch(&PingEvent);
}
