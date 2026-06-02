use app::{Compose, Lens};

// ── shared state types ────────────────────────────────────────────────────────

struct Renderer {
    draw_calls: u32,
}

struct Wayland {
    committed: bool,
}

struct ButtonState {
    clicked: bool,
    click_count: u32,
}

struct AppState {
    renderer: Renderer,
    wayland: Wayland,
    button: ButtonState,
}

unsafe impl Lens<Renderer> for AppState {
    fn lens(&mut self) -> &mut Renderer {
        &mut self.renderer
    }
}

unsafe impl Lens<Wayland> for AppState {
    fn lens(&mut self) -> &mut Wayland {
        &mut self.wayland
    }
}

unsafe impl Lens<ButtonState> for AppState {
    fn lens(&mut self) -> &mut ButtonState {
        &mut self.button
    }
}

// ── context structs ───────────────────────────────────────────────────────────

struct RenderCtx<'a> {
    renderer: &'a mut Renderer,
    wayland: &'a mut Wayland,
}

unsafe impl<'a, S> Compose<'a, S> for RenderCtx<'a>
where
    S: Lens<Renderer> + Lens<Wayland> + 'a,
{
    fn compose(state: &'a mut S) -> Self {
        unsafe {
            RenderCtx {
                renderer: &mut *(Lens::<Renderer>::lens(state) as *mut Renderer),
                wayland: &mut *(Lens::<Wayland>::lens(state) as *mut Wayland),
            }
        }
    }
}

struct ButtonCtx<'a> {
    renderer: &'a mut Renderer,
    wayland: &'a mut Wayland,
    state: &'a mut ButtonState,
}

unsafe impl<'a, S> Compose<'a, S> for ButtonCtx<'a>
where
    S: Lens<Renderer> + Lens<Wayland> + Lens<ButtonState> + 'a,
{
    fn compose(state: &'a mut S) -> Self {
        unsafe {
            ButtonCtx {
                renderer: &mut *(Lens::<Renderer>::lens(state) as *mut Renderer),
                wayland: &mut *(Lens::<Wayland>::lens(state) as *mut Wayland),
                state: &mut *(Lens::<ButtonState>::lens(state) as *mut ButtonState),
            }
        }
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn make_state() -> AppState {
    AppState {
        renderer: Renderer { draw_calls: 0 },
        wayland: Wayland { committed: false },
        button: ButtonState { clicked: false, click_count: 0 },
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[test]
fn compose_two_fields() {
    let mut state = make_state();
    let ctx = RenderCtx::compose(&mut state);

    ctx.renderer.draw_calls += 1;
    ctx.wayland.committed = true;

    assert_eq!(state.renderer.draw_calls, 1);
    assert!(state.wayland.committed);
}

#[test]
fn compose_three_fields() {
    let mut state = make_state();
    let ctx = ButtonCtx::compose(&mut state);

    ctx.renderer.draw_calls += 3;
    ctx.wayland.committed = true;
    ctx.state.clicked = true;
    ctx.state.click_count += 1;

    assert_eq!(state.renderer.draw_calls, 3);
    assert!(state.wayland.committed);
    assert!(state.button.clicked);
    assert_eq!(state.button.click_count, 1);
}

#[test]
fn compose_mutations_are_independent() {
    let mut state = make_state();
    let ctx = ButtonCtx::compose(&mut state);

    ctx.renderer.draw_calls = 99;

    assert_eq!(state.renderer.draw_calls, 99);
    assert!(!state.wayland.committed);
    assert!(!state.button.clicked);
}

#[test]
fn compose_same_state_twice_sequential() {
    let mut state = make_state();

    {
        let ctx = RenderCtx::compose(&mut state);
        ctx.renderer.draw_calls += 1;
    }
    {
        let ctx = RenderCtx::compose(&mut state);
        ctx.renderer.draw_calls += 1;
    }

    assert_eq!(state.renderer.draw_calls, 2);
}

#[test]
fn compose_subset_does_not_affect_excluded_field() {
    let mut state = make_state();
    state.button.click_count = 42;

    let ctx = RenderCtx::compose(&mut state);
    ctx.renderer.draw_calls = 7;

    assert_eq!(state.button.click_count, 42, "button state must be untouched");
}
