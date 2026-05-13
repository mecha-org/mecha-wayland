#![recursion_limit = "4096"]

mod ring;
mod wayland;

use ring::{Ring, ring_module};

#[derive(Default)]
struct AppState {
    ring: Ring,
    counter: Counter,
}

#[derive(Default)]
struct Counter {
    count: u32,
}

fn main() {
    let state = AppState::default();
    let mut app = app::App::new(state)
        .register_module(|s| &mut s.ring, ring_module!(1))
        .register_module(
            |app| &mut app.counter,
            app::module::Module::new().on(|counter: &mut Counter, event: &ring::IoEvent| {
                if matches!(event, ring::IoEvent::EventOne) {
                    counter.count += 1;
                    println!("Counter: {}", counter.count);
                }
            }),
        );

    app.run();
}
