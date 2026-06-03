use chrono::Local;
use utils::{Color, Point, Rect, Size};

use crate::{
    atlas,
    state::{self, HitBoxes},
};

pub fn render(
    s: &mut state::AppState,
    body_w: f32,
    body_h: f32,
    win_w: f32,
    win_h: f32,
    icon_tex: renderer::TextureId,
    hit_boxes: &mut HitBoxes,
) {
    use crate::ui::components::draw_centered_text;
    use layout::layout;
    use renderer::commands::DrawQuad;

    let theme_colors = s.ui.theme.colors();
    let (time_str, date_str) = get_local_time(s.ui.format_24h, s.ui.show_seconds);

    layout!(
        {
            available_width: body_w,
            available_height: body_h,
            direction: column,
            gap: 20.0,
            padding_top: 60.0,

            layout!({ height: 80.0 }, {
                let bb = Rect::xywh(x, y, width, height);
                draw_centered_text(&mut s.renderer, &atlas::UI_FONT_MONO_56, icon_tex, time_str, &bb, 0.9, theme_colors.text_primary);
            }),
            layout!({ height: 30.0 }, {
                let bb = Rect::xywh(x, y, width, height);
                draw_centered_text(&mut s.renderer, &atlas::UI_FONT_MONO_16, icon_tex, date_str, &bb, 0.9, theme_colors.text_secondary);
            }),
        },
        {}
    );

    let settings_btn_bb = Rect::xywh(win_w - 92.0, 16.0, 76.0, 28.0);
    hit_boxes.settings_btn = settings_btn_bb;

    if !s.ui.show_settings {
        s.renderer.send_command(DrawQuad {
            color: theme_colors.btn_bg,
            border_color: theme_colors.btn_border,
            origin: Point::new(settings_btn_bb.x(), settings_btn_bb.y()),
            z: 0.6,
            size: Size::new(settings_btn_bb.width(), settings_btn_bb.height()),
            border_radius: 8.0,
            border_thickness: 1.0,
        });
        draw_centered_text(
            &mut s.renderer,
            &atlas::UI_FONT_MONO_14,
            icon_tex,
            "Settings".to_string(),
            &settings_btn_bb,
            0.9,
            theme_colors.btn_text,
        );
    }

    if s.ui.show_settings {
        s.renderer.send_command(DrawQuad {
            color: theme_colors.modal_backdrop,
            border_color: Color::TRANSPARENT,
            origin: Point::ZERO,
            z: 0.91,
            size: Size::new(win_w, win_h),
            border_radius: 20.0,
            border_thickness: 0.0,
        });

        let dialog_w = 280.0;
        let dialog_h = 240.0;
        let dialog_x = (win_w - dialog_w) / 2.0;
        let dialog_y = (win_h - dialog_h) / 2.0;

        s.renderer.send_command(DrawQuad {
            color: theme_colors.modal_bg,
            border_color: theme_colors.modal_border,
            origin: Point::new(dialog_x, dialog_y),
            z: 0.92,
            size: Size::new(dialog_w, dialog_h),
            border_radius: 16.0,
            border_thickness: 1.5,
        });

        layout!(
            {
                available_width: dialog_w,
                available_height: dialog_h,
                direction: column,
                gap: 12.0,
                padding_top: 16.0,
                padding_left: 30.0,
                padding_right: 30.0,

                layout!({ height: 24.0 }, {
                    let bb = Rect::xywh(dialog_x + x, dialog_y + y, width, height);
                    draw_centered_text(
                        &mut s.renderer, &atlas::UI_FONT_MONO_16, icon_tex,
                        "Settings".to_string(), &bb, 0.93, theme_colors.text_primary,
                    );
                }),

                layout!({ height: 32.0 }, {
                    let bb = Rect::xywh(dialog_x + x, dialog_y + y, width, height);
                    hit_boxes.format_toggle_btn = bb;
                    let format_label = if s.ui.format_24h { "Format: 24-Hour" } else { "Format: 12-Hour" };
                    s.renderer.send_command(DrawQuad {
                        color:            theme_colors.btn_bg,
                        border_color:     theme_colors.btn_border,
                        origin:           Point::new(bb.x(), bb.y()),
                        z:                0.93,
                        size:             Size::new(bb.width(), bb.height()),
                        border_radius:    8.0,
                        border_thickness: 1.0,
                    });
                    draw_centered_text(
                        &mut s.renderer, &atlas::UI_FONT_MONO_14, icon_tex,
                        format_label.to_string(), &bb, 0.95, theme_colors.btn_text,
                    );
                }),

                layout!({ height: 32.0 }, {
                    let bb = Rect::xywh(dialog_x + x, dialog_y + y, width, height);
                    hit_boxes.seconds_toggle_btn = bb;
                    let seconds_label = if s.ui.show_seconds { "Seconds: Show" } else { "Seconds: Hide" };
                    s.renderer.send_command(DrawQuad {
                        color:            theme_colors.btn_bg,
                        border_color:     theme_colors.btn_border,
                        origin:           Point::new(bb.x(), bb.y()),
                        z:                0.93,
                        size:             Size::new(bb.width(), bb.height()),
                        border_radius:    8.0,
                        border_thickness: 1.0,
                    });
                    draw_centered_text(
                        &mut s.renderer, &atlas::UI_FONT_MONO_14, icon_tex,
                        seconds_label.to_string(), &bb, 0.95, theme_colors.btn_text,
                    );
                }),

                layout!({ height: 32.0 }, {
                    let bb = Rect::xywh(dialog_x + x, dialog_y + y, width, height);
                    hit_boxes.theme_toggle_btn = bb;
                    s.renderer.send_command(DrawQuad {
                        color:            theme_colors.btn_bg,
                        border_color:     theme_colors.btn_border,
                        origin:           Point::new(bb.x(), bb.y()),
                        z:                0.93,
                        size:             Size::new(bb.width(), bb.height()),
                        border_radius:    8.0,
                        border_thickness: 1.0,
                    });
                    draw_centered_text(
                        &mut s.renderer, &atlas::UI_FONT_MONO_14, icon_tex,
                        s.ui.theme.to_string(), &bb, 0.95, theme_colors.btn_text,
                    );
                }),

                layout!({ height: 32.0 }, {
                    let btn_w = 100.0;
                    let bb = Rect::xywh(
                        dialog_x + x + (width - btn_w) / 2.0,
                        dialog_y + y,
                        btn_w,
                        height,
                    );
                    hit_boxes.done_btn = bb;
                    s.renderer.send_command(DrawQuad {
                        color:            theme_colors.done_bg,
                        border_color:     theme_colors.done_border,
                        origin:           Point::new(bb.x(), bb.y()),
                        z:                0.93,
                        size:             Size::new(bb.width(), bb.height()),
                        border_radius:    8.0,
                        border_thickness: 1.0,
                    });
                    draw_centered_text(
                        &mut s.renderer, &atlas::UI_FONT_MONO_14, icon_tex,
                        "Done".to_string(), &bb, 0.95, Color::WHITE,
                    );
                }),
            },
            {}
        );
    }
}

pub fn get_local_time(format_24h: bool, show_seconds: bool) -> (String, String) {
    let now = Local::now();
    let time_fmt = match (format_24h, show_seconds) {
        (true, true) => "%H:%M:%S",
        (true, false) => "%H:%M",
        (false, true) => "%I:%M:%S %p",
        (false, false) => "%I:%M %p",
    };
    let time = now.format(time_fmt).to_string();
    let date = now.format("%A, %b %d, %Y").to_string();
    (time, date)
}
