use crate::commands::{
    clear_color::ClearColorQueue,
    draw_mono_sprite::MonoSpriteQueue,
    draw_quad::QuadQueue,
    draw_rect::RectQueue,
    draw_text::DrawTextQueue,
};
use crate::texture::TextureStore;

mod clear_color;
pub use clear_color::ClearColor;

mod draw_quad;
pub use draw_quad::DrawQuad;

mod draw_rect;
pub use draw_rect::DrawRect;

mod draw_mono_sprite;
pub use draw_mono_sprite::DrawMonochromeSprite;

mod draw_text;
pub use draw_text::DrawText;

pub struct RenderContext<'a> {
    pub gl:              &'a glow::Context,
    pub viewport_width:  u32,
    pub viewport_height: u32,
    pub(crate) textures: &'a TextureStore,
}

pub trait Command: Clone {
    fn get_queue_from_registry(registry: &mut CommandQueueRegistry)
    -> &mut impl CommandQueue<Self>;

    fn on_enqueue(_registry: &mut CommandQueueRegistry, _command: &Self) {}
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
        C::on_enqueue(self, &command);
        C::get_queue_from_registry(self).enqueue(command);
    }

    pub fn process<C: Command>(&mut self, ctx: &RenderContext) {
        C::get_queue_from_registry(self).process(ctx);
    }
}

#[derive(Default)]
pub struct CommandQueueRegistry {
    clear_color_queue:      ClearColorQueue,
    draw_rect_queue:        RectQueue,
    draw_quad_queue:        QuadQueue,
    draw_mono_sprite_queue: MonoSpriteQueue,
    draw_text_queue:        DrawTextQueue,
}
