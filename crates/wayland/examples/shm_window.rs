use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::ptr;

use app::prelude::*;
use app::{Poll, PrePoll, RegisteredModule, Start};
use io_ring::{Ring, RingSettings};
use wayland::{
    Handle, Interface, Wayland, WlBuffer, WlCallbackEvent, WlCompositor, WlRegistryEvent, WlShell,
    WlShellSurface, WlShellSurfaceEvent, WlShm, WlShmFormat,
};

const WIDTH: i32 = 640;
const HEIGHT: i32 = 480;
const STRIDE: i32 = WIDTH * 4;
// XRGB8888 sky blue: X=0x00 R=0x5B G=0x9F B=0xD0
const COLOR: u32 = 0x005B_9FD0;

#[derive(State)]
struct ShmWindow {
    ring: Ring,
    wayland: Wayland,
    compositor: Option<Handle<WlCompositor>>,
    shm: Option<Handle<WlShm>>,
    shell: Option<Handle<WlShell>>,
    shell_surface: Option<Handle<WlShellSurface>>,
    buffer: Option<Handle<WlBuffer>>,
}

impl ShmWindow {
    fn new() -> Self {
        let ring = Ring::new(RingSettings::default());
        let wayland = Wayland::new(ring.proxy());
        Self {
            ring,
            wayland,
            compositor: None,
            shm: None,
            shell: None,
            shell_surface: None,
            buffer: None,
        }
    }

    fn start(&mut self) {
        let display = self.wayland.display();
        display.get_registry();
        display.sync();
    }

    fn on_global(&mut self, ev: &WlRegistryEvent) {
        let WlRegistryEvent::Global {
            sender,
            name,
            interface,
            version,
        } = ev
        else {
            return;
        };
        match interface.as_str() {
            WlCompositor::NAME => self.compositor = Some(sender.bind(*name, *version)),
            WlShm::NAME => self.shm = Some(sender.bind(*name, *version)),
            WlShell::NAME => self.shell = Some(sender.bind(*name, *version)),
            _ => {}
        }
    }

    fn on_done(&mut self) {
        let (Some(compositor), Some(shm), Some(shell)) = (&self.compositor, &self.shm, &self.shell)
        else {
            return;
        };

        let surface = compositor.create_surface();
        let shell_surface = shell.get_shell_surface(&surface);
        shell_surface.set_toplevel();

        let buffer = alloc_shm_buffer(shm);
        surface.attach(Some(&buffer), 0, 0);
        surface.damage(0, 0, WIDTH, HEIGHT);
        surface.commit();

        self.shell_surface = Some(shell_surface);
        self.buffer = Some(buffer);
    }

    fn pong(&self, serial: u32) {
        if let Some(ss) = &self.shell_surface {
            ss.pong(serial);
        }
    }
}

fn alloc_shm_buffer(shm: &Handle<WlShm>) -> Handle<WlBuffer> {
    let size = (STRIDE * HEIGHT) as usize;

    let fd: OwnedFd = unsafe {
        let raw = libc::memfd_create(c"shm_window".as_ptr(), libc::MFD_CLOEXEC);
        assert!(raw >= 0, "memfd_create failed");
        assert_eq!(libc::ftruncate(raw, size as i64), 0, "ftruncate failed");
        OwnedFd::from_raw_fd(raw)
    };

    unsafe {
        let ptr = libc::mmap(
            ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd.as_raw_fd(),
            0,
        );
        assert!(ptr != libc::MAP_FAILED, "mmap failed");
        let pixels = std::slice::from_raw_parts_mut(ptr as *mut u32, size / 4);
        pixels.fill(COLOR);
        libc::munmap(ptr, size);
    }

    let pool = shm.create_pool(fd.as_fd(), size as i32);
    let buffer = pool.create_buffer(0, WIDTH, HEIGHT, STRIDE, WlShmFormat::Xrgb8888);
    pool.destroy();
    buffer
}

fn module<S>() -> impl RegisteredModule<ShmWindow, S> {
    Module::new()
        .mount(wayland::module::<S>().into_module())
        .on(|s: &mut ShmWindow, _: &Start| s.start())
        .on(|s: &mut ShmWindow, ev: &WlRegistryEvent| s.on_global(ev))
        .on(|s: &mut ShmWindow, _: &WlCallbackEvent| s.on_done())
        .on(|s: &mut ShmWindow, ev: &WlShellSurfaceEvent| {
            if let WlShellSurfaceEvent::Ping { serial, .. } = ev {
                s.pong(*serial);
            }
        })
        .on(|s: &mut ShmWindow, _: &PrePoll| s.wayland.proxy().flush())
}

fn main() {
    let mut app = App::new(ShmWindow::new())
        .mount(module())
        .mount(io_ring::module());

    app.dispatch(&Start);
    loop {
        app.dispatch(&PrePoll);
        app.dispatch(&Poll);
    }
}
