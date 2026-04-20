use crate::commands::clear_color::ClearColorQueue;

mod clear_color;
pub use clear_color::ClearColor;

pub trait Command: Clone {
    fn get_queue_from_registry(registry: &mut CommandQueueRegistry)
    -> &mut impl CommandQueue<Self>;
}

pub trait CommandQueue<C: Command>: Default {
    fn init(&mut self, gl: &glow::Context);
    fn enqueue(&mut self, command: C);
    fn process(&mut self, gl: &glow::Context);
}

impl CommandQueueRegistry {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn init_queue<C: Command>(&mut self, gl: &glow::Context) {
        C::get_queue_from_registry(self).init(gl);
    }

    pub fn enqueue<C: Command>(&mut self, command: C) {
        C::get_queue_from_registry(self).enqueue(command);
    }

    pub fn process<C: Command>(&mut self, gl: &glow::Context) {
        C::get_queue_from_registry(self).process(gl);
    }
}

#[derive(Default)]
pub struct CommandQueueRegistry {
    clear_color_queue: ClearColorQueue,
}
