use interactivity::hit::{HitArea, HitAreaRegistry};
use renderer::Renderer;
use renderer::commands::{ClearColor, DrawMonochromeSprite, DrawQuad, DrawRect, DrawText};
use ui::RenderCommand;
use utils::Color;

/// Populate a [`HitAreaRegistry`] from a slice of render commands.
pub fn collect_hit_areas(registry: &mut HitAreaRegistry, commands: &[RenderCommand]) {
    registry.clear();
    for cmd in commands {
        if let RenderCommand::RegisterHitArea { id, rect } = cmd {
            registry.push(HitArea {
                id: *id,
                rect: *rect,
            });
        }
    }
}

/// Draw a frame and optionally populate a [`HitAreaRegistry`] from
pub fn render_frame(renderer: &mut Renderer, commands: Vec<RenderCommand>, bg_color: Color) {
    renderer.send_command(ClearColor::rgb(bg_color.r, bg_color.g, bg_color.b));

    for cmd in commands {
        match cmd {
            RenderCommand::DrawQuad {
                color,
                border_color,
                origin,
                z,
                size,
                border_radius,
                border_thickness,
            } => {
                renderer.send_command(DrawQuad {
                    color,
                    border_color,
                    origin,
                    z,
                    size,
                    border_radius,
                    border_thickness,
                });
            }

            RenderCommand::DrawText {
                font,
                text,
                origin,
                z,
                color,
                atlas_id: Some(aid),
            } => {
                let texture_id = renderer.get_texture_id(aid);
                renderer.send_command(DrawText {
                    font,
                    texture_id,
                    text,
                    origin,
                    z,
                    color,
                });
            }

            // Text node with no atlas — skip silently.
            RenderCommand::DrawText { atlas_id: None, .. } => {}

            // RegisterHitArea is handled by collect_hit_areas(); nothing to draw here.
            RenderCommand::RegisterHitArea { .. } => {}
        }
    }

    // Process in the same order as the renderer pipeline requires.
    renderer.process_command_queue::<ClearColor>();
    renderer.process_command_queue::<DrawRect>();
    renderer.process_command_queue::<DrawQuad>();
    renderer.process_command_queue::<DrawMonochromeSprite>();
    renderer.process_command_queue::<DrawText>();
    renderer.finish();
}
