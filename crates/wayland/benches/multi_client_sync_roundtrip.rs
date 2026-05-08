
use criterion::{Criterion};
use std::env;
use std::io::Write;
use std::os::unix::net::UnixStream;
use wayland_protocols::wire;

pub fn bench_multi_client_sync_roundtrip(c: &mut Criterion) {
    // Retrieve Socket Path
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

    let mut group = c.benchmark_group("compositor:: multi client");

    let number_of_clients: Vec<usize> = vec![2, 5, 10, 25];

    for clients in number_of_clients {
        let mut streams = Vec::with_capacity(clients);
        // Creating + Adding the stream to the streams: vector<stream>
        for _ in 0..clients {
            let stream =
                UnixStream::connect(&socket_path).expect("Unable to connect to wayland socket");
            streams.push(stream);
        }

        // This sets the input size of the benchmark group
        group.throughput(criterion::Throughput::Elements(clients as u64));

        let mut next_ids = vec![2u32; clients];

        // now after creating socket connections
        // We will perform roundtrip calls on each client
        group.bench_with_input("sync roundtrip connections", &clients, |b, _| {
            // routine : have to return the total duration taken by the program execution
            b.iter_custom(|iters| {
                let mut total_duration = std::time::Duration::ZERO;

                for _ in 0..iters {
                    let t0 = std::time::Instant::now();

                    let mut callback_ids = Vec::with_capacity(clients);

                    // Phase 1: send one sync from each client.
                    for i in 0..clients {
                        let callback_id = next_ids[i];
                        next_ids[i] += 1;

                        let mut frame = Vec::new();
                        wire::send_msg(&mut frame, 1, 0, &callback_id.to_le_bytes()).unwrap();
                        streams[i].write_all(&frame).unwrap();
                        callback_ids.push(callback_id);
                    }

                    // Phase 2: drain until each client sees its callback::done.
                    for i in 0..clients {
                        loop {
                            let (obj_id, opcode, _body) = wire::recv_msg(&mut streams[i]).unwrap();
                            if obj_id == callback_ids[i] && opcode == 0 {
                                break;
                            }
                        }
                    }

                    total_duration += t0.elapsed();
                }

                total_duration
            });
        });
    }
    group.finish();
}