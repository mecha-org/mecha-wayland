use criterion::{Criterion, criterion_group, criterion_main};

mod draw_quad;
mod draw_rect;
mod lavender;
use draw_quad::{
    bench_mixed_growing as bench_quad_mixed_growing,
    bench_mixed_shrinking as bench_quad_mixed_shrinking,
    bench_mixed_stacked as bench_quad_mixed_stacked,
    bench_solid_growing as bench_quad_solid_growing,
    bench_solid_quad,
    bench_solid_shrinking as bench_quad_solid_shrinking,
    bench_solid_stacked as bench_quad_solid_stacked,
    bench_translucent_growing as bench_quad_translucent_growing,
    bench_translucent_quad,
    bench_translucent_shrinking as bench_quad_translucent_shrinking,
    bench_translucent_stacked as bench_quad_translucent_stacked,
};
use draw_rect::{
    bench_mixed_growing, bench_mixed_shrinking, bench_mixed_stacked, bench_solid_growing,
    bench_solid_rect, bench_solid_shrinking, bench_solid_stacked, bench_translucent_growing,
    bench_translucent_rect, bench_translucent_shrinking, bench_translucent_stacked,
};
use lavender::bench_lavender_render;

criterion_group! {
    name = renderer_benches;
    config = Criterion::default();
    targets = bench_lavender_render,
        bench_solid_rect, bench_translucent_rect,
        bench_solid_stacked, bench_translucent_stacked, bench_mixed_stacked,
        bench_solid_growing, bench_translucent_growing, bench_mixed_growing,
        bench_solid_shrinking, bench_translucent_shrinking, bench_mixed_shrinking,
        bench_solid_quad, bench_translucent_quad,
        bench_quad_solid_stacked, bench_quad_translucent_stacked, bench_quad_mixed_stacked,
        bench_quad_solid_growing, bench_quad_translucent_growing, bench_quad_mixed_growing,
        bench_quad_solid_shrinking, bench_quad_translucent_shrinking, bench_quad_mixed_shrinking
}

criterion_main!(renderer_benches);
