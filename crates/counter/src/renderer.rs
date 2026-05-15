use app::event::Event;

#[derive(Clone, Copy, Debug)]
pub enum RenderEvent {
    ProcessQueues,
}

impl Event for RenderEvent {}

#[macro_export]
macro_rules! register_renderer {
    () => {
        app::module::Module::<::renderer::Renderer>::new()
            .processor(|r: &mut ::renderer::Renderer, _: &app::Start| {
                r.init_command_queue::<::renderer::commands::ClearColor>();
                r.init_command_queue::<::renderer::commands::DrawRect>();
                r.init_command_queue::<::renderer::commands::DrawQuad>();
                r.init_command_queue::<::renderer::commands::DrawMonochromeSprite>();
                r.init_command_queue::<::renderer::commands::DrawText>();
                None::<crate::renderer::RenderEvent>
            })
            .on(|r: &mut ::renderer::Renderer, _: &crate::renderer::RenderEvent| {
                r.process_command_queue::<::renderer::commands::ClearColor>();
                r.process_command_queue::<::renderer::commands::DrawRect>();
                r.process_command_queue::<::renderer::commands::DrawMonochromeSprite>();
                r.process_command_queue::<::renderer::commands::DrawText>();
                r.process_command_queue::<::renderer::commands::DrawQuad>();
            })
    };
}
