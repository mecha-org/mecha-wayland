#![recursion_limit = "4096"]

mod atlas {
    include!(concat!(env!("OUT_DIR"), "/ui_gen.rs"));
}
mod renderer;

use std::{os::fd::AsRawFd, time::Duration};

use app::{App, Poll, Start, event::Event};
use io_ring::{Ring, register_ring};
use taffy::prelude::*;
use timer::{Timer, TimerEvent, TimerSettings, register_timer};
use wayland::{Wayland, register_wayland};

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
struct UiLayout {
    card: BoundingBox,
    title: BoundingBox,
    count: BoundingBox,
    minus: BoundingBox,
    plus: BoundingBox,
}

struct AppState {
    ring: Ring,
    timer: Timer,
    counter: Counter,
    wayland: Wayland,
    renderer: ::renderer::Renderer,
    surface_id: u32,
    surface_size: (i32, i32),
    dmabuf: [Option<::renderer::RenderableSurface<::renderer::DmaBuf>>; 2],
    wl_buf_ids: [u32; 2],
    buf_in_flight: [bool; 2],
    icon_tex: Option<::renderer::TextureId>,
    cursor_x: f64,
    cursor_y: f64,
    ui_layout: UiLayout,
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
            counter: Counter::default(),
            wayland,
            renderer,
            surface_id: 0,
            surface_size: (0, 0),
            dmabuf: [None, None],
            wl_buf_ids: [0, 0],
            buf_in_flight: [false, false],
            icon_tex: None,
            cursor_x: 0.0,
            cursor_y: 0.0,
            ui_layout: UiLayout::default(),
        }
    }
}

#[derive(Default)]
struct Counter {
    count: i32,
}

#[derive(Clone, Copy, Debug)]
pub enum CounterEvent {
    Updated { new_count: i32 },
}

impl Event for CounterEvent {}

fn main() {
    let state = AppState::new();

    let mut app = app::App::new(state)
        .register_module(|s| &mut s.ring, register_ring!())
        .register_module(|s| &mut s.timer, register_timer!())
        .register_module(|s| &mut s.renderer, register_renderer!())
        .register_module(|s| &mut s.wayland, register_wayland!())
        .register_module(
            |s| s,
            app::module::Module::new().on(|s: &mut AppState, _: &wayland::Initilised| {
                use wayland::zwlr_layer_shell::{KeyboardInteractivity, Layer};

                let surface_id = s.wayland.compositor.create_surface();
                s.wayland.surface.register(surface_id);
                s.surface_id = surface_id;

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
        .register_module(
            |s| s,
            app::module::Module::new().on(
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
                    s.surface_size = (w, h);

                    // Compute Taffy layout on Configure
                    s.ui_layout = compute_ui_layout(w as f32, h as f32);

                    // Allocate double-buffered dmabuf surfaces.
                    let surface0 = s
                        .renderer
                        .create_surface::<::renderer::DmaBuf>(w as u32, h as u32)
                        .expect("dmabuf surface 0");
                    let surface1 = s
                        .renderer
                        .create_surface::<::renderer::DmaBuf>(w as u32, h as u32)
                        .expect("dmabuf surface 1");

                    // Create wl_buffer for each surface via zwp_linux_dmabuf_v1.
                    let buf_id0 = create_wl_buffer(&mut s.wayland, &surface0, w, h);
                    let buf_id1 = create_wl_buffer(&mut s.wayland, &surface1, w, h);
                    s.wayland.wl_buffer.register(buf_id0);
                    s.wayland.wl_buffer.register(buf_id1);
                    s.wl_buf_ids = [buf_id0, buf_id1];

                    // Upload the atlas texture once on first configure.
                    if s.icon_tex.is_none() {
                        s.icon_tex = s.renderer.upload_atlas(atlas::UI.png_bytes).ok();
                    }

                    // Render the counter UI into surface 0 for the first frame.
                    s.renderer.active_surface(&surface0);
                    render_counter_ui(
                        &mut s.renderer,
                        s.counter.count,
                        s.icon_tex.unwrap(),
                        &s.ui_layout,
                    );
                    s.renderer.finish();

                    s.dmabuf = [Some(surface0), Some(surface1)];
                    s.buf_in_flight = [true, false];

                    s.wayland.layer_surface.ack_configure(*id, *serial);
                    s.wayland.surface.attach(s.surface_id, buf_id0, 0, 0);
                    s.wayland.surface.damage(s.surface_id, 0, 0, w, h);

                    let cb_id = s.wayland.surface.frame(s.surface_id);
                    s.wayland.callback.register_frame(cb_id);

                    s.wayland.surface.commit(s.surface_id);
                    // flush via sendmsg — dmabuf fds are queued
                    s.wayland.flush();
                },
            ),
        )
        .register_module(
            |s| s,
            app::module::Module::new().on(|s: &mut AppState, _: &wayland::WlCallbackEvent| {
                // Find the free buffer (compositor released it).
                let free_idx = if !s.buf_in_flight[0] {
                    0
                } else if !s.buf_in_flight[1] {
                    1
                } else {
                    return;
                };

                let surface = s.dmabuf[free_idx].as_ref().unwrap();
                s.renderer.active_surface(surface);
                if let Some(icon_tex) = s.icon_tex {
                    render_counter_ui(&mut s.renderer, s.counter.count, icon_tex, &s.ui_layout);
                    s.renderer.finish();
                }

                let (w, h) = s.surface_size;
                s.wayland
                    .surface
                    .attach(s.surface_id, s.wl_buf_ids[free_idx], 0, 0);
                s.wayland.surface.damage(s.surface_id, 0, 0, w, h);

                let cb_id = s.wayland.surface.frame(s.surface_id);
                s.wayland.callback.register_frame(cb_id);

                s.wayland.surface.commit(s.surface_id);
                s.buf_in_flight[free_idx] = true;
                s.wayland.flush();
            }),
        )
        .register_module(
            |s| s,
            app::module::Module::new().on(|s: &mut AppState, ev: &wayland::WlBufferEvent| {
                let wayland::WlBufferEvent::Release { id } = ev;
                for i in 0..2 {
                    if s.wl_buf_ids[i] == *id {
                        s.buf_in_flight[i] = false;
                        break;
                    }
                }
            }),
        )
        .register_module(
            |s| s,
            app::module::Module::new().on(|_: &mut AppState, ev: &wayland::KeyboardEvent| {
                println!("[App] Keyboard Event: {:?}", ev);
                if let wayland::KeyboardEvent::Key { key, state, .. } = ev {
                    if (*key == 1 || *key == 16) && *state == wayland::KeyState::Pressed {
                        println!("[App] Exiting...");
                        std::process::exit(0);
                    }
                }
            }),
        )
        .register_module(
            |s| s,
            app::module::Module::new().on(
                |s: &mut AppState, ev: &wayland::PointerEvent| match ev {
                    wayland::PointerEvent::Motion {
                        surface_x,
                        surface_y,
                        ..
                    } => {
                        s.cursor_x = *surface_x;
                        s.cursor_y = *surface_y;
                    }
                    wayland::PointerEvent::Button {
                        button: _, state, ..
                    } if *state == wayland::ButtonState::Pressed => {
                        if let Some(delta) = hit_button(&s.ui_layout, s.cursor_x, s.cursor_y) {
                            s.counter.count += delta;
                            redraw(s);
                        }
                    }
                    _ => {}
                },
            ),
        )
        .register_module(
            |s| s,
            app::module::Module::new().on(|s: &mut AppState, ev: &wayland::TouchEvent| {
                if let wayland::TouchEvent::Down { x, y, .. } = ev {
                    if let Some(delta) = hit_button(&s.ui_layout, *x, *y) {
                        s.counter.count += delta;
                        redraw(s);
                    }
                }
            }),
        )
        .register_module(
            |s| s,
            app::module::Module::new().on(|s: &mut AppState, ev: &app::Start| {
                let id = s.timer.start_timer(TimerSettings {
                    duration: Duration::from_secs(2),
                    repeat: true,
                });
                println!("Timer Started with ID: {}", id.0);
            }),
        )
        .register_module(
            |s| s,
            app::module::Module::new().on(|s: &mut AppState, ev: &timer::TimerEvent| {
                println!("Timer Event: {:?}", ev);
            }),
        );

    app.run();
}

fn hit_button(layout: &UiLayout, x: f64, y: f64) -> Option<i32> {
    if layout.minus.contains(x, y) {
        return Some(-1);
    }
    if layout.plus.contains(x, y) {
        return Some(1);
    }
    None
}

fn redraw(s: &mut AppState) {
    let free_idx = if !s.buf_in_flight[0] {
        0
    } else if !s.buf_in_flight[1] {
        1
    } else {
        return;
    };

    let surface = s.dmabuf[free_idx].as_ref().unwrap();
    s.renderer.active_surface(surface);
    if let Some(icon_tex) = s.icon_tex {
        render_counter_ui(&mut s.renderer, s.counter.count, icon_tex, &s.ui_layout);
        s.renderer.finish();
    }

    let (w, h) = s.surface_size;
    s.wayland
        .surface
        .attach(s.surface_id, s.wl_buf_ids[free_idx], 0, 0);
    s.wayland.surface.damage(s.surface_id, 0, 0, w, h);

    let cb_id = s.wayland.surface.frame(s.surface_id);
    s.wayland.callback.register_frame(cb_id);

    s.wayland.surface.commit(s.surface_id);
    s.buf_in_flight[free_idx] = true;
    s.wayland.flush();
}

fn compute_ui_layout(width: f32, height: f32) -> UiLayout {
    let mut taffy = TaffyTree::<()>::new();

    let title = taffy
        .new_leaf(Style {
            size: Size {
                width: percent(1.0),
                height: length(50.0),
            },
            ..Default::default()
        })
        .unwrap();

    let count_node = taffy
        .new_leaf(Style {
            size: Size {
                width: percent(1.0),
                height: length(120.0),
            },
            ..Default::default()
        })
        .unwrap();

    let minus_button = taffy
        .new_leaf(Style {
            size: Size {
                width: length(110.0),
                height: length(52.0),
            },
            ..Default::default()
        })
        .unwrap();

    let plus_button = taffy
        .new_leaf(Style {
            size: Size {
                width: length(110.0),
                height: length(52.0),
            },
            ..Default::default()
        })
        .unwrap();

    let button_row = taffy
        .new_with_children(
            Style {
                display: Display::Flex,
                justify_content: Some(JustifyContent::SpaceBetween),
                align_items: Some(AlignItems::Center),
                size: Size {
                    width: percent(1.0),
                    height: length(52.0),
                },
                padding: Rect {
                    left: length(60.0),
                    right: length(60.0),
                    ..Rect::zero()
                },
                ..Default::default()
            },
            &[minus_button, plus_button],
        )
        .unwrap();

    let card = taffy
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: Some(AlignItems::Center),
                size: Size {
                    width: length(width),
                    height: length(height),
                },
                padding: Rect {
                    top: length(16.0),
                    ..Rect::zero()
                },
                gap: Size {
                    width: zero(),
                    height: length(40.0),
                },
                ..Default::default()
            },
            &[title, count_node, button_row],
        )
        .unwrap();

    taffy
        .compute_layout(
            card,
            Size {
                width: AvailableSpace::Definite(width),
                height: AvailableSpace::Definite(height),
            },
        )
        .unwrap();

    let card_layout = taffy.layout(card).unwrap();
    let title_layout = taffy.layout(title).unwrap();
    let count_layout = taffy.layout(count_node).unwrap();
    let row_layout = taffy.layout(button_row).unwrap();
    let minus_layout = taffy.layout(minus_button).unwrap();
    let plus_layout = taffy.layout(plus_button).unwrap();

    UiLayout {
        card: BoundingBox {
            x: card_layout.location.x,
            y: card_layout.location.y,
            w: card_layout.size.width,
            h: card_layout.size.height,
        },
        title: BoundingBox {
            x: card_layout.location.x + title_layout.location.x,
            y: card_layout.location.y + title_layout.location.y,
            w: title_layout.size.width,
            h: title_layout.size.height,
        },
        count: BoundingBox {
            x: card_layout.location.x + count_layout.location.x,
            y: card_layout.location.y + count_layout.location.y,
            w: count_layout.size.width,
            h: count_layout.size.height,
        },
        minus: BoundingBox {
            x: card_layout.location.x + row_layout.location.x + minus_layout.location.x,
            y: card_layout.location.y + row_layout.location.y + minus_layout.location.y,
            w: minus_layout.size.width,
            h: minus_layout.size.height,
        },
        plus: BoundingBox {
            x: card_layout.location.x + row_layout.location.x + plus_layout.location.x,
            y: card_layout.location.y + row_layout.location.y + plus_layout.location.y,
            w: plus_layout.size.width,
            h: plus_layout.size.height,
        },
    }
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
        origin: (center_x, center_y, z_index),
        color: (1.0, 1.0, 1.0, 1.0),
    });
}

fn render_counter_ui(
    renderer: &mut ::renderer::Renderer,
    count: i32,
    icon_tex: ::renderer::TextureId,
    layout: &UiLayout,
) {
    use ::renderer::commands::*;

    renderer.send_command(ClearColor {
        r: 0.32,
        g: 0.32,
        b: 0.32,
        a: 1.0,
    });

    // Card background
    renderer.send_command(DrawQuad {
        color: (0.16, 0.16, 0.18, 1.0),
        border_color: (0.30, 0.30, 0.35, 1.0),
        origin: (layout.card.x, layout.card.y, 0.0),
        size: (layout.card.w, layout.card.h),
        border_radius: 20.0,
        border_thickness: 2.0,
    });

    // Title
    draw_centered_text(
        renderer,
        &atlas::UI_FONT_INTER_24,
        icon_tex,
        "Counter",
        &layout.title,
        0.5,
    );

    // Count value
    draw_centered_text(
        renderer,
        &atlas::UI_FONT_INTER_100,
        icon_tex,
        &format!("{count}"),
        &layout.count,
        0.5,
    );

    // Minus button
    renderer.send_command(DrawQuad {
        color: (0.2, 0.4, 0.9, 1.0),
        border_color: (0.4, 0.6, 1.0, 1.0),
        origin: (layout.minus.x, layout.minus.y, 1.0),
        size: (layout.minus.w, layout.minus.h),
        border_radius: 12.0,
        border_thickness: 2.0,
    });
    draw_centered_text(
        renderer,
        &atlas::UI_FONT_INTER_24,
        icon_tex,
        "-",
        &layout.minus,
        0.4,
    );

    // Plus button
    renderer.send_command(DrawQuad {
        color: (0.2, 0.7, 0.3, 1.0),
        border_color: (0.4, 0.9, 0.5, 1.0),
        origin: (layout.plus.x, layout.plus.y, 1.0),
        size: (layout.plus.w, layout.plus.h),
        border_radius: 12.0,
        border_thickness: 2.0,
    });
    draw_centered_text(
        renderer,
        &atlas::UI_FONT_INTER_24,
        icon_tex,
        "+",
        &layout.plus,
        0.5,
    );

    renderer.process_command_queue::<ClearColor>();
    renderer.process_command_queue::<DrawRect>();
    renderer.process_command_queue::<DrawQuad>();
    renderer.process_command_queue::<DrawMonochromeSprite>();
    renderer.process_command_queue::<DrawText>();
}

/// Allocates a `zwp_linux_buffer_params_v1`, submits one plane's fd + metadata,
/// and creates a `wl_buffer` via `create_immed`. Returns the new wl_buffer id.
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
