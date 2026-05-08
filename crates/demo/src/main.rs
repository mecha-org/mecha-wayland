use io_runtime::{event::EventManager, ring::Ring};
use std::io;
use wayland::{
    connection::Connection,
    object::{self, WlObjectProxy},
    registry::ObjectRegistry,
    wl_callback::WlCallback,
    wl_display::{WlDisplay, WlDisplayProxy},
    wl_registry::{WlRegistry, WlRegistryProxy},
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
    display: WlDisplayProxy,
    registry: WlRegistryProxy,
    objects: ObjectRegistry,
}

impl Runtime {
    pub fn new() -> io::Result<Self> {
        let mut io = Ring::new()?;
        let conn = Connection::connect(&mut io)?;
        let mut objects = ObjectRegistry::new();

        let display = objects.create::<WlDisplay>();
        let registry = objects.create::<WlRegistry>();

        Ok(Self {
            io,
            conn,
            display,
            registry,
            objects,
        })
    }

    pub fn run() -> io::Result<()> {
        let mut rt = Self::new()?;
        let conn = &mut rt.conn;
        let io = &mut rt.io;
        let display = &mut rt.display;
        let registry = rt.registry;
        let objects = &mut rt.objects;

        // request for globals
        let sync = objects.create::<WlCallback>();
        display.get_registry(conn, io, registry.object_id())?;
        display.sync(conn, io, &sync)?;

        loop {
            println!("Waiting for events...");
            let len = io.wait_and_dispatch(1)?;
            println!("Dispatched {len} events");

            let events = conn.drain(io, len)?;
            objects.dispatch_all(events)?;

            if sync.on_done().is_some() {
                break;
            }
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
