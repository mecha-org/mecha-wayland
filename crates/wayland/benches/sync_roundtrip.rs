use criterion::Criterion;
use std::env;
use std::io::Write;
use std::os::unix::net::UnixStream;
use wayland_protocols::wire;

pub fn bench_sync_roundtrip(c: &mut Criterion) {
    let socket_path = {
        let rt = env::var("XDG_RUNTIME_DIR")
            .expect("XDG_RUNTIME_DIR not set — is a Wayland compositor running?");
        let disp = env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
        if disp.starts_with('/') {
            disp
        } else {
            format!("{}/{}", rt, disp)
        }
    };

    // One persistent connection across all samples — matches real app behaviour.
    let mut stream = UnixStream::connect(&socket_path)
        .expect("could not connect to Wayland compositor — is one running?");

    // Start object IDs above wl_display (1); each sync allocates one callback.
    let mut next_id: u32 = 2;

    c.bench_function("display::sync roundtrip", |b| {
        // iter_custom lets the benchmark own timing,
        // so only the latency of the IPC round-trip (not Criterion's own overhead) is counted
        b.iter_custom(|iters| {
            let mut total = std::time::Duration::ZERO;
            for _ in 0..iters {
                let callback_id = next_id;
                next_id += 1;

                // wl_display::sync — object_id=1, opcode=0, body=[new_id: u32]
                let mut frame = Vec::new();
                wire::send_msg(&mut frame, 1, 0, &callback_id.to_le_bytes()).unwrap();

                let t0 = std::time::Instant::now();
                stream.write_all(&frame).unwrap();

                // Drain events until wl_callback::done (opcode=0) for our id.
                // Other events (e.g. wl_display::delete_id) are skipped.
                loop {
                    let (obj_id, opcode, _body) = wire::recv_msg(&mut stream).unwrap();
                    if obj_id == callback_id && opcode == 0 {
                        break;
                    }
                }
                total += t0.elapsed();
            }
            total
        });
    });
}
