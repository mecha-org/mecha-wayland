use assets::BakedFont;

use crate::commands::{Command, CommandQueue, RenderContext, DrawMonochromeSprite};
use crate::texture::TextureId;

#[derive(Clone)]
pub struct DrawText {
    pub font:       &'static BakedFont,
    pub texture_id: TextureId,
    pub text:       String,
    pub origin:     (f32, f32, f32), // baseline-left (x, y, z-depth)
    pub color:      (f32, f32, f32, f32),
}

impl Command for DrawText {
    fn get_queue_from_registry(
        registry: &mut super::CommandQueueRegistry,
    ) -> &mut impl CommandQueue<Self> {
        &mut registry.draw_text_queue
    }

    fn on_enqueue(registry: &mut super::CommandQueueRegistry, cmd: &Self) {
        let (mut pen_x, baseline_y, z) = cmd.origin;
        for ch in cmd.text.chars() {
            let byte = ch as u32;
            if byte < 32 || byte > 126 {
                continue;
            }
            let glyph = &cmd.font.glyphs[(byte - 32) as usize];
            if glyph.w > 0.0 && glyph.h > 0.0 {
                registry.enqueue(DrawMonochromeSprite {
                    texture_id: cmd.texture_id,
                    region:     (glyph.x, glyph.y, glyph.w, glyph.h),
                    origin:     (pen_x + glyph.bearing_x, baseline_y - glyph.bearing_y - glyph.h, z),
                    size:       (glyph.w, glyph.h),
                    color:      cmd.color,
                });
            }
            pen_x += glyph.advance;
        }
    }
}

#[derive(Default)]
pub(crate) struct DrawTextQueue;

impl CommandQueue<DrawText> for DrawTextQueue {
    fn init(&mut self, _ctx: &RenderContext) {}
    fn enqueue(&mut self, _command: DrawText) {}
    fn process(&mut self, _ctx: &RenderContext) {}
}
