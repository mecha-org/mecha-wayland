use assets::BakedFont;
use utils::{Color, Point, Rect, Size};

use crate::{
    commands::{Command, CommandQueue, DrawMonochromeSprite, RenderContext},
    texture::TextureId,
};

#[derive(Clone)]
pub struct DrawText {
    pub font: &'static BakedFont,
    pub texture_id: TextureId,
    pub text: String,
    /// Baseline-left position in screen pixels.
    pub origin: Point,
    /// Depth value.
    pub z: f32,
    pub color: Color,
}

impl Command for DrawText {
    fn get_queue_from_registry(
        registry: &mut super::CommandQueueRegistry,
    ) -> &mut impl CommandQueue<Self> {
        &mut registry.draw_text_queue
    }

    fn on_enqueue(registry: &mut super::CommandQueueRegistry, cmd: &Self) {
        let mut pen_x = cmd.origin.x();
        let baseline_y = cmd.origin.y();
        let z = cmd.z;

        for ch in cmd.text.chars() {
            let byte = ch as u32;
            if byte < 32 || byte > 126 {
                continue;
            }
            let glyph = &cmd.font.glyphs[(byte - 32) as usize];
            if glyph.w > 0.0 && glyph.h > 0.0 {
                registry.enqueue(DrawMonochromeSprite {
                    texture_id: cmd.texture_id,
                    region: Rect::xywh(glyph.x, glyph.y, glyph.w, glyph.h),
                    origin: Point::new(
                        pen_x + glyph.bearing_x,
                        baseline_y - glyph.bearing_y - glyph.h,
                    ),
                    z,
                    size: Size::new(glyph.w, glyph.h),
                    color: cmd.color,
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
