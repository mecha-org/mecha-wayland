pub fn module<AppState>() -> impl app::RegisteredModule<renderer::Renderer, AppState> {
    app::Module::<renderer::Renderer, _, _>::new().on(
        |r: &mut renderer::Renderer, _: &app::Start| {
            r.init_command_queue::<renderer::commands::ClearColor>();
            r.init_command_queue::<renderer::commands::DrawRect>();
            r.init_command_queue::<renderer::commands::DrawQuad>();
            r.init_command_queue::<renderer::commands::DrawMonochromeSprite>();
            r.init_command_queue::<renderer::commands::DrawText>();
        },
    )
}
