use app::Event;

#[derive(Clone, Copy, Debug)]
pub enum RenderEvent {
    ProcessQueues,
}

impl Event for RenderEvent {}

pub fn module<AppState>() -> impl app::RegisteredModule<renderer::Renderer, AppState> {
    app::Module::<renderer::Renderer, _, _>::new()
        .on(|r: &mut renderer::Renderer, _: &app::Start| {
            r.init_command_queue::<renderer::commands::ClearColor>();
            r.init_command_queue::<renderer::commands::DrawRect>();
            r.init_command_queue::<renderer::commands::DrawQuad>();
            r.init_command_queue::<renderer::commands::DrawMonochromeSprite>();
            r.init_command_queue::<renderer::commands::DrawText>();
            None::<RenderEvent>
        })
        .on(|r: &mut renderer::Renderer, _: &RenderEvent| {
            r.process_command_queue::<renderer::commands::ClearColor>();
            r.process_command_queue::<renderer::commands::DrawRect>();
            r.process_command_queue::<renderer::commands::DrawMonochromeSprite>();
            r.process_command_queue::<renderer::commands::DrawText>();
            r.process_command_queue::<renderer::commands::DrawQuad>();
        })
}
