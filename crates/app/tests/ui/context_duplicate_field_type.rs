use app::prelude::*;

struct Renderer { draw_calls: u32 }

#[context]
struct BadCtx {
    a: Renderer,
    b: Renderer,
}

fn main() {}
