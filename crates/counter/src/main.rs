#![recursion_limit = "4096"]

mod atlas {
    include!(concat!(env!("OUT_DIR"), "/ui_gen.rs"));
}
mod renderer;

use app::prelude::*;
use std::{os::fd::AsRawFd, time::Duration};

use ::renderer::commands::{Color, Point};
use io_ring::Ring;
use layout::layout;
use timer::{Timer, TimerSettings};
use wayland::Wayland;

// ARGB8888 little-endian fourcc
const DRM_FORMAT_ARGB8888: u32 = 0x34325241;

#[derive(Default, Clone, Copy, Debug)]
struct BoundingBox {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl BoundingBox {
    fn contains(&self, px: f64, py: f64) -> bool {
        let px = px as f32;
        let py = py as f32;
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

#[derive(Default, Clone, Copy, Debug)]
struct HitBoxes {
    minus: BoundingBox,
    plus: BoundingBox,
}

#[derive(Default)]
struct Counter {
    count: i32,
}

struct UiState {
    counter: Counter,
    surface_id: u32,
    surface_size: (i32, i32),
    dmabuf: [Option<::renderer::RenderableSurface<::renderer::DmaBuf>>; 2],
    wl_buf_ids: [u32; 2],
    buf_in_flight: [bool; 2],
    icon_tex: Option<::renderer::TextureId>,
    cursor_x: f64,
    cursor_y: f64,
    hit_boxes: HitBoxes,
}

impl UiState {
    fn new() -> Self {
        Self {
            counter: Counter::default(),
            surface_id: 0,
            surface_size: (0, 0),
            dmabuf: [None, None],
            wl_buf_ids: [0, 0],
            buf_in_flight: [false, false],
            icon_tex: None,
            cursor_x: 0.0,
            cursor_y: 0.0,
            hit_boxes: HitBoxes::default(),
        }
    }
}

#[derive(State)]
struct AppState {
    ring: Ring,
    timer: Timer,
    wayland: Wayland,
    renderer: ::renderer::Renderer,
    ui: UiState,
}

impl AppState {
    fn new() -> Self {
        let ring = Ring::default();
        let timer = Timer::new(ring.get_proxy());
        let wayland = Wayland::new(ring.get_proxy()).expect("failed to create wayland connection");
        let renderer = ::renderer::Renderer::new().expect("failed to create renderer");

        Self {
            ring,
            timer,
            wayland,
            renderer,
            ui: UiState::new(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum CounterEvent {
    Updated { new_count: i32 },
}

impl app::Event for CounterEvent {}

fn main() {
    let state = AppState::new();

    let mut app = app::App::new(state)
        .mount(io_ring::module())
        .mount(timer::module())
        .mount(renderer::module())
        .mount(wayland::module())
        .mount(
            app::Module::new().on(|s: &mut AppState, _: &wayland::Initilised| {
                use wayland::zwlr_layer_shell::{KeyboardInteractivity, Layer};

                let surface_id = s.wayland.compositor.create_surface();
                s.wayland.surface.register(surface_id);
                s.ui.surface_id = surface_id;

                let layer_surface_id =
                    s.wayland
                        .layer_shell
                        .get_layer_surface(surface_id, 0, Layer::Top, "counter");
                s.wayland.layer_surface.register(layer_surface_id);
                s.wayland.layer_surface.set_size(layer_surface_id, 400, 360);
                s.wayland
                    .layer_surface
                    .set_keyboard_interactivity(layer_surface_id, KeyboardInteractivity::OnDemand);

                s.wayland.surface.commit(surface_id);
                s.wayland.flush();
            }),
        )
        .mount(
            app::Module::new().on(
                |s: &mut AppState, ev: &wayland::zwlr_layer_shell::LayerSurfaceEvent| {
                    use wayland::zwlr_layer_shell::LayerSurfaceEvent;

                    let LayerSurfaceEvent::Configured {
                        id,
                        serial,
                        width,
                        height,
                    } = ev
                    else {
                        return;
                    };

                    let w = if *width == 0 { 256i32 } else { *width as i32 };
                    let h = if *height == 0 { 256i32 } else { *height as i32 };
                    s.ui.surface_size = (w, h);

                    let surface0 = s
                        .renderer
                        .create_surface::<::renderer::DmaBuf>(w as u32, h as u32)
                        .expect("dmabuf surface 0");
                    let surface1 = s
                        .renderer
                        .create_surface::<::renderer::DmaBuf>(w as u32, h as u32)
                        .expect("dmabuf surface 1");

                    let buf_id0 = create_wl_buffer(&mut s.wayland, &surface0, w, h);
                    let buf_id1 = create_wl_buffer(&mut s.wayland, &surface1, w, h);
                    s.wayland.wl_buffer.register(buf_id0);
                    s.wayland.wl_buffer.register(buf_id1);
                    s.ui.wl_buf_ids = [buf_id0, buf_id1];

                    if s.ui.icon_tex.is_none() {
                        s.ui.icon_tex = s.renderer.upload_atlas(atlas::UI.png_bytes).ok();
                    }

                    s.renderer.active_surface(&surface0);
                    s.ui.hit_boxes = render_counter_ui(
                        &mut s.renderer,
                        w as f32,
                        h as f32,
                        s.ui.counter.count,
                        s.ui.icon_tex.unwrap(),
                    );
                    s.renderer.finish();

                    s.ui.dmabuf = [Some(surface0), Some(surface1)];
                    s.ui.buf_in_flight = [true, false];

                    s.wayland.layer_surface.ack_configure(*id, *serial);
                    s.wayland.surface.attach(s.ui.surface_id, buf_id0, 0, 0);
                    s.wayland.surface.damage(s.ui.surface_id, 0, 0, w, h);

                    let cb_id = s.wayland.surface.frame(s.ui.surface_id);
                    s.wayland.callback.register_frame(cb_id);

                    s.wayland.surface.commit(s.ui.surface_id);
                    s.wayland.flush();
                },
            ),
        )
        .mount(
            app::Module::new().on(|s: &mut AppState, _: &wayland::WlCallbackEvent| {
                let free_idx = if !s.ui.buf_in_flight[0] {
                    0
                } else if !s.ui.buf_in_flight[1] {
                    1
                } else {
                    return;
                };

                let surface = s.ui.dmabuf[free_idx].as_ref().unwrap();
                s.renderer.active_surface(surface);
                if let Some(icon_tex) = s.ui.icon_tex {
                    let (w, h) = s.ui.surface_size;
                    s.ui.hit_boxes = render_counter_ui(
                        &mut s.renderer,
                        w as f32,
                        h as f32,
                        s.ui.counter.count,
                        icon_tex,
                    );
                    s.renderer.finish();
                }

                let (w, h) = s.ui.surface_size;
                s.wayland
                    .surface
                    .attach(s.ui.surface_id, s.ui.wl_buf_ids[free_idx], 0, 0);
                s.wayland.surface.damage(s.ui.surface_id, 0, 0, w, h);

                let cb_id = s.wayland.surface.frame(s.ui.surface_id);
                s.wayland.callback.register_frame(cb_id);

                s.wayland.surface.commit(s.ui.surface_id);
                s.ui.buf_in_flight[free_idx] = true;
                s.wayland.flush();
            }),
        )
        .mount(
            app::Module::new().on(|s: &mut AppState, ev: &wayland::WlBufferEvent| {
                let wayland::WlBufferEvent::Release { id } = ev;
                for i in 0..2 {
                    if s.ui.wl_buf_ids[i] == *id {
                        s.ui.buf_in_flight[i] = false;
                        break;
                    }
                }
            }),
        )
        .mount(
            app::Module::new().on(|_: &mut AppState, ev: &wayland::KeyboardEvent| {
                if let wayland::KeyboardEvent::Key { key, state, .. } = ev {
                    if (*key == 1 || *key == 16) && *state == wayland::KeyState::Pressed {
                        std::process::exit(0);
                    }
                }
            }),
        )
        .mount(
            app::Module::new().on(|s: &mut AppState, ev: &wayland::PointerEvent| match ev {
                wayland::PointerEvent::Motion {
                    surface_x,
                    surface_y,
                    ..
                } => {
                    s.ui.cursor_x = *surface_x;
                    s.ui.cursor_y = *surface_y;
                }
                wayland::PointerEvent::Button {
                    button: _, state, ..
                } if *state == wayland::ButtonState::Pressed => {
                    if let Some(delta) = hit_button(&s.ui.hit_boxes, s.ui.cursor_x, s.ui.cursor_y) {
                        s.ui.counter.count += delta;
                        redraw(s);
                    }
                }
                _ => {}
            }),
        )
        .mount(
            app::Module::new().on(|s: &mut AppState, ev: &wayland::TouchEvent| {
                if let wayland::TouchEvent::Down { x, y, .. } = ev {
                    if let Some(delta) = hit_button(&s.ui.hit_boxes, *x, *y) {
                        s.ui.counter.count += delta;
                        redraw(s);
                    }
                }
            }),
        )
        .mount(
            app::Module::new().on(|s: &mut AppState, _: &app::Start| {
                s.timer.start_timer(TimerSettings {
                    duration: Duration::from_secs(2),
                    repeat: true,
                });
            }),
        )
        .mount(
            app::Module::new().on(|_: &mut AppState, _: &timer::TimerEvent| {}),
        );

    app.dispatch(&app::Start);
    loop {
        app.dispatch(&app::PrePoll);
        app.dispatch(&app::Poll);
    }
}

fn hit_button(hit_boxes: &HitBoxes, x: f64, y: f64) -> Option<i32> {
    if hit_boxes.minus.contains(x, y) {
        return Some(-1);
    }
    if hit_boxes.plus.contains(x, y) {
        return Some(1);
    }
    None
}

fn redraw(s: &mut AppState) {
    let free_idx = if !s.ui.buf_in_flight[0] {
        0
    } else if !s.ui.buf_in_flight[1] {
        1
    } else {
        return;
    };

    let surface = s.ui.dmabuf[free_idx].as_ref().unwrap();
    s.renderer.active_surface(surface);
    if let Some(icon_tex) = s.ui.icon_tex {
        let (w, h) = s.ui.surface_size;
        s.ui.hit_boxes = render_counter_ui(
            &mut s.renderer,
            w as f32,
            h as f32,
            s.ui.counter.count,
            icon_tex,
        );
        s.renderer.finish();
    }

    let (w, h) = s.ui.surface_size;
    s.wayland
        .surface
        .attach(s.ui.surface_id, s.ui.wl_buf_ids[free_idx], 0, 0);
    s.wayland.surface.damage(s.ui.surface_id, 0, 0, w, h);

    let cb_id = s.wayland.surface.frame(s.ui.surface_id);
    s.wayland.callback.register_frame(cb_id);

    s.wayland.surface.commit(s.ui.surface_id);
    s.ui.buf_in_flight[free_idx] = true;
    s.wayland.flush();
}

fn draw_centered_text(
    renderer: &mut ::renderer::Renderer,
    font: &'static assets::BakedFont,
    texture_id: ::renderer::TextureId,
    text: &str,
    box_bounds: &BoundingBox,
    z_index: f32,
) {
    let text_w = font.measure_width(text);
    let center_x = box_bounds.x + (box_bounds.w - text_w) / 2.0;
    let center_y = box_bounds.y + font.get_baseline_offset(box_bounds.h);

    renderer.send_command(::renderer::commands::DrawText {
        font,
        texture_id,
        text: text.to_string(),
        origin: Point::new(center_x, center_y),
        z: z_index,
        color: Color::rgb(1.0, 1.0, 1.0),
    });
}

fn render_counter_ui(
    renderer: &mut ::renderer::Renderer,
    win_w: f32,
    win_h: f32,
    count: i32,
    icon_tex: ::renderer::TextureId,
) -> HitBoxes {
    use ::renderer::commands::*;

    let mut hit_boxes = HitBoxes::default();
    let count_str = format!("{count}");

    renderer.send_command(ClearColor::rgb(0.32, 0.32, 0.32));

    layout!(
        {
            available_width: win_w,
            available_height: win_h,
            direction: column,
            gap: 40.0,
            padding_top: 16.0,

            layout!({ height: 50.0 }, {
                let bb = BoundingBox { x, y, w: width, h: height };
                draw_centered_text(renderer, &atlas::UI_FONT_INTER_24, icon_tex, "Counter", &bb, 0.95);
            }),
            layout!({ height: 120.0 }, {
                let bb = BoundingBox { x, y, w: width, h: height };
                draw_centered_text(renderer, &atlas::UI_FONT_INTER_100, icon_tex, &count_str, &bb, 0.95);
            }),
            layout!({
                direction: row,
                height: 52.0,
                padding_left: 60.0,
                padding_right: 60.0,
                justify: space_between,

                layout!({ width: 110.0, height: 52.0 }, {
                    let bb = BoundingBox { x, y, w: width, h: height };
                    renderer.send_command(DrawQuad {
                        color: Color::rgb(0.2, 0.4, 0.9),
                        border_color: Color::rgb(0.4, 0.6, 1.0),
                        origin: Point::new(bb.x, bb.y),
                        z: 1.0,
                        size: Size::new(bb.w, bb.h),
                        border_radius: 12.0,
                        border_thickness: 2.0,
                    });
                    draw_centered_text(renderer, &atlas::UI_FONT_INTER_24, icon_tex, "-", &bb, 0.4);
                    hit_boxes.minus = bb;
                }),
                layout!({ width: 110.0, height: 52.0 }, {
                    let bb = BoundingBox { x, y, w: width, h: height };
                    renderer.send_command(DrawQuad {
                        color: Color::rgb(0.2, 0.7, 0.3),
                        border_color: Color::rgb(0.4, 0.9, 0.5),
                        origin: Point::new(bb.x, bb.y),
                        z: 1.0,
                        size: Size::new(bb.w, bb.h),
                        border_radius: 12.0,
                        border_thickness: 2.0,
                    });
                    draw_centered_text(renderer, &atlas::UI_FONT_INTER_24, icon_tex, "+", &bb, 0.5);
                    hit_boxes.plus = bb;
                }),
            }, {
            }),
        },
        {
            renderer.send_command(DrawQuad {
                color: Color::rgb(0.16, 0.16, 0.18),
                border_color: Color::rgb(0.30, 0.30, 0.35),
                origin: Point::new(x, y),
                z: 0.0,
                size: Size::new(width, height),
                border_radius: 20.0,
                border_thickness: 2.0,
            });
        }
    );

    renderer.process_command_queue::<ClearColor>();
    renderer.process_command_queue::<DrawRect>();
    renderer.process_command_queue::<DrawQuad>();
    renderer.process_command_queue::<DrawMonochromeSprite>();
    renderer.process_command_queue::<DrawText>();

    hit_boxes
}

fn create_wl_buffer(
    wayland: &mut Wayland,
    surface: &::renderer::RenderableSurface<::renderer::DmaBuf>,
    width: i32,
    height: i32,
) -> u32 {
    let modifier = surface.backend.modifier;
    let modifier_hi = (modifier >> 32) as u32;
    let modifier_lo = (modifier & 0xffff_ffff) as u32;
    let fd = unsafe { libc::dup(surface.backend.prime_fd.as_raw_fd()) };

    let params_id = wayland.dmabuf.create_params();
    wayland.buf_params.register(params_id);
    wayland.buf_params.add(
        params_id,
        fd,
        0,
        0,
        surface.backend.stride,
        modifier_hi,
        modifier_lo,
    );
    let buf_id = wayland
        .buf_params
        .create_immed(params_id, width, height, DRM_FORMAT_ARGB8888, 0);
    wayland.buf_params.destroy(params_id);
    buf_id
}
