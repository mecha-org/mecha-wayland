use utils::Rect;

use crate::state::{ActiveTab, AppState, HitBoxes};

mod clock;
pub mod components;
mod stopwatch;
pub mod theme;

pub fn handle_click(s: &mut AppState, x: f64, y: f64) {
    if s.show_settings {
        if s.hit_boxes.format_toggle_btn.contains(x, y) {
            s.format_24h = !s.format_24h;
            redraw(s);
            return;
        }
        if s.hit_boxes.seconds_toggle_btn.contains(x, y) {
            s.show_seconds = !s.show_seconds;
            redraw(s);
            return;
        }
        if s.hit_boxes.theme_toggle_btn.contains(x, y) {
            s.theme = s.theme.next();
            redraw(s);
            return;
        }
        if s.hit_boxes.done_btn.contains(x, y) {
            s.show_settings = false;
            redraw(s);
            return;
        }
        // Click outside the dialog can also close it
        let win_w = s.engine.surface_size.0 as f32;
        let win_h = s.engine.surface_size.1 as f32;
        let dialog_bb = Rect::xywh((win_w - 280.0) / 2.0, (win_h - 240.0) / 2.0, 280.0, 240.0);
        if !dialog_bb.contains(x, y) {
            s.show_settings = false;
            redraw(s);
        }
        return;
    }

    if s.hit_boxes.settings_btn.contains(x, y) {
        s.show_settings = true;
        redraw(s);
        return;
    }

    if s.hit_boxes.clock_tab.contains(x, y) {
        s.active_tab = ActiveTab::Clock;
        redraw(s);
        return;
    }
    if s.hit_boxes.stopwatch_tab.contains(x, y) {
        s.active_tab = ActiveTab::Stopwatch;
        redraw(s);
        return;
    }

    if s.active_tab == ActiveTab::Stopwatch {
        if s.hit_boxes.start_stop_btn.contains(x, y) {
            s.stopwatch.toggle();
            redraw(s);
        } else if s.hit_boxes.lap_reset_btn.contains(x, y) {
            s.stopwatch.lap_or_reset();
            redraw(s);
        }
    }
}

pub fn redraw(s: &mut AppState) {
    let free_idx = if !s.engine.buf_in_flight[0] {
        0
    } else if !s.engine.buf_in_flight[1] {
        1
    } else {
        return;
    };

    let surface = s.engine.dmabuf[free_idx].as_ref().unwrap();
    s.engine.renderer.active_surface(surface);

    let (w, h) = s.engine.surface_size;
    s.hit_boxes = render_app_ui(s, w as f32, h as f32);

    s.engine.renderer.finish();

    s.engine
        .wayland
        .surface
        .attach(s.engine.surface_id, s.engine.wl_buf_ids[free_idx], 0, 0);
    s.engine
        .wayland
        .surface
        .damage(s.engine.surface_id, 0, 0, w, h);

    let cb_id = s.engine.wayland.surface.frame(s.engine.surface_id);
    s.engine.wayland.callback.register_frame(cb_id);

    s.engine.wayland.surface.commit(s.engine.surface_id);
    s.engine.buf_in_flight[free_idx] = true;
    s.engine.wayland.flush();
}

pub fn render_app_ui(s: &mut AppState, win_w: f32, win_h: f32) -> HitBoxes {
    let mut hit_boxes = HitBoxes::default();
    let icon_tex = s.engine.icon_tex.expect("Atlas texture missing");
    let theme_colors = s.theme.colors();

    use layout::layout;
    use renderer::commands::*;

    // Clear to the app background colour
    s.engine
        .renderer
        .send_command(ClearColor(theme_colors.app_bg));

    // App Shell
    layout!(
        {
            available_width: win_w,
            available_height: win_h,
            direction: column,
            padding_top: 10,
            padding_bottom: 10,

            // Dynamic Body
            layout!(
                { height: win_h - 50.0 - 20.0 - 2.0 }, // Total Window - Navbar - Padding - Border
                {
                    match s.active_tab {
                        crate::state::ActiveTab::Clock => {
                            crate::ui::clock::render(
                                s, width, height, win_w, win_h, icon_tex, &mut hit_boxes
                            );
                        }
                        crate::state::ActiveTab::Stopwatch => {
                            crate::ui::stopwatch::render(
                                s, width, height, win_w, win_h, icon_tex, &mut hit_boxes
                            );
                        }
                    }
                }
            ),

            // "Fixed" Bottom Navbar
            layout!(
                {
                    direction: row,
                    height: 50.0,
                    padding_left: 12.0,
                    padding_right: 12.0,
                    justify: space_between,

                    layout!({ width: 180 }, {
                        let bb = Rect::xywh(x, y, width, height);
                        hit_boxes.clock_tab = bb;
                        crate::ui::components::draw_tab_button(
                            &mut s.engine.renderer, icon_tex, "Clock".to_string(), s.active_tab == ActiveTab::Clock, bb, theme_colors
                        );
                    }),

                    layout!({ width: 180 }, {
                        let bb = Rect::xywh(x, y, width, height);
                        hit_boxes.stopwatch_tab = bb;
                        crate::ui::components::draw_tab_button(
                            &mut s.engine.renderer, icon_tex, "Stopwatch".to_string(), s.active_tab == ActiveTab::Stopwatch, bb, theme_colors
                        );
                    }),
                },
                {
                }
            ),
        },
        {
            // App Background
            s.engine.renderer.send_command(DrawQuad {
                color:            theme_colors.app_bg,
                border_color:     theme_colors.app_border,
                origin:           Point::new(x, y),
                z:                0.0,
                size:             Size::new(width, height),
                border_radius:    20.0,
                border_thickness: 2.0,
            });
        }
    );

    s.engine.renderer.process_command_queue::<ClearColor>();
    // s.engine.renderer.process_command_queue::<DrawRect>();
    s.engine.renderer.process_command_queue::<DrawQuad>();
    s.engine
        .renderer
        .process_command_queue::<DrawMonochromeSprite>();
    s.engine.renderer.process_command_queue::<DrawText>();

    hit_boxes
}
