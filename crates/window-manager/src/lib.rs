mod connection;

use connection::Connection;
use io_ring::{IoEvent, IoToken, RingProxy};
use std::{
    marker::PhantomData,
    os::{fd::IntoRawFd, unix::net::UnixStream},
    path::PathBuf,
};

pub struct WindowManager {
    ring_proxy: RingProxy,
    connection: Connection,
}

impl WindowManager {
    fn wayland_socket_path() -> PathBuf {
        let xdg_runtime_dir =
            std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".into());
        let wayland_display =
            std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
        PathBuf::from(xdg_runtime_dir).join(wayland_display)
    }

    pub fn new(ring_proxy: RingProxy) -> Self {
        let path = Self::wayland_socket_path();
        let connection = Connection::new(path);

        Self {
            ring_proxy,
            connection,
        }
    }

    pub fn start(&mut self) {
        println!("Init was called!");
    }

    pub fn pre_poll(&mut self) {}

    pub fn poll(&mut self) {}
}

pub struct Window<T> {
    _phantom: PhantomData<T>,
}

pub fn module<S>() -> impl app::RegisteredModule<WindowManager, S> {
    app::Module::new()
        .on(|window_manager: &mut WindowManager, _: &app::Start| window_manager.start())
        .on(|window_manager: &mut WindowManager, _: &app::PrePoll| window_manager.pre_poll())
        .on(|window_manager: &mut WindowManager, _: &app::Poll| window_manager.poll())
}
