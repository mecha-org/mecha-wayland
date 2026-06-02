use criterion::Criterion;
use glow::HasContext;
use renderer::{DmaBuf, Renderer, commands::ClearColor};
use utils::Color;

const WIDTH: u32 = 1028;
const HEIGHT: u32 = 1080;

// Lavender: hsl(240°, 67%, 94%) ≈ rgb(0.902, 0.902, 0.980)
const LAVENDER: Color = Color {
    r: 0.902,
    g: 0.902,
    b: 0.980,
    a: 1.0,
};

pub fn bench_lavender_render(c: &mut Criterion) {
    let mut renderer =
        Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");

    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");

    renderer.init_command_queue::<ClearColor>();
    c.bench_function("lavender_clear_1028x1080", |b| {
        b.iter(|| {
            renderer.active_surface(&surface);
            unsafe {
                renderer.send_command(ClearColor(LAVENDER));
                renderer.process_command_queue::<ClearColor>();
                renderer.gl.finish(); // blocks until GPU completes — measures true GPU time
            }
        });
    });

    renderer.destroy_surface(surface);
}
