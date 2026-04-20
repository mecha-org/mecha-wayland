use glow::{COLOR_BUFFER_BIT, HasContext};

use crate::commands::{Command, CommandQueue};

#[derive(Clone)]
pub struct ClearColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}
impl Command for ClearColor {
    fn get_queue_from_registry(
        registry: &mut super::CommandQueueRegistry,
    ) -> &mut impl CommandQueue<Self> {
        &mut registry.clear_color_queue
    }
}

#[derive(Default)]
pub(crate) struct ClearColorQueue(Option<(f32, f32, f32, f32)>);

impl CommandQueue<ClearColor> for ClearColorQueue {
    fn init(&mut self, _gl: &glow::Context) {}

    fn enqueue(&mut self, command: ClearColor) {
        let mut color = (0.0, 0.0, 0.0, 0.0);
        color.0 = command.r;
        color.1 = command.g;
        color.2 = command.b;
        color.3 = command.a;
        self.0 = Some(color)
    }

    fn process(&mut self, gl: &glow::Context) {
        if let Some(color) = self.0 {
            unsafe {
                gl.clear_color(color.0, color.1, color.2, color.3);
                gl.clear(COLOR_BUFFER_BIT);
            }
        }
        self.0 = None;
    }
}
