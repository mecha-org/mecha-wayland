use app::{Poll, PrePoll, Start, prelude::*};
use io_ring::{Ring, RingSettings};
use wayland::{ClientConnected, WaylandServer};

mod client_window;
mod protocols;

use client_window::ClientWindow;
use protocols::wl_registry::WlRegistryState;

#[derive(State)]
struct Compositor {
    server: WaylandServer,
    ring: Ring,
    registry: WlRegistryState,
    client_window: ClientWindow,
}

fn main() {
    let ring = Ring::new(RingSettings::default());
    let server = WaylandServer::new("wayland-2", ring.proxy());
    let client_window = ClientWindow::new(ring.proxy(), 1080, 1240, "compositor");

    let mut app = App::new(Compositor {
        server,
        ring,
        registry: WlRegistryState::new(),
        client_window,
    })
    .mount(wayland::server_module())
    .mount(io_ring::module())
    .mount(client_window::module())
    .mount(protocols::wl_display::module())
    .mount(protocols::wl_registry::module())
    .mount(protocols::wl_callback::module())
    .mount(protocols::wl_compositor::module())
    .mount(Module::<Compositor, _, _>::new().on(
        |_: &mut Compositor, event: &ClientConnected| {
            println!("client connected: {:?}", event.id);
            hlist![]
        },
    ));

    app.dispatch(&Start);
    loop {
        app.dispatch(&PrePoll);
        app.dispatch(&Poll);
    }
}
