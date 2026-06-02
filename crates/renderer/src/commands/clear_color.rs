use glow::{COLOR_BUFFER_BIT, DEPTH_BUFFER_BIT, HasContext};
use utils::Color;

use crate::commands::{Command, CommandQueue, RenderContext};

#[derive(Clone, Copy)]
pub struct ClearColor(pub Color);

impl ClearColor {
    /// Construct from normalized `[0, 1]` RGB components with full opacity.
    #[inline]
    pub fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self(Color::rgb(r, g, b))
    }
}

impl From<Color> for ClearColor {
    fn from(c: Color) -> Self {
        Self(c)
    }
}

impl Command for ClearColor {
    fn get_queue_from_registry(
        registry: &mut super::CommandQueueRegistry,
    ) -> &mut impl CommandQueue<Self> {
        &mut registry.clear_color_queue
    }
}

#[derive(Default)]
pub(crate) struct ClearColorQueue(Option<Color>);

impl CommandQueue<ClearColor> for ClearColorQueue {
    fn init(&mut self, _ctx: &RenderContext) {}

    fn enqueue(&mut self, command: ClearColor) {
        self.0 = Some(command.0);
    }

    fn process(&mut self, ctx: &RenderContext) {
        if let Some(c) = self.0.take() {
            unsafe {
                ctx.gl.clear_color(c.r, c.g, c.b, c.a);
                ctx.gl.clear_depth_f32(0.0);
                ctx.gl.clear(COLOR_BUFFER_BIT | DEPTH_BUFFER_BIT);
            }
        }
    }
}
