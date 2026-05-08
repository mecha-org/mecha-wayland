use criterion::{Criterion, black_box};
use std::io::Cursor;
use wayland_protocols::wire;

// The pure CPU cost of decoding one Wayland wire-protocol message from a buffer already in memory
pub fn bench_parse_wire_message(c: &mut Criterion) {
    let mut body = Vec::new();

    body.extend_from_slice(&1u32.to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&6u32.to_le_bytes());
    body.extend_from_slice(b"hello\0\0\0");

    // Build the frame using the real send_msg from wire.rs.
    let mut frame = Vec::new();
    wire::send_msg(&mut frame, 1, 0, &body).unwrap();

    c.bench_function("event_loop_iteration: parse_wire_message", |b| {
        // Use the real recv_msg from wire.rs with a Cursor as the Read source.
        b.iter(|| wire::recv_msg(&mut Cursor::new(black_box(&frame))))
    });
}
