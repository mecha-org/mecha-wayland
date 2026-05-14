#![recursion_limit = "4096"]

mod ring;
mod timer;

use app::event::Event;
use ring::Ring;
use timer::{Timer, TimerEvents, TimerSettings};

struct AppState {
    ring: Ring,
    timer: Timer,
    counter: Counter,
}

impl AppState {
    fn new() -> Self {
        let ring = Ring::default();
        let timer = Timer::new(ring.get_proxy());

        Self {
            ring,
            timer,
            counter: Counter::default(),
        }
    }
}

#[derive(Default)]
struct Counter {
    count: u32,
}

#[derive(Clone, Copy, Debug)]
pub enum CounterEvent {
    Updated { new_count: u32 },
}

impl Event for CounterEvent {}

fn main() {
    let state = AppState::new();

    let mut app = app::App::new(state)
        .register_module(
            |s| &mut s.timer,
            app::module::Module::new().on(|s: &mut Timer, _e: &app::Start| {
                println!("App started");
                let timer_id = s.start_timer(TimerSettings {
                    duration: std::time::Duration::from_secs(5),
                });
                println!("Timer {} started", timer_id);
            }),
        )
        .register_module(|s| &mut s.ring, register_ring!(1))
        .register_module(|s| &mut s.timer, register_timer!())
        .register_module(
            |s| &mut s.counter,
            app::module::Module::new().processor(
                |counter: &mut Counter, event: &TimerEvents| -> Option<CounterEvent> {
                    match event {
                        TimerEvents::TimerFinished { id } => {
                            counter.count += 1;
                            println!("Timer {} finished! Total fired: {}", id, counter.count);

                            // Emit the state change event
                            Some(CounterEvent::Updated {
                                new_count: counter.count,
                            })
                        }
                    }
                },
            ),
        )
        .register_module(
            |s| &mut s.timer,
            app::module::Module::new().on(|timer: &mut Timer, event: &CounterEvent| {
                match event {
                    CounterEvent::Updated { new_count } => {
                        println!(
                            "Counter reached {}. Spawning a follow-up timer...",
                            new_count
                        );

                        // Start a new timer based on the counter's state
                        timer.start_timer(TimerSettings {
                            duration: std::time::Duration::from_secs(2),
                        });
                    }
                }
            }),
        );

    app.run();
}
