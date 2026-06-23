use app::{RegisteredModule, prelude::State};
use io_ring::{IoEvent, IoToken, RingProxy};
use std::{
    marker::PhantomData,
    os::{fd::IntoRawFd, unix::net::UnixStream},
    path::PathBuf,
};
use wayland::Wayland;

#[derive(State)]
pub struct WindowManager {
    wayland: Wayland,
}

impl WindowManager {
    pub fn new(ring_proxy: RingProxy) -> Self {
        let wayland = Wayland::new(ring_proxy.clone());
        Self { wayland }
    }
    pub fn start(&mut self) {
        let display = self.wayland.display();
        let _registry = display.get_registry();
        let _callback = display.sync();
    }
    pub fn pre_poll(&mut self) {
        self.wayland.proxy().flush();
    }
    pub fn poll(&mut self) {}
}

pub struct Window<T> {
    _phantom: PhantomData<T>,
}

pub fn module<S>() -> impl app::RegisteredModule<WindowManager, S> {
    app::Module::new()
        .mount(wayland::module::<S>().into_module())
        .on(|window_manager: &mut WindowManager, _: &app::Start| window_manager.start())
        .on(|window_manager: &mut WindowManager, _: &app::PrePoll| window_manager.pre_poll())
        .on(|window_manager: &mut WindowManager, _: &app::Poll| window_manager.poll())
        .on(
            |window_manager: &mut WindowManager, event: &wayland::WlDisplayEvent| {
                println!("{:?}", event);
            },
        )
        .on(
            |window_manager: &mut WindowManager, event: &wayland::WlRegistryEvent| {
                println!("{:?}", event);
            },
        )
        .on(
            |window_manager: &mut WindowManager, event: &wayland::WlCallbackEvent| {
                println!("{:?}", event);
            },
        )
}
