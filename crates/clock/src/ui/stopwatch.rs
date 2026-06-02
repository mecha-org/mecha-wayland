use utils::{Color, Rect};

use crate::{
    atlas,
    state::{self, HitBoxes},
    ui::components::draw_action_button,
};
use std::time::Duration;

pub fn render(
    s: &mut state::AppState,
    body_w: f32,
    body_h: f32,
    _win_w: f32,
    _win_h: f32,
    icon_tex: renderer::TextureId,
    hit_boxes: &mut HitBoxes,
) {
    use crate::ui::components::draw_centered_text;
    use layout::layout;

    let theme_colors = s.ui.theme.colors();

    let elapsed = s.ui.stopwatch.accumulated_duration
        + s.ui.stopwatch
            .start_instant
            .map(|i| i.elapsed())
            .unwrap_or(Duration::ZERO);
    let stopwatch_str = format_duration(elapsed);
    let is_running = s.ui.stopwatch.is_running;
    let laps = &s.ui.stopwatch.laps;

    layout!(
        {
            available_width: body_w,
            available_height: body_h,
            direction: column,
            gap: 18.0,
            padding_top: 55.0,

            layout!({ height: 60.0 },
                {
                    let bb = Rect::xywh(x, y, width, height);
                    draw_centered_text(&mut s.renderer, &atlas::UI_FONT_MONO_48, icon_tex, stopwatch_str, &bb, 0.9, theme_colors.text_primary);
                }
            ),

            layout!(
                {
                    direction: row,
                    height: 45.0,
                    padding_left: 35.0,
                    padding_right: 35.0,
                    justify: space_between,

                    layout!({ width: 120.0 }, {
                        let bb = Rect::xywh(x, y, width, height);
                        hit_boxes.lap_reset_btn = bb;
                        let label = if is_running { "Lap" } else { "Reset" };
                        draw_action_button(
                            &mut s.renderer, icon_tex, label.to_string(),
                            bb, 0.5,
                            theme_colors.btn_bg, theme_colors.btn_border, theme_colors.btn_text,
                        );
                    }),

                    layout!({ width: 120.0 }, {
                        let bb = Rect::xywh(x, y, width, height);
                        hit_boxes.start_stop_btn = bb;
                        let (bg_color, border_color, label) = if is_running {
                            (theme_colors.stopwatch_danger_bg, theme_colors.stopwatch_danger_border, "Stop")
                        } else {
                            (theme_colors.stopwatch_success_bg, theme_colors.stopwatch_success_border, "Start")
                        };
                        draw_action_button(
                            &mut s.renderer, icon_tex, label.to_string(),
                            bb, 0.5, bg_color, border_color, Color::WHITE,
                        );
                    }),
                },
                {}
            ),

            layout!(
                {
                    direction: column,
                    height: 110.0,
                    padding_top: 8.0,
                    padding_left: 30.0,
                    padding_right: 30.0,
                    gap: 6.0,

                    layout!({ height: 20.0 }, {
                        let bb = Rect::xywh(x, y, width, height);
                        if let Some(lap_str) = get_lap_label(laps, 0) {
                            draw_centered_text(&mut s.renderer, &atlas::UI_FONT_MONO_14, icon_tex, lap_str, &bb, 0.9, theme_colors.text_primary);
                        }
                    }),
                    layout!({ height: 20.0 }, {
                        let bb = Rect::xywh(x, y, width, height);
                        if let Some(lap_str) = get_lap_label(laps, 1) {
                            draw_centered_text(&mut s.renderer, &atlas::UI_FONT_MONO_14, icon_tex, lap_str, &bb, 0.9, theme_colors.text_secondary);
                        }
                    }),
                    layout!({ height: 20.0 }, {
                        let bb = Rect::xywh(x, y, width, height);
                        if let Some(lap_str) = get_lap_label(laps, 2) {
                            draw_centered_text(&mut s.renderer, &atlas::UI_FONT_MONO_14, icon_tex, lap_str, &bb, 0.9, theme_colors.text_muted);
                        }
                    }),
                },
                {}
            ),
        },
        {}
    );
}

pub fn format_duration(dur: Duration) -> String {
    let ms = dur.as_millis();
    let hundredths = (ms % 1000) / 10;
    let total_secs = ms / 1000;
    let secs = total_secs % 60;
    let mins = total_secs / 60;
    let hours = mins / 60;
    if hours > 0 {
        format!("{:02}:{:02}:{:02}.{:02}", hours, mins % 60, secs, hundredths)
    } else {
        format!("{:02}:{:02}.{:02}", mins, secs, hundredths)
    }
}

pub fn get_lap_label(laps: &[Duration], display_index: usize) -> Option<String> {
    if laps.is_empty() || display_index >= laps.len() {
        return None;
    }
    let lap_idx = laps.len() - 1 - display_index;
    Some(format!("Lap {}: {}", lap_idx + 1, format_duration(laps[lap_idx])))
}
