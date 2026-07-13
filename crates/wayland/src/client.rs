use std::env;
use std::os::fd::IntoRawFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use app::{Many, RegisteredModule, prelude::*};
use io_ring::{IoEvent, RingProxy};

use crate::{IoCompletion, ObjectId, RawWaylandEvent, Wayland, handle_io_event, make_inner, proto};

#[cfg(feature = "client")]
use crate::WlDisplayEvent;

impl Wayland {
    pub fn new(ring_proxy: RingProxy) -> Self {
        let xdg_runtime_dir =
            env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".into());
        let wayland_display = env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
        let path = PathBuf::from(xdg_runtime_dir).join(wayland_display);

        let stream = UnixStream::connect(&path).expect("failed to connect to wayland socket");
        stream.set_nonblocking(true).unwrap();
        let fd = stream.into_raw_fd();

        let data = make_inner(fd, ring_proxy, 2);
        data.borrow_mut().submit_read();
        Wayland { data }
    }
}

pub fn module<S>() -> impl app::RegisteredModule<Wayland, S> {
    let m = Module::<Wayland, _, _>::new()
        .on(|wayland: &mut Wayland, io_event: &IoEvent| {
            let IoEvent::Completed { token, result } = io_event;
            let events = match handle_io_event(&wayland.data, *token, *result) {
                IoCompletion::Read(events) => {
                    wayland.data.borrow_mut().submit_read();
                    events
                }
                IoCompletion::Disconnect => panic!("wayland socket error"),
                _ => vec![],
            };
            Many(events.into_iter())
        })
        .mount(proto::manual::client_module::<S>().into_module())
        .mount(proto::generated::client_module::<S>().into_module());

    #[cfg(feature = "client")]
    let m = m.on(|wayland: &mut Wayland, event: &WlDisplayEvent| {
        if let WlDisplayEvent::DeleteId { id, .. } = event {
            wayland.invalidate_object(ObjectId(*id));
        }
        None::<RawWaylandEvent>
    });

    m
}
