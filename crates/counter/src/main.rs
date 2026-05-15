#![recursion_limit = "4096"]

mod ring;
mod timer;
mod wayland;
mod wire;

use app::{App, event::Event};
use ring::Ring;
use timer::{Timer, TimerEvents, TimerSettings};
use wayland::Wayland;

struct AppState {
    ring: Ring,
    timer: Timer,
    counter: Counter,
    wayland: Wayland,
    surface_id: u32,
}

impl AppState {
    fn new() -> Self {
        let ring = Ring::default();
        let timer = Timer::new(ring.get_proxy());
        let wayland = Wayland::new(ring.get_proxy()).expect("failed to create wayland connection");

        Self {
            ring,
            timer,
            counter: Counter::default(),
            wayland,
            surface_id: 0,
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
                s.wayland.layer_surface.set_size(layer_surface_id, 256, 256);
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
                    use wayland::wl_shm::{alloc_shm_fd, mmap_shm};
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
                    let stride = w * 4;
                    let size = (stride * h) as usize;

                    let fd = alloc_shm_fd(size);
                    let ptr = mmap_shm(fd, size);
                    // ARGB8888 opaque black: A=0xFF, R=0, G=0, B=0
                    unsafe {
                        let pixels = std::slice::from_raw_parts_mut(
                            ptr as *mut u32,
                            w as usize * h as usize,
                        );
                        for p in pixels.iter_mut() {
                            *p = 0xFF000000u32;
                        }
                        libc::munmap(ptr as *mut libc::c_void, size);
                    }

                    s.wayland.layer_surface.ack_configure(*id, *serial);

                    let pool_id = s.wayland.shm.create_pool(fd, size as i32);
                    s.wayland.shm.register_pool(pool_id);
                    let buf_id = s
                        .wayland
                        .shm
                        .pool_create_buffer(pool_id, 0, w, h, stride, 0);
                    s.wayland.shm.register_buffer(buf_id);
                    s.wayland.shm.pool_destroy(pool_id);

                    s.wayland.surface.attach(s.surface_id, buf_id, 0, 0);
                    s.wayland.surface.damage(s.surface_id, 0, 0, w, h);
                    s.wayland.surface.commit(s.surface_id);
                    // flush via sendmsg since the shm fd is queued
                    s.wayland.flush();
                },
            ),
        )
        .register_module(
            |s| s,
            app::module::Module::new().on(|_: &mut AppState, ev: &wayland::KeyboardEvent| {
                println!("[App] Keyboard Event: {:?}", ev);
                if let wayland::KeyboardEvent::Key { key, state, .. } = ev {
                    // keycode 1 is Escape, 16 is 'q' (linux keycodes)
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
