use criterion::{Criterion, black_box};
use glow::HasContext;
use renderer::{DmaBuf, Renderer};

const WIDTH: u32 = 1028;
const HEIGHT: u32 = 1080;

pub fn bench_lavender_render(c: &mut Criterion) {
    let renderer = Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");
    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");

    c.bench_function("lavender_clear_1028x1080", |b| {
        b.iter(|| unsafe {
            renderer.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(black_box(surface.fbo)));
            renderer.gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
            renderer.gl.clear_color(0.902, 0.902, 0.980, 1.0);
            renderer.gl.clear(glow::COLOR_BUFFER_BIT);
            renderer.gl.finish(); // blocks until GPU completes — measures true GPU time
        });
    });

    renderer.destroy_surface(surface);
}
