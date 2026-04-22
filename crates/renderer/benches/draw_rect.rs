use criterion::{BenchmarkId, Criterion};
use glow::HasContext;
use renderer::{DmaBuf, Renderer, commands::DrawRect};

const WIDTH: u32 = 1028;
const HEIGHT: u32 = 1080;

const SIZES: &[(u32, u32)] = &[(64, 64), (256, 256), (512, 512), (1024, 1024)];

// Number of rectangles in stacked benchmarks. Change here to adjust all groups.
const N: usize = 100;

// Size range for growing-rect benchmarks: back rect starts at MIN, front rect ends at MAX.
const MIN_RECT_SIZE: f32 = 16.0;
const MAX_RECT_SIZE: f32 = 1024.0;

pub fn bench_solid_rect(c: &mut Criterion) {
    let mut renderer =
        Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");
    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");
    renderer.init_command_queue::<DrawRect>();

    let mut group = c.benchmark_group("solid_rect");
    for &(w, h) in SIZES {
        group.bench_with_input(BenchmarkId::new("size", format!("{w}x{h}")), &(w, h), |b, &(w, h)| {
            b.iter(|| {
                renderer.active_surface(&surface);
                unsafe {
                    renderer.gl.clear_depth_f32(0.0);
                    renderer.gl.clear(glow::DEPTH_BUFFER_BIT);
                    renderer.send_command(DrawRect {
                        color: (0.2, 0.6, 1.0, 1.0),
                        origin: (0.0, 0.0, 0.0),
                        size: (w as f32, h as f32),
                    });
                    renderer.process_command_queue::<DrawRect>();
                    renderer.gl.finish();
                }
            });
        });
    }
    group.finish();

    renderer.destroy_surface(surface);
}

pub fn bench_translucent_rect(c: &mut Criterion) {
    let mut renderer =
        Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");
    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");
    renderer.init_command_queue::<DrawRect>();

    let mut group = c.benchmark_group("translucent_rect");
    for &(w, h) in SIZES {
        group.bench_with_input(BenchmarkId::new("size", format!("{w}x{h}")), &(w, h), |b, &(w, h)| {
            b.iter(|| {
                renderer.active_surface(&surface);
                unsafe {
                    renderer.gl.clear_depth_f32(0.0);
                    renderer.gl.clear(glow::DEPTH_BUFFER_BIT);
                    renderer.send_command(DrawRect {
                        color: (0.2, 0.6, 1.0, 0.5),
                        origin: (0.0, 0.0, 0.0),
                        size: (w as f32, h as f32),
                    });
                    renderer.process_command_queue::<DrawRect>();
                    renderer.gl.finish();
                }
            });
        });
    }
    group.finish();

    renderer.destroy_surface(surface);
}

// N solid rects, all same size, stacked back-to-front with z stepping from 0 → 1.
pub fn bench_solid_stacked(c: &mut Criterion) {
    let mut renderer =
        Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");
    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");
    renderer.init_command_queue::<DrawRect>();

    let z_step = 1.0 / N as f32;

    let mut group = c.benchmark_group("solid_stacked_N100");
    for &(w, h) in SIZES {
        group.bench_with_input(BenchmarkId::new("size", format!("{w}x{h}")), &(w, h), |b, &(w, h)| {
            b.iter(|| {
                renderer.active_surface(&surface);
                unsafe {
                    renderer.gl.clear_depth_f32(0.0);
                    renderer.gl.clear(glow::DEPTH_BUFFER_BIT);
                    for i in 0..N {
                        renderer.send_command(DrawRect {
                            color: (0.2, 0.6, 1.0, 1.0),
                            origin: (0.0, 0.0, i as f32 * z_step),
                            size: (w as f32, h as f32),
                        });
                    }
                    renderer.process_command_queue::<DrawRect>();
                    renderer.gl.finish();
                }
            });
        });
    }
    group.finish();

    renderer.destroy_surface(surface);
}

// N translucent rects, all same size, stacked back-to-front with z stepping from 0 → 1.
pub fn bench_translucent_stacked(c: &mut Criterion) {
    let mut renderer =
        Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");
    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");
    renderer.init_command_queue::<DrawRect>();

    let z_step = 1.0 / N as f32;

    let mut group = c.benchmark_group("translucent_stacked_N100");
    for &(w, h) in SIZES {
        group.bench_with_input(BenchmarkId::new("size", format!("{w}x{h}")), &(w, h), |b, &(w, h)| {
            b.iter(|| {
                renderer.active_surface(&surface);
                unsafe {
                    renderer.gl.clear_depth_f32(0.0);
                    renderer.gl.clear(glow::DEPTH_BUFFER_BIT);
                    for i in 0..N {
                        renderer.send_command(DrawRect {
                            color: (0.2, 0.6, 1.0, 0.5),
                            origin: (0.0, 0.0, i as f32 * z_step),
                            size: (w as f32, h as f32),
                        });
                    }
                    renderer.process_command_queue::<DrawRect>();
                    renderer.gl.finish();
                }
            });
        });
    }
    group.finish();

    renderer.destroy_surface(surface);
}

// N rects stacked back-to-front: even indices solid (a=1.0), odd indices translucent (a=0.5).
// With N=100 this gives exactly 50 solid and 50 translucent, interleaved.
pub fn bench_mixed_stacked(c: &mut Criterion) {
    let mut renderer =
        Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");
    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");
    renderer.init_command_queue::<DrawRect>();

    let z_step = 1.0 / N as f32;

    let mut group = c.benchmark_group("mixed_stacked_N100");
    for &(w, h) in SIZES {
        group.bench_with_input(BenchmarkId::new("size", format!("{w}x{h}")), &(w, h), |b, &(w, h)| {
            b.iter(|| {
                renderer.active_surface(&surface);
                unsafe {
                    renderer.gl.clear_depth_f32(0.0);
                    renderer.gl.clear(glow::DEPTH_BUFFER_BIT);
                    for i in 0..N {
                        let alpha = if i % 2 == 0 { 1.0 } else { 0.5 };
                        renderer.send_command(DrawRect {
                            color: (0.2, 0.6, 1.0, alpha),
                            origin: (0.0, 0.0, i as f32 * z_step),
                            size: (w as f32, h as f32),
                        });
                    }
                    renderer.process_command_queue::<DrawRect>();
                    renderer.gl.finish();
                }
            });
        });
    }
    group.finish();

    renderer.destroy_surface(surface);
}

// --- Growing: size increases back → front (small at back, large at front) ---

pub fn bench_solid_growing(c: &mut Criterion) {
    let mut renderer =
        Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");
    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");
    renderer.init_command_queue::<DrawRect>();

    let z_step = 1.0 / N as f32;

    let mut group = c.benchmark_group("solid_growing_N100");
    group.bench_function("size_16_to_1024", |b| {
        b.iter(|| {
            renderer.active_surface(&surface);
            unsafe {
                for i in 0..N {
                    let t = i as f32 / (N - 1) as f32;
                    let s = MIN_RECT_SIZE + t * (MAX_RECT_SIZE - MIN_RECT_SIZE);
                    renderer.send_command(DrawRect {
                        color: (0.2, 0.6, 1.0, 1.0),
                        origin: (0.0, 0.0, i as f32 * z_step),
                        size: (s, s),
                    });
                }
                renderer.process_command_queue::<DrawRect>();
                renderer.gl.finish();
            }
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
    renderer.init_command_queue::<DrawRect>();

    let z_step = 1.0 / N as f32;

    let mut group = c.benchmark_group("translucent_growing_N100");
    group.bench_function("size_16_to_1024", |b| {
        b.iter(|| {
            renderer.active_surface(&surface);
            unsafe {
                for i in 0..N {
                    let t = i as f32 / (N - 1) as f32;
                    let s = MIN_RECT_SIZE + t * (MAX_RECT_SIZE - MIN_RECT_SIZE);
                    renderer.send_command(DrawRect {
                        color: (0.2, 0.6, 1.0, 0.5),
                        origin: (0.0, 0.0, i as f32 * z_step),
                        size: (s, s),
                    });
                }
                renderer.process_command_queue::<DrawRect>();
                renderer.gl.finish();
            }
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
    renderer.init_command_queue::<DrawRect>();

    let z_step = 1.0 / N as f32;

    let mut group = c.benchmark_group("mixed_growing_N100");
    group.bench_function("size_16_to_1024", |b| {
        b.iter(|| {
            renderer.active_surface(&surface);
            unsafe {
                for i in 0..N {
                    let t = i as f32 / (N - 1) as f32;
                    let s = MIN_RECT_SIZE + t * (MAX_RECT_SIZE - MIN_RECT_SIZE);
                    let alpha = if i % 2 == 0 { 1.0 } else { 0.5 };
                    renderer.send_command(DrawRect {
                        color: (0.2, 0.6, 1.0, alpha),
                        origin: (0.0, 0.0, i as f32 * z_step),
                        size: (s, s),
                    });
                }
                renderer.process_command_queue::<DrawRect>();
                renderer.gl.finish();
            }
        });
    });
    group.finish();

    renderer.destroy_surface(surface);
}

// --- Shrinking: size decreases back → front (large at back, small at front) ---

pub fn bench_solid_shrinking(c: &mut Criterion) {
    let mut renderer =
        Renderer::new().expect("Renderer::new failed — needs /dev/dri/renderD* and EGL");
    let surface = renderer
        .create_surface::<DmaBuf>(WIDTH, HEIGHT)
        .expect("create_surface failed");
    renderer.init_command_queue::<DrawRect>();

    let z_step = 1.0 / N as f32;

    let mut group = c.benchmark_group("solid_shrinking_N100");
    group.bench_function("size_1024_to_16", |b| {
        b.iter(|| {
            renderer.active_surface(&surface);
            unsafe {
                for i in 0..N {
                    let t = i as f32 / (N - 1) as f32;
                    let s = MAX_RECT_SIZE - t * (MAX_RECT_SIZE - MIN_RECT_SIZE);
                    renderer.send_command(DrawRect {
                        color: (0.2, 0.6, 1.0, 1.0),
                        origin: (0.0, 0.0, i as f32 * z_step),
                        size: (s, s),
                    });
                }
                renderer.process_command_queue::<DrawRect>();
                renderer.gl.finish();
            }
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
    renderer.init_command_queue::<DrawRect>();

    let z_step = 1.0 / N as f32;

    let mut group = c.benchmark_group("translucent_shrinking_N100");
    group.bench_function("size_1024_to_16", |b| {
        b.iter(|| {
            renderer.active_surface(&surface);
            unsafe {
                for i in 0..N {
                    let t = i as f32 / (N - 1) as f32;
                    let s = MAX_RECT_SIZE - t * (MAX_RECT_SIZE - MIN_RECT_SIZE);
                    renderer.send_command(DrawRect {
                        color: (0.2, 0.6, 1.0, 0.5),
                        origin: (0.0, 0.0, i as f32 * z_step),
                        size: (s, s),
                    });
                }
                renderer.process_command_queue::<DrawRect>();
                renderer.gl.finish();
            }
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
    renderer.init_command_queue::<DrawRect>();

    let z_step = 1.0 / N as f32;

    let mut group = c.benchmark_group("mixed_shrinking_N100");
    group.bench_function("size_1024_to_16", |b| {
        b.iter(|| {
            renderer.active_surface(&surface);
            unsafe {
                for i in 0..N {
                    let t = i as f32 / (N - 1) as f32;
                    let s = MAX_RECT_SIZE - t * (MAX_RECT_SIZE - MIN_RECT_SIZE);
                    let alpha = if i % 2 == 0 { 1.0 } else { 0.5 };
                    renderer.send_command(DrawRect {
                        color: (0.2, 0.6, 1.0, alpha),
                        origin: (0.0, 0.0, i as f32 * z_step),
                        size: (s, s),
                    });
                }
                renderer.process_command_queue::<DrawRect>();
                renderer.gl.finish();
            }
        });
    });
    group.finish();

    renderer.destroy_surface(surface);
}
