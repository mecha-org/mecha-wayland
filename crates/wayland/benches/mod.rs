use criterion::{Criterion, criterion_group, criterion_main};

pub mod frame;
pub mod multi_client_sync_roundtrip;
pub mod parse_wire_message;
pub mod sync_roundtrip;

use frame::bench_attached_surface_event_loop;
use multi_client_sync_roundtrip::bench_multi_client_sync_roundtrip;
use parse_wire_message::bench_parse_wire_message;
use sync_roundtrip::bench_sync_roundtrip;

criterion_group! {
    name = wayland_protocols_benches;
    config = Criterion::default();
    targets =
    bench_parse_wire_message,
    bench_sync_roundtrip,
    bench_multi_client_sync_roundtrip,
    bench_attached_surface_event_loop
}

criterion_main!(wayland_protocols_benches);
