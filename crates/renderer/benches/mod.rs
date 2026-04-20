use criterion::{Criterion, criterion_group, criterion_main};

mod draw_rect;
mod lavender;
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
        bench_solid_shrinking, bench_translucent_shrinking, bench_mixed_shrinking
}

criterion_main!(renderer_benches);
