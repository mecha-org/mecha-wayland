use crate::commands::{clear_color::ClearColorQueue, draw_rect::RectQueue};

mod clear_color;
pub use clear_color::ClearColor;

mod draw_rect;
pub use draw_rect::DrawRect;

pub struct RenderContext<'a> {
    pub gl: &'a glow::Context,
    pub viewport_width: u32,
    pub viewport_height: u32,
}

pub trait Command: Clone {
    fn get_queue_from_registry(registry: &mut CommandQueueRegistry)
    -> &mut impl CommandQueue<Self>;
}

pub trait CommandQueue<C: Command>: Default {
    fn init(&mut self, ctx: &RenderContext);
    fn enqueue(&mut self, command: C);
    fn process(&mut self, ctx: &RenderContext);
}

impl CommandQueueRegistry {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn init_queue<C: Command>(&mut self, ctx: &RenderContext) {
        C::get_queue_from_registry(self).init(ctx);
    }

    pub fn enqueue<C: Command>(&mut self, command: C) {
        C::get_queue_from_registry(self).enqueue(command);
    }

    pub fn process<C: Command>(&mut self, ctx: &RenderContext) {
        C::get_queue_from_registry(self).process(ctx);
    }
}

#[derive(Default)]
pub struct CommandQueueRegistry {
    clear_color_queue: ClearColorQueue,
    draw_rect_queue: RectQueue,
}
