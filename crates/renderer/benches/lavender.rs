use criterion::{Criterion, black_box};
use glow::HasContext;
use renderer::{DmaBuf, Renderer, commands::ClearColor};

const WIDTH: u32 = 1028;
const HEIGHT: u32 = 1080;

pub fn bench_lavender_render(c: &mut Criterion) {
    let mut renderer =
        Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");

    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");

    renderer.set_width(WIDTH);
    renderer.set_height(HEIGHT);
    renderer.init_command_queue::<ClearColor>();
    c.bench_function("lavender_clear_1028x1080", |b| {
        b.iter(|| unsafe {
            renderer
                .gl
                .bind_framebuffer(glow::FRAMEBUFFER, Some(black_box(surface.fbo)));
            renderer.gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
            let command = ClearColor {
                r: 0.902,
                g: 0.902,
                b: 0.980,
                a: 1.000,
            };
            renderer.send_command(command);
            renderer.process_command_queue::<ClearColor>();
            renderer.gl.finish(); // blocks until GPU completes — measures true GPU time
        });
    });

    renderer.destroy_surface(surface);
}
