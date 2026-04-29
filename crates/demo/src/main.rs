use io_runtime::{event::EventManager, ring::Ring};
use std::io;
use wayland::{
    connection::Connection, wl_callback::SyncCallback, wl_display::Display, wl_registry::Registry,
};
// use wayland_protocols::{
//     connection::Connection, wl_callback::SyncCallback, wl_display::Display, wl_registry::Registry,
// };

// #[derive(Clone)]
// struct WindowCreated {
//     pub id: String,
// }

pub struct Runtime {
    io: Ring,
    conn: Connection,
    display: Display,
    registry: Registry,
}

impl Runtime {
    pub fn new() -> io::Result<Self> {
        let mut io = Ring::new()?;
        let mut conn = Connection::connect(&mut io)?;
        let display = Display::new(1);
        let registry = Registry::new(conn.alloc_id());

        Ok(Self {
            io,
            conn,
            display,
            registry,
        })
    }

    pub fn run() -> io::Result<()> {
        let mut rt = Self::new()?;
        let conn = &mut rt.conn;
        let io = &mut rt.io;
        let display = &mut rt.display;
        let registry = &mut rt.registry;

        // request for globals
        let sync = SyncCallback::new(conn.alloc_id());
        display.inner.get_registry(conn, io, &registry.inner)?;
        display.inner.sync(conn, io, &sync)?;

        loop {
            println!("Waiting for events...");
            let len = io.wait_and_dispatch(1)?;
            println!("Dispatched {len} events");

            let msg = conn.drain(io, len)?;
        }

        Ok(())
    }
}

fn main() -> io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("demo=debug".parse().unwrap()),
        )
        .init();

    let ev = EventManager::new().unwrap();

    if let Err(e) = Runtime::run() {
        eprintln!("fatal: {e}");
        std::process::exit(1);
    }

    Ok(())
}
