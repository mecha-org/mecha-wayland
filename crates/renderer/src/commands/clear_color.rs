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
pub(crate) struct ClearColorQueue(f32, f32, f32, f32);

impl CommandQueue<ClearColor> for ClearColorQueue {
    fn init(&mut self, _gl: &glow::Context) {}

    fn enqueue(&mut self, command: ClearColor) {
        self.0 = command.r;
        self.1 = command.g;
        self.2 = command.b;
        self.3 = command.a;
    }

    fn process(&mut self, gl: &glow::Context) {
        unsafe {
            gl.clear_color(self.0, self.1, self.2, self.3);
            gl.clear(COLOR_BUFFER_BIT);
        }
    }
}
