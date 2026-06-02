use app::prelude::*;

#[derive(Debug)] struct Tick;   impl Event for Tick {}
#[derive(Debug)] struct Resize; impl Event for Resize {}
#[derive(Debug)] struct Beat;   impl Event for Beat {}

struct Renderer { draw_calls: u32 }
struct Wayland  { committed: bool }
struct Audio    { volume: u32 }

#[derive(State)]
struct AppState {
    renderer: Renderer,
    wayland: Wayland,
    audio: Audio,
}

#[context]
struct RenderCtx {
    renderer: Renderer,
    wayland: Wayland,
}

#[context]
struct AudioCtx {
    audio: Audio,
}

fn make_state() -> AppState {
    AppState {
        renderer: Renderer { draw_calls: 0 },
        wayland: Wayland { committed: false },
        audio: Audio { volume: 0 },
    }
}

#[test]
fn with_context_single_field_mutates_state() {
    let module = Module::<AppState, _, _>::new()
        .on(with_context!(|ctx: RenderCtx<'_>, _: &Tick| {
            ctx.renderer.draw_calls += 1;
        }));

    let mut app = App::new(make_state()).mount(module);
    app.dispatch(&Tick);

    assert_eq!(app.state().renderer.draw_calls, 1);
}

#[test]
fn with_context_mixed_with_plain_handler() {
    let module = Module::<AppState, _, _>::new()
        .on(with_context!(|ctx: RenderCtx<'_>, _: &Tick| {
            ctx.renderer.draw_calls += 1;
        }))
        .on(|s: &mut AppState, _: &Resize| {
            s.wayland.committed = true;
        });

    let mut app = App::new(make_state()).mount(module);
    app.dispatch(&Tick);
    app.dispatch(&Resize);

    assert_eq!(app.state().renderer.draw_calls, 1);
    assert!(app.state().wayland.committed);
}

#[test]
fn with_context_multi_field_mutates_all() {
    let module = Module::<AppState, _, _>::new()
        .on(with_context!(|ctx: RenderCtx<'_>, _: &Tick| {
            ctx.renderer.draw_calls += 1;
            ctx.wayland.committed = true;
        }));

    let mut app = App::new(make_state()).mount(module);
    app.dispatch(&Tick);

    assert_eq!(app.state().renderer.draw_calls, 1);
    assert!(app.state().wayland.committed);
}

#[test]
fn with_context_multiple_context_types_no_conflict() {
    let module = Module::<AppState, _, _>::new()
        .on(with_context!(|ctx: RenderCtx<'_>, _: &Tick| {
            ctx.renderer.draw_calls += 1;
        }))
        .on(with_context!(|ctx: AudioCtx<'_>, _: &Beat| {
            ctx.audio.volume += 10;
        }));

    let mut app = App::new(make_state()).mount(module);
    app.dispatch(&Tick);
    app.dispatch(&Beat);

    assert_eq!(app.state().renderer.draw_calls, 1);
    assert_eq!(app.state().audio.volume, 10);
}
