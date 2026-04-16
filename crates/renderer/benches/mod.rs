use criterion::{Criterion, criterion_group, criterion_main};

mod lavender;
use lavender::bench_lavender_render;

criterion_group! {
    name = renderer_benches;
    config = Criterion::default();
    targets = bench_lavender_render
}

criterion_main!(renderer_benches);
