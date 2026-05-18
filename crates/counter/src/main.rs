#![recursion_limit = "4096"]

mod atlas {
    include!(concat!(env!("OUT_DIR"), "/ui_gen.rs"));
}
mod renderer;
mod ring;
mod timer;
mod wayland;
mod wire;

use std::os::fd::AsRawFd;

use app::{App, event::Event};
use ring::Ring;
use timer::{Timer, TimerEvents, TimerSettings};
use wayland::Wayland;

// ARGB8888 little-endian fourcc
const DRM_FORMAT_ARGB8888: u32 = 0x34325241;

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
        }
    }
}

#[derive(Default)]
struct Counter {
    count: u32,
}

#[derive(Clone, Copy, Debug)]
pub enum CounterEvent {
    Updated { new_count: u32 },
}

impl Event for CounterEvent {}

fn main() {
    let state = AppState::new();

    let mut app = app::App::new(state)
        .register_module(|s| &mut s.ring, register_ring!(1))
        .register_module(|s| &mut s.timer, register_timer!())
        .register_module(|s| &mut s.renderer, register_renderer!())
        .register_module(|s| &mut s.wayland, register_wayland!())
        .register_module(
            |s| s,
            app::module::Module::new().on(|s: &mut AppState, _: &wayland::Initilised| {
                use wayland::zwlr_layer_shell::LAYER_TOP;

                let surface_id = s.wayland.compositor.create_surface();
                s.wayland.surface.register(surface_id);
                s.surface_id = surface_id;

                let layer_surface_id = s
                    .wayland
                    .layer_shell
                    .get_layer_surface(surface_id, 0, LAYER_TOP, "counter");
                s.wayland.layer_surface.register(layer_surface_id);
                s.wayland.layer_surface.set_size(layer_surface_id, 400, 360);
                s.wayland
                    .layer_surface
                    .set_keyboard_interactivity(layer_surface_id, 2);

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
                        s.icon_tex = s
                            .renderer
                            .upload_atlas(atlas::UI.png_bytes)
                            .ok();
                    }

                    // Render the counter UI into surface 0 for the first frame.
                    s.renderer.active_surface(&surface0);
                    render_counter_ui(&mut s.renderer, s.counter.count, s.icon_tex.unwrap());
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
                    render_counter_ui(&mut s.renderer, s.counter.count, icon_tex);
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
                    if (*key == 1 || *key == 16) && *state == 1 {
                        println!("[App] Exiting...");
                        std::process::exit(0);
                    }
                }
            }),
        )
        .register_module(
            |s| s,
            app::module::Module::new().on(|_: &mut AppState, ev: &wayland::PointerEvent| {
                println!("[App] Pointer Event: {:?}", ev);
            }),
        )
        .register_module(
            |s| s,
            app::module::Module::new().on(|_: &mut AppState, ev: &wayland::TouchEvent| {
                println!("[App] Touch Event: {:?}", ev);
            }),
        );

    app.run();
}

fn render_counter_ui(
    renderer: &mut ::renderer::Renderer,
    count: u32,
    icon_tex: ::renderer::TextureId,
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
        origin: (0.0, 0.0, 0.0),
        size: (400.0, 360.0),
        border_radius: 20.0,
        border_thickness: 2.0,
    });

    // Title
    renderer.send_command(DrawText {
        font: &atlas::UI_FONT_INTER_24,
        texture_id: icon_tex,
        text: "Counter".to_string(),
        origin: (16.0, 48.0, 0.5),
        color: (1.0, 1.0, 1.0, 1.0),
    });

    // Count value
    renderer.send_command(DrawText {
        font: &atlas::UI_FONT_INTER_100,
        texture_id: icon_tex,
        text: format!("{count}"),
        origin: (150.0, 188.0, 0.5),
        color: (1.0, 1.0, 1.0, 1.0),
    });

    // Minus button
    renderer.send_command(DrawQuad {
        color: (0.2, 0.4, 0.9, 1.0),
        border_color: (0.4, 0.6, 1.0, 1.0),
        origin: (60.0, 238.0, 1.0),
        size: (110.0, 52.0),
        border_radius: 12.0,
        border_thickness: 2.0,
    });
    renderer.send_command(DrawText {
        font: &atlas::UI_FONT_INTER_24,
        texture_id: icon_tex,
        text: "-".to_string(),
        origin: (105.0, 274.0, 0.4),
        color: (1.0, 1.0, 1.0, 1.0),
    });

    // Plus button
    renderer.send_command(DrawQuad {
        color: (0.2, 0.7, 0.3, 1.0),
        border_color: (0.4, 0.9, 0.5, 1.0),
        origin: (230.0, 238.0, 1.0),
        size: (110.0, 52.0),
        border_radius: 12.0,
        border_thickness: 2.0,
    });
    renderer.send_command(DrawText {
        font: &atlas::UI_FONT_INTER_24,
        texture_id: icon_tex,
        text: "+".to_string(),
        origin: (275.0, 274.0, 0.5),
        color: (1.0, 1.0, 1.0, 1.0),
    });

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
