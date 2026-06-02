use assets::BakedFont;
use renderer::{Renderer, TextureId, commands::*};
use utils::{Color, Point, Rect, Size};

use crate::atlas;

pub fn draw_centered_text(
    renderer: &mut Renderer,
    font: &'static BakedFont,
    texture_id: TextureId,
    text: String,
    bb: &Rect,
    z: f32,
    color: Color,
) {
    let text_w = font.measure_width(&text);
    let center_x = bb.x() + (bb.width() - text_w) / 2.0;
    let center_y = bb.y() + font.get_baseline_offset(bb.height());

    renderer.send_command(DrawText {
        font,
        texture_id,
        text,
        origin: Point::new(center_x, center_y),
        z,
        color,
    });
}

pub fn draw_tab_button(
    renderer: &mut Renderer,
    icon_tex: TextureId,
    label: String,
    is_active: bool,
    bb: Rect,
    theme_colors: &crate::ui::theme::ThemeColors,
) {
    if is_active {
        renderer.send_command(DrawQuad {
            color: theme_colors.tab_bg_active,
            border_color: theme_colors.tab_border_active,
            origin: Point::new(bb.x(), bb.y()),
            z: 0.2,
            size: Size::new(bb.width(), bb.height()),
            border_radius: 12.0,
            border_thickness: 1.5,
        });
        draw_centered_text(
            renderer,
            &atlas::UI_FONT_MONO_16,
            icon_tex,
            label,
            &bb,
            0.9,
            theme_colors.tab_text_active,
        );
    } else {
        draw_centered_text(
            renderer,
            &atlas::UI_FONT_MONO_16,
            icon_tex,
            label,
            &bb,
            0.9,
            theme_colors.tab_text_inactive,
        );
    }
}

pub fn draw_action_button(
    renderer: &mut Renderer,
    icon_tex: TextureId,
    label: String,
    bb: Rect,
    z: f32,
    bg_color: Color,
    border_color: Color,
    text_color: Color,
) {
    renderer.send_command(DrawQuad {
        color: bg_color,
        border_color,
        origin: Point::new(bb.x(), bb.y()),
        z,
        size: Size::new(bb.width(), bb.height()),
        border_radius: 12.0,
        border_thickness: 1.5,
    });
    draw_centered_text(
        renderer,
        &atlas::UI_FONT_MONO_16,
        icon_tex,
        label,
        &bb,
        z + 0.05,
        text_color,
    );
}
