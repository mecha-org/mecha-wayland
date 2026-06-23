use app::prelude::*;
use io_ring::{Ring, RingSettings};
use window_manager::WindowManager;

#[derive(State)]
pub struct Launcher {
    window_manager: WindowManager,
    ring: Ring,
}

impl Launcher {
    pub fn new() -> Self {
        let ring = Ring::new(RingSettings::default());
        let window_manager = WindowManager::new(ring.proxy());

        Launcher {
            ring,
            window_manager,
        }
    }
}

fn main() {
    let mut app = App::new(Launcher::new())
        .mount(window_manager::module())
        .mount(io_ring::module());

    app.dispatch(&app::Start);
    loop {
        app.dispatch(&app::PrePoll);
        app.dispatch(&app::Poll);
    }
}
