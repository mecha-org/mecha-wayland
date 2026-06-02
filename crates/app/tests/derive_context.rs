use app::prelude::*;

struct Renderer { draw_calls: u32 }
struct Wayland  { committed: bool }

#[derive(State)]
struct AppState {
    renderer: Renderer,
    wayland: Wayland,
}

#[context]
struct RenderCtx {
    renderer: Renderer,
    wayland: Wayland,
}

#[test]
fn derive_context_blanket_compose_impl() {
    let mut state = AppState {
        renderer: Renderer { draw_calls: 0 },
        wayland: Wayland { committed: false },
    };

    let ctx = RenderCtx::compose(&mut state);
    ctx.renderer.draw_calls += 1;
    ctx.wayland.committed = true;

    assert_eq!(state.renderer.draw_calls, 1);
    assert!(state.wayland.committed);
}

#[test]
fn derive_context_subset_does_not_affect_excluded_field() {
    let mut state = AppState {
        renderer: Renderer { draw_calls: 0 },
        wayland: Wayland { committed: false },
    };

    let ctx = RenderCtx::compose(&mut state);
    ctx.renderer.draw_calls = 7;

    assert!(!state.wayland.committed);
}
