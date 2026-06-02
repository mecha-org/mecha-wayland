#![recursion_limit = "4096"]

pub mod atlas {
    include!(concat!(env!("OUT_DIR"), "/ui_gen.rs"));
}
pub mod renderer;

mod state;
mod ui;
mod wayland_ext;

use ui::render_app_ui;

use state::AppState;
use wayland_ext::create_wl_buffer;

fn main() {
    let state = AppState::new();

    let mut app = app::App::new(state)
        .mount(|s| &mut s.engine.ring, io_ring::module())
        .mount(|s| &mut s.engine.timer, timer::module())
        .mount(|s| &mut s.engine.renderer, renderer::module())
        .mount(|s| &mut s.engine.wayland, wayland::module())
        .mount(|s| s, app::Module::new().on(on_layer_init))
        .mount(|s| s, app::Module::new().on(on_surface_configure))
        .mount(|s| s, app::Module::new().on(on_frame_callback))
        .mount(|s| s, app::Module::new().on(on_buffer_release))
        .mount(|s| s, app::Module::new().on(on_keyboard_event))
        .mount(|s| s, app::Module::new().on(on_pointer_event))
        .mount(|s| s, app::Module::new().on(on_touch_event));

    // app.run();
    app.dispatch(&app::Start);
    loop {
        app.dispatch(&app::PrePoll);
        app.dispatch(&app::Poll);
    }
}

fn on_layer_init(s: &mut AppState, _: &wayland::Initilised) {
    use wayland::zwlr_layer_shell::{KeyboardInteractivity, Layer};

    let surface_id = s.engine.wayland.compositor.create_surface();
    s.engine.wayland.surface.register(surface_id);
    s.engine.surface_id = surface_id;

    let layer_surface_id =
        s.engine
            .wayland
            .layer_shell
            .get_layer_surface(surface_id, 0, Layer::Top, "clock");
    s.engine.wayland.layer_surface.register(layer_surface_id);
    s.engine
        .wayland
        .layer_surface
        .set_size(layer_surface_id, 400, 360);
    s.engine
        .wayland
        .layer_surface
        .set_keyboard_interactivity(layer_surface_id, KeyboardInteractivity::OnDemand);

    s.engine.wayland.surface.commit(surface_id);
    s.engine.wayland.flush();
}

fn on_surface_configure(s: &mut AppState, ev: &wayland::zwlr_layer_shell::LayerSurfaceEvent) {
    let wayland::zwlr_layer_shell::LayerSurfaceEvent::Configured {
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
    s.engine.surface_size = (w, h);

    let surface0 = s
        .engine
        .renderer
        .create_surface::<::renderer::DmaBuf>(w as u32, h as u32)
        .expect("dmabuf surface 0");
    let surface1 = s
        .engine
        .renderer
        .create_surface::<::renderer::DmaBuf>(w as u32, h as u32)
        .expect("dmabuf surface 1");

    let buf_id0 = create_wl_buffer(&mut s.engine.wayland, &surface0, w, h);
    let buf_id1 = create_wl_buffer(&mut s.engine.wayland, &surface1, w, h);

    s.engine.wayland.wl_buffer.register(buf_id0);
    s.engine.wayland.wl_buffer.register(buf_id1);
    s.engine.wl_buf_ids = [buf_id0, buf_id1];

    if s.engine.icon_tex.is_none() {
        s.engine.icon_tex = s
            .engine
            .renderer
            .upload_atlas(crate::atlas::UI.png_bytes)
            .ok();
    }

    s.engine.renderer.active_surface(&surface0);
    s.hit_boxes = render_app_ui(s, w as f32, h as f32);
    s.engine.renderer.finish();

    s.engine.dmabuf = [Some(surface0), Some(surface1)];
    s.engine.buf_in_flight = [true, false];

    s.engine.wayland.layer_surface.ack_configure(*id, *serial);
    s.engine
        .wayland
        .surface
        .attach(s.engine.surface_id, buf_id0, 0, 0);
    s.engine
        .wayland
        .surface
        .damage(s.engine.surface_id, 0, 0, w, h);

    let cb_id = s.engine.wayland.surface.frame(s.engine.surface_id);
    s.engine.wayland.callback.register_frame(cb_id);

    s.engine.wayland.surface.commit(s.engine.surface_id);
    s.engine.wayland.flush();
}

fn on_frame_callback(s: &mut AppState, _: &wayland::WlCallbackEvent) {
    crate::ui::redraw(s);
}

fn on_buffer_release(s: &mut AppState, ev: &wayland::WlBufferEvent) {
    let wayland::WlBufferEvent::Release { id } = ev;
    for i in 0..2 {
        if s.engine.wl_buf_ids[i] == *id {
            s.engine.buf_in_flight[i] = false;
            break;
        }
    }
}

fn on_keyboard_event(_: &mut AppState, ev: &wayland::KeyboardEvent) {
    if let wayland::KeyboardEvent::Key { key, state, .. } = ev
        && (*key == 1 || *key == 16)
        && *state == wayland::KeyState::Pressed
    {
        println!("[Clock App] Exiting...");
        std::process::exit(0);
    }
}

fn on_pointer_event(s: &mut AppState, ev: &wayland::PointerEvent) {
    match ev {
        wayland::PointerEvent::Motion {
            surface_x,
            surface_y,
            ..
        } => {
            s.cursor_x = *surface_x;
            s.cursor_y = *surface_y;
        }
        wayland::PointerEvent::Button { state, .. } if *state == wayland::ButtonState::Pressed => {
            crate::ui::handle_click(s, s.cursor_x, s.cursor_y);
        }
        _ => {}
    }
}

fn on_touch_event(s: &mut AppState, ev: &wayland::TouchEvent) {
    if let wayland::TouchEvent::Down { x, y, .. } = ev {
        crate::ui::handle_click(s, *x, *y);
    }
}
