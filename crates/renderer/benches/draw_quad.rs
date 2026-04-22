use criterion::{BenchmarkId, Criterion, black_box};
use glow::HasContext;
use renderer::{DmaBuf, Renderer, commands::{DrawQuad, DrawRect}};

const WIDTH: u32 = 1028;
const HEIGHT: u32 = 1080;

const SIZES: &[(u32, u32)] = &[(64, 64), (256, 256), (512, 512), (1024, 1024)];

const N: usize = 100;

const MIN_QUAD_SIZE: f32 = 16.0;
const MAX_QUAD_SIZE: f32 = 1024.0;

pub fn bench_solid_quad(c: &mut Criterion) {
    let mut renderer =
        Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");
    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");
    renderer.set_width(WIDTH);
    renderer.set_height(HEIGHT);
    renderer.init_command_queue::<DrawRect>();
    renderer.init_command_queue::<DrawQuad>();

    let mut group = c.benchmark_group("solid_quad");
    for &(w, h) in SIZES {
        group.bench_with_input(BenchmarkId::new("size", format!("{w}x{h}")), &(w, h), |b, &(w, h)| {
            b.iter(|| unsafe {
                renderer
                    .gl
                    .bind_framebuffer(glow::FRAMEBUFFER, Some(black_box(surface.fbo)));
                renderer.gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
                renderer.gl.clear_depth_f32(0.0);
                renderer.gl.clear(glow::DEPTH_BUFFER_BIT);
                renderer.send_command(DrawQuad {
                    color: (0.2, 0.6, 1.0, 1.0),
                    border_color: (1.0, 1.0, 1.0, 1.0),
                    origin: (0.0, 0.0, 0.0),
                    size: (w as f32, h as f32),
                    border_radius: 8.0,
                    border_thickness: 0.0,
                });
                renderer.process_command_queue::<DrawRect>();
                renderer.process_command_queue::<DrawQuad>();
                renderer.gl.finish();
            });
        });
    }
    group.finish();

    renderer.destroy_surface(surface);
}

pub fn bench_translucent_quad(c: &mut Criterion) {
    let mut renderer =
        Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");
    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");
    renderer.set_width(WIDTH);
    renderer.set_height(HEIGHT);
    renderer.init_command_queue::<DrawRect>();
    renderer.init_command_queue::<DrawQuad>();

    let mut group = c.benchmark_group("translucent_quad");
    for &(w, h) in SIZES {
        group.bench_with_input(BenchmarkId::new("size", format!("{w}x{h}")), &(w, h), |b, &(w, h)| {
            b.iter(|| unsafe {
                renderer
                    .gl
                    .bind_framebuffer(glow::FRAMEBUFFER, Some(black_box(surface.fbo)));
                renderer.gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
                renderer.gl.clear_depth_f32(0.0);
                renderer.gl.clear(glow::DEPTH_BUFFER_BIT);
                renderer.send_command(DrawQuad {
                    color: (0.2, 0.6, 1.0, 0.5),
                    border_color: (1.0, 1.0, 1.0, 1.0),
                    origin: (0.0, 0.0, 0.0),
                    size: (w as f32, h as f32),
                    border_radius: 8.0,
                    border_thickness: 0.0,
                });
                renderer.process_command_queue::<DrawRect>();
                renderer.process_command_queue::<DrawQuad>();
                renderer.gl.finish();
            });
        });
    }
    group.finish();

    renderer.destroy_surface(surface);
}

pub fn bench_solid_stacked(c: &mut Criterion) {
    let mut renderer =
        Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");
    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");
    renderer.set_width(WIDTH);
    renderer.set_height(HEIGHT);
    renderer.init_command_queue::<DrawRect>();
    renderer.init_command_queue::<DrawQuad>();

    let z_step = 1.0 / N as f32;

    let mut group = c.benchmark_group("solid_quad_stacked_N100");
    for &(w, h) in SIZES {
        group.bench_with_input(BenchmarkId::new("size", format!("{w}x{h}")), &(w, h), |b, &(w, h)| {
            b.iter(|| unsafe {
                renderer
                    .gl
                    .bind_framebuffer(glow::FRAMEBUFFER, Some(black_box(surface.fbo)));
                renderer.gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
                renderer.gl.clear_depth_f32(0.0);
                renderer.gl.clear(glow::DEPTH_BUFFER_BIT);
                for i in 0..N {
                    renderer.send_command(DrawQuad {
                        color: (0.2, 0.6, 1.0, 1.0),
                        border_color: (1.0, 1.0, 1.0, 1.0),
                        origin: (0.0, 0.0, i as f32 * z_step),
                        size: (w as f32, h as f32),
                        border_radius: 8.0,
                        border_thickness: 0.0,
                    });
                }
                renderer.process_command_queue::<DrawRect>();
                renderer.process_command_queue::<DrawQuad>();
                renderer.gl.finish();
            });
        });
    }
    group.finish();

    renderer.destroy_surface(surface);
}

pub fn bench_translucent_stacked(c: &mut Criterion) {
    let mut renderer =
        Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");
    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");
    renderer.set_width(WIDTH);
    renderer.set_height(HEIGHT);
    renderer.init_command_queue::<DrawRect>();
    renderer.init_command_queue::<DrawQuad>();

    let z_step = 1.0 / N as f32;

    let mut group = c.benchmark_group("translucent_quad_stacked_N100");
    for &(w, h) in SIZES {
        group.bench_with_input(BenchmarkId::new("size", format!("{w}x{h}")), &(w, h), |b, &(w, h)| {
            b.iter(|| unsafe {
                renderer
                    .gl
                    .bind_framebuffer(glow::FRAMEBUFFER, Some(black_box(surface.fbo)));
                renderer.gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
                renderer.gl.clear_depth_f32(0.0);
                renderer.gl.clear(glow::DEPTH_BUFFER_BIT);
                for i in 0..N {
                    renderer.send_command(DrawQuad {
                        color: (0.2, 0.6, 1.0, 0.5),
                        border_color: (1.0, 1.0, 1.0, 1.0),
                        origin: (0.0, 0.0, i as f32 * z_step),
                        size: (w as f32, h as f32),
                        border_radius: 8.0,
                        border_thickness: 0.0,
                    });
                }
                renderer.process_command_queue::<DrawRect>();
                renderer.process_command_queue::<DrawQuad>();
                renderer.gl.finish();
            });
        });
    }
    group.finish();

    renderer.destroy_surface(surface);
}

pub fn bench_mixed_stacked(c: &mut Criterion) {
    let mut renderer =
        Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");
    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");
    renderer.set_width(WIDTH);
    renderer.set_height(HEIGHT);
    renderer.init_command_queue::<DrawRect>();
    renderer.init_command_queue::<DrawQuad>();

    let z_step = 1.0 / N as f32;

    let mut group = c.benchmark_group("mixed_quad_stacked_N100");
    for &(w, h) in SIZES {
        group.bench_with_input(BenchmarkId::new("size", format!("{w}x{h}")), &(w, h), |b, &(w, h)| {
            b.iter(|| unsafe {
                renderer
                    .gl
                    .bind_framebuffer(glow::FRAMEBUFFER, Some(black_box(surface.fbo)));
                renderer.gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
                renderer.gl.clear_depth_f32(0.0);
                renderer.gl.clear(glow::DEPTH_BUFFER_BIT);
                for i in 0..N {
                    let alpha = if i % 2 == 0 { 1.0 } else { 0.5 };
                    renderer.send_command(DrawQuad {
                        color: (0.2, 0.6, 1.0, alpha),
                        border_color: (1.0, 1.0, 1.0, 1.0),
                        origin: (0.0, 0.0, i as f32 * z_step),
                        size: (w as f32, h as f32),
                        border_radius: 8.0,
                        border_thickness: 0.0,
                    });
                }
                renderer.process_command_queue::<DrawRect>();
                renderer.process_command_queue::<DrawQuad>();
                renderer.gl.finish();
            });
        });
    }
    group.finish();

    renderer.destroy_surface(surface);
}

pub fn bench_solid_growing(c: &mut Criterion) {
    let mut renderer =
        Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");
    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");
    renderer.set_width(WIDTH);
    renderer.set_height(HEIGHT);
    renderer.init_command_queue::<DrawRect>();
    renderer.init_command_queue::<DrawQuad>();

    let z_step = 1.0 / N as f32;

    let mut group = c.benchmark_group("solid_quad_growing_N100");
    group.bench_function("size_16_to_1024", |b| {
        b.iter(|| unsafe {
            renderer
                .gl
                .bind_framebuffer(glow::FRAMEBUFFER, Some(black_box(surface.fbo)));
            renderer.gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
            renderer.gl.clear_depth_f32(0.0);
            renderer.gl.clear(glow::DEPTH_BUFFER_BIT);
            for i in 0..N {
                let t = i as f32 / (N - 1) as f32;
                let s = MIN_QUAD_SIZE + t * (MAX_QUAD_SIZE - MIN_QUAD_SIZE);
                renderer.send_command(DrawQuad {
                    color: (0.2, 0.6, 1.0, 1.0),
                    border_color: (1.0, 1.0, 1.0, 1.0),
                    origin: (0.0, 0.0, i as f32 * z_step),
                    size: (s, s),
                    border_radius: 8.0,
                    border_thickness: 0.0,
                });
            }
            renderer.process_command_queue::<DrawRect>();
            renderer.process_command_queue::<DrawQuad>();
            renderer.gl.finish();
        });
    });
    group.finish();

    renderer.destroy_surface(surface);
}

pub fn bench_translucent_growing(c: &mut Criterion) {
    let mut renderer =
        Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");
    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");
    renderer.set_width(WIDTH);
    renderer.set_height(HEIGHT);
    renderer.init_command_queue::<DrawRect>();
    renderer.init_command_queue::<DrawQuad>();

    let z_step = 1.0 / N as f32;

    let mut group = c.benchmark_group("translucent_quad_growing_N100");
    group.bench_function("size_16_to_1024", |b| {
        b.iter(|| unsafe {
            renderer
                .gl
                .bind_framebuffer(glow::FRAMEBUFFER, Some(black_box(surface.fbo)));
            renderer.gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
            renderer.gl.clear_depth_f32(0.0);
            renderer.gl.clear(glow::DEPTH_BUFFER_BIT);
            for i in 0..N {
                let t = i as f32 / (N - 1) as f32;
                let s = MIN_QUAD_SIZE + t * (MAX_QUAD_SIZE - MIN_QUAD_SIZE);
                renderer.send_command(DrawQuad {
                    color: (0.2, 0.6, 1.0, 0.5),
                    border_color: (1.0, 1.0, 1.0, 1.0),
                    origin: (0.0, 0.0, i as f32 * z_step),
                    size: (s, s),
                    border_radius: 8.0,
                    border_thickness: 0.0,
                });
            }
            renderer.process_command_queue::<DrawRect>();
            renderer.process_command_queue::<DrawQuad>();
            renderer.gl.finish();
        });
    });
    group.finish();

    renderer.destroy_surface(surface);
}

pub fn bench_mixed_growing(c: &mut Criterion) {
    let mut renderer =
        Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");
    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");
    renderer.set_width(WIDTH);
    renderer.set_height(HEIGHT);
    renderer.init_command_queue::<DrawRect>();
    renderer.init_command_queue::<DrawQuad>();

    let z_step = 1.0 / N as f32;

    let mut group = c.benchmark_group("mixed_quad_growing_N100");
    group.bench_function("size_16_to_1024", |b| {
        b.iter(|| unsafe {
            renderer
                .gl
                .bind_framebuffer(glow::FRAMEBUFFER, Some(black_box(surface.fbo)));
            renderer.gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
            renderer.gl.clear_depth_f32(0.0);
            renderer.gl.clear(glow::DEPTH_BUFFER_BIT);
            for i in 0..N {
                let t = i as f32 / (N - 1) as f32;
                let s = MIN_QUAD_SIZE + t * (MAX_QUAD_SIZE - MIN_QUAD_SIZE);
                let alpha = if i % 2 == 0 { 1.0 } else { 0.5 };
                renderer.send_command(DrawQuad {
                    color: (0.2, 0.6, 1.0, alpha),
                    border_color: (1.0, 1.0, 1.0, 1.0),
                    origin: (0.0, 0.0, i as f32 * z_step),
                    size: (s, s),
                    border_radius: 8.0,
                    border_thickness: 0.0,
                });
            }
            renderer.process_command_queue::<DrawRect>();
            renderer.process_command_queue::<DrawQuad>();
            renderer.gl.finish();
        });
    });
    group.finish();

    renderer.destroy_surface(surface);
}

pub fn bench_solid_shrinking(c: &mut Criterion) {
    let mut renderer =
        Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");
    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");
    renderer.set_width(WIDTH);
    renderer.set_height(HEIGHT);
    renderer.init_command_queue::<DrawRect>();
    renderer.init_command_queue::<DrawQuad>();

    let z_step = 1.0 / N as f32;

    let mut group = c.benchmark_group("solid_quad_shrinking_N100");
    group.bench_function("size_1024_to_16", |b| {
        b.iter(|| unsafe {
            renderer
                .gl
                .bind_framebuffer(glow::FRAMEBUFFER, Some(black_box(surface.fbo)));
            renderer.gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
            renderer.gl.clear_depth_f32(0.0);
            renderer.gl.clear(glow::DEPTH_BUFFER_BIT);
            for i in 0..N {
                let t = i as f32 / (N - 1) as f32;
                let s = MAX_QUAD_SIZE - t * (MAX_QUAD_SIZE - MIN_QUAD_SIZE);
                renderer.send_command(DrawQuad {
                    color: (0.2, 0.6, 1.0, 1.0),
                    border_color: (1.0, 1.0, 1.0, 1.0),
                    origin: (0.0, 0.0, i as f32 * z_step),
                    size: (s, s),
                    border_radius: 8.0,
                    border_thickness: 0.0,
                });
            }
            renderer.process_command_queue::<DrawRect>();
            renderer.process_command_queue::<DrawQuad>();
            renderer.gl.finish();
        });
    });
    group.finish();

    renderer.destroy_surface(surface);
}

pub fn bench_translucent_shrinking(c: &mut Criterion) {
    let mut renderer =
        Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");
    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");
    renderer.set_width(WIDTH);
    renderer.set_height(HEIGHT);
    renderer.init_command_queue::<DrawRect>();
    renderer.init_command_queue::<DrawQuad>();

    let z_step = 1.0 / N as f32;

    let mut group = c.benchmark_group("translucent_quad_shrinking_N100");
    group.bench_function("size_1024_to_16", |b| {
        b.iter(|| unsafe {
            renderer
                .gl
                .bind_framebuffer(glow::FRAMEBUFFER, Some(black_box(surface.fbo)));
            renderer.gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
            renderer.gl.clear_depth_f32(0.0);
            renderer.gl.clear(glow::DEPTH_BUFFER_BIT);
            for i in 0..N {
                let t = i as f32 / (N - 1) as f32;
                let s = MAX_QUAD_SIZE - t * (MAX_QUAD_SIZE - MIN_QUAD_SIZE);
                renderer.send_command(DrawQuad {
                    color: (0.2, 0.6, 1.0, 0.5),
                    border_color: (1.0, 1.0, 1.0, 1.0),
                    origin: (0.0, 0.0, i as f32 * z_step),
                    size: (s, s),
                    border_radius: 8.0,
                    border_thickness: 0.0,
                });
            }
            renderer.process_command_queue::<DrawRect>();
            renderer.process_command_queue::<DrawQuad>();
            renderer.gl.finish();
        });
    });
    group.finish();

    renderer.destroy_surface(surface);
}

pub fn bench_mixed_shrinking(c: &mut Criterion) {
    let mut renderer =
        Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");
    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");
    renderer.set_width(WIDTH);
    renderer.set_height(HEIGHT);
    renderer.init_command_queue::<DrawRect>();
    renderer.init_command_queue::<DrawQuad>();

    let z_step = 1.0 / N as f32;

    let mut group = c.benchmark_group("mixed_quad_shrinking_N100");
    group.bench_function("size_1024_to_16", |b| {
        b.iter(|| unsafe {
            renderer
                .gl
                .bind_framebuffer(glow::FRAMEBUFFER, Some(black_box(surface.fbo)));
            renderer.gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
            renderer.gl.clear_depth_f32(0.0);
            renderer.gl.clear(glow::DEPTH_BUFFER_BIT);
            for i in 0..N {
                let t = i as f32 / (N - 1) as f32;
                let s = MAX_QUAD_SIZE - t * (MAX_QUAD_SIZE - MIN_QUAD_SIZE);
                let alpha = if i % 2 == 0 { 1.0 } else { 0.5 };
                renderer.send_command(DrawQuad {
                    color: (0.2, 0.6, 1.0, alpha),
                    border_color: (1.0, 1.0, 1.0, 1.0),
                    origin: (0.0, 0.0, i as f32 * z_step),
                    size: (s, s),
                    border_radius: 8.0,
                    border_thickness: 0.0,
                });
            }
            renderer.process_command_queue::<DrawRect>();
            renderer.process_command_queue::<DrawQuad>();
            renderer.gl.finish();
        });
    });
    group.finish();

    renderer.destroy_surface(surface);
}
