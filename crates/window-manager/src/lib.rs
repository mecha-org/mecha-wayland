use app::{RegisteredModule, prelude::State};
use io_ring::RingProxy;
use std::{
    marker::PhantomData,
    os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
    ptr,
};
use wayland::{Handle, Interface, *};

const BAR_HEIGHT: u32 = 30;
const BAR_COLOR: u32 = 0x00_1E_1E_2E;

#[derive(Default)]
pub struct WaylandGlobals {
    compositor: Option<Handle<WlCompositor>>,
    shm: Option<Handle<WlShm>>,
    output: Option<Handle<WlOutput>>,
    layer_shell: Option<Handle<ZwlrLayerShellV1>>,
    surface: Option<Handle<WlSurface>>,
    buffer: Option<Handle<WlBuffer>>,
}

#[derive(State)]
pub struct WindowManager {
    wayland: Wayland,
    globals: WaylandGlobals,
    windows_initialised: bool,
}

impl WindowManager {
    pub fn new(ring_proxy: RingProxy) -> Self {
        let wayland = Wayland::new(ring_proxy.clone());
        Self {
            wayland,
            globals: WaylandGlobals::default(),
            windows_initialised: false,
        }
    }
    pub fn start(&mut self) {
        let display = self.wayland.display();
        display.get_registry();
        display.sync();
    }
    pub fn pre_poll(&mut self) {
        self.wayland.proxy().flush();
    }
    pub fn poll(&mut self) {}
}

fn alloc_shm_buffer(shm: &Handle<WlShm>, width: u32, height: u32) -> Handle<WlBuffer> {
    let stride = width * 4;
    let size = (stride * height) as usize;

    let fd: OwnedFd = unsafe {
        let raw = libc::memfd_create(c"wm_status_bar".as_ptr(), libc::MFD_CLOEXEC);
        assert!(raw >= 0, "memfd_create failed");
        assert_eq!(libc::ftruncate(raw, size as i64), 0, "ftruncate failed");
        OwnedFd::from_raw_fd(raw)
    };

    unsafe {
        let p = libc::mmap(
            ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd.as_raw_fd(),
            0,
        );
        assert!(p != libc::MAP_FAILED, "mmap failed");
        let pixels = std::slice::from_raw_parts_mut(p as *mut u32, size / 4);
        pixels.fill(BAR_COLOR);
        libc::munmap(p, size);
    }

    let pool = shm.create_pool(fd.as_fd(), size as i32);
    let buffer = pool.create_buffer(0, width as i32, height as i32, stride as i32, WlShmFormat::Xrgb8888);
    pool.destroy();
    buffer
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
            |_window_manager: &mut WindowManager, event: &wayland::WlDisplayEvent| {
                println!("{:?}", event);
            },
        )
        .on(
            |window_manager: &mut WindowManager, event: &wayland::WlRegistryEvent| {
                if let wayland::WlRegistryEvent::Global { sender, name, interface, version } = event {
                    match interface.as_str() {
                        WlCompositor::NAME => {
                            window_manager.globals.compositor = Some(sender.bind(*name, *version))
                        }
                        WlShm::NAME => {
                            window_manager.globals.shm = Some(sender.bind(*name, *version))
                        }
                        ZwlrLayerShellV1::NAME => {
                            window_manager.globals.layer_shell = Some(sender.bind(*name, *version))
                        }
                        WlOutput::NAME => {
                            window_manager.globals.output = Some(sender.bind(*name, *version))
                        }
                        _ => {}
                    }
                }
            },
        )
        .on(
            |window_manager: &mut WindowManager, _: &wayland::WlCallbackEvent| {
                if window_manager.windows_initialised {
                    return;
                }

                let (compositor, layer_shell) = {
                    let g = &window_manager.globals;
                    match (g.compositor.clone(), g.layer_shell.clone()) {
                        (Some(c), Some(ls)) => (c, ls),
                        _ => return,
                    }
                };

                let surface = compositor.create_surface();
                let layer_surface = layer_shell.get_layer_surface(
                    &surface,
                    None,
                    ZwlrLayerShellV1Layer::Top,
                    "mechanix-status-bar",
                );

                layer_surface.set_size(0, BAR_HEIGHT);
                layer_surface.set_anchor(
                    ZwlrLayerSurfaceV1Anchor::Top
                        | ZwlrLayerSurfaceV1Anchor::Left
                        | ZwlrLayerSurfaceV1Anchor::Right,
                );
                layer_surface.set_exclusive_zone(BAR_HEIGHT as i32);
                surface.commit();

                window_manager.globals.surface = Some(surface);
            },
        )
        .on(
            |window_manager: &mut WindowManager, event: &wayland::ZwlrLayerSurfaceV1Event| {
                if let wayland::ZwlrLayerSurfaceV1Event::Configure { sender, serial, width, height } = event {
                    let (surface, shm) = {
                        let g = &window_manager.globals;
                        match (g.surface.clone(), g.shm.clone()) {
                            (Some(s), Some(shm)) => (s, shm),
                            _ => return,
                        }
                    };

                    let w = if *width == 0 { 1920 } else { *width };
                    let h = if *height == 0 { BAR_HEIGHT } else { *height };

                    sender.ack_configure(*serial);

                    let buffer = alloc_shm_buffer(&shm, w, h);
                    surface.attach(Some(&buffer), 0, 0);
                    surface.damage(0, 0, w as i32, h as i32);
                    surface.commit();

                    window_manager.globals.buffer = Some(buffer);
                    window_manager.windows_initialised = true;
                }
            },
        )
}
