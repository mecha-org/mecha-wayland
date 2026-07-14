use app::{Poll, PrePoll, Start, prelude::*};
use io_ring::{Ring, RingSettings};
use wayland::{ClientConnected, WaylandServer};

mod client_window;
mod protocols;
mod rect;

use client_window::ClientWindow;
use protocols::wl_registry::WlRegistryState;
use protocols::wl_shm::WlShmState;
use protocols::wl_surface::{SurfaceCommitted, SurfaceState};
use protocols::xdg_shell::XdgShellState;

#[derive(State)]
struct Compositor {
    server: WaylandServer,
    ring: Ring,
    registry: WlRegistryState,
    shm: WlShmState,
    surfaces: SurfaceState,
    xdg_shell: XdgShellState,
    client_window: ClientWindow,
}

fn blit(compositor: &mut Compositor, ev: &SurfaceCommitted) {
    let (buf_id, prev_buf_id, _frame_callbacks) = {
        let surface = match compositor.surfaces.surfaces.get_mut(&ev.surface_id) {
            Some(s) => s,
            None => return,
        };
        let buf_id = match surface.current.buffer {
            Some(id) => id,
            None => return,
        };
        let prev_id = surface.previous_buffer.take();
        let frames: Vec<_> = surface.current.frame_callbacks.drain(..).collect();
        (buf_id, prev_id, frames)
    };

    let (src_ptr, src_stride, src_width_bytes, src_height) = {
        let shm_buf = match compositor.shm.buffers.get(&buf_id) {
            Some(b) => b,
            None => return,
        };
        (
            shm_buf.ptr.as_ptr() as *const u8,
            shm_buf.stride as usize,
            shm_buf.width as usize * 4,
            shm_buf.height as usize,
        )
    };

    let (prime_fd, dst_stride, _, dst_height) = compositor.client_window.back_buffer_info();
    let dst_stride = dst_stride as usize;
    let dst_size = dst_stride * dst_height as usize;

    let dst_ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            dst_size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            prime_fd,
            0,
        )
    };
    if dst_ptr == libc::MAP_FAILED {
        eprintln!("blit: mmap of DMA-BUF failed");
        return;
    }
    let dst_ptr = dst_ptr as *mut u8;
    let copy_width = src_width_bytes.min(dst_stride);
    let copy_height = src_height.min(dst_height as usize);
    for row in 0..copy_height {
        unsafe {
            std::ptr::copy_nonoverlapping(
                src_ptr.add(row * src_stride),
                dst_ptr.add(row * dst_stride),
                copy_width,
            );
        }
    }
    unsafe {
        libc::munmap(dst_ptr as *mut _, dst_size);
    }

    if let Some(prev_id) = prev_buf_id {
        if let Some(buf) = compositor.shm.buffers.get(&prev_id) {
            buf.handle.release();
        }
    }

    compositor.client_window.commit_blitted_frame();
}

fn main() {
    let ring = Ring::new(RingSettings::default());
    let server = WaylandServer::new("wayland-2", ring.proxy());
    let client_window = ClientWindow::new(ring.proxy(), 1080, 1240, "compositor");

    let mut app = App::new(Compositor {
        server,
        ring,
        registry: WlRegistryState::new(),
        shm: WlShmState::new(),
        surfaces: SurfaceState::new(),
        xdg_shell: XdgShellState::default(),
        client_window,
    })
    .mount(wayland::server_module())
    .mount(io_ring::module())
    .mount(client_window::module())
    .mount(protocols::wl_display::module())
    .mount(protocols::wl_registry::module())
    .mount(protocols::wl_callback::module())
    .mount(protocols::wl_compositor::module())
    .mount(protocols::wl_shm::module())
    .mount(protocols::wl_region::module())
    .mount(protocols::wl_surface::module())
    .mount(protocols::xdg_shell::module())
    .mount(Module::<Compositor, _, _>::new().on(
        |compositor: &mut Compositor, ev: &SurfaceCommitted| {
            blit(compositor, ev);
            hlist![]
        },
    ))
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
