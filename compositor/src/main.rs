use std::time::Instant;

use app::{Poll, PrePoll, Start, prelude::*};
use io_ring::{Ring, RingSettings};
use wayland::{ClientConnected, WaylandServer, WlShmFormat};

mod client_window;
mod protocols;
mod rect;

use crate::protocols::wl_seat::WlSeatState;
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
    seat: WlSeatState,
    xdg_shell: XdgShellState,
    client_window: ClientWindow,
    start_time: Instant,
}

// TO REMOVE: CPU blit.
fn blit(compositor: &mut Compositor, ev: &SurfaceCommitted) {
    let now = compositor.start_time.elapsed().as_millis() as u32;

    let (buf_id, prev_buf_id) = {
        let surface = match compositor.surfaces.surfaces.get_mut(&ev.surface_id) {
            Some(s) => s,
            None => return,
        };
        let buf_id = match surface.current.buffer {
            Some(id) => id,
            None => return,
        };
        let prev_id = surface.previous_buffer.take();
        (buf_id, prev_id)
    };

    // TO REMOVE: mmap DMA-BUF for CPU write.
    let (src_ptr, src_stride, src_width_bytes, src_height, xrgb) = {
        let shm_buf = match compositor.shm.buffers.get(&buf_id) {
            Some(b) => b,
            None => return,
        };
        // TO REMOVE: capture buffer dimensions for hit-testing.
        if let Some(surf) = compositor.surfaces.surfaces.get_mut(&ev.surface_id) {
            surf.current.buffer_width = shm_buf.width;
            surf.current.buffer_height = shm_buf.height;
        }
        (
            shm_buf.ptr.as_ptr() as *const u8,
            shm_buf.stride as usize,
            shm_buf.width as usize * 4,
            shm_buf.height as usize,
            matches!(shm_buf.format, WlShmFormat::Xrgb8888),
        )
    };

    // TO REMOVE: CPU blit tears without release tracking.
    if compositor.client_window.is_back_released() {
        // TO REMOVE: mmap.
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

        // TO REMOVE: CPU copy.
        for row in 0..copy_height {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    src_ptr.add(row * src_stride),
                    dst_ptr.add(row * dst_stride),
                    copy_width,
                );
            }
            // TO REMOVE: XRGB alpha stamp.
            if xrgb {
                let row_base = unsafe { dst_ptr.add(row * dst_stride) };
                for px in 0..copy_width / 4 {
                    unsafe {
                        *row_base.add(px * 4 + 3) = 0xFF;
                    }
                }
            }
        }

        // TO REMOVE: CPU fill.
        let row_bytes = dst_stride;
        for row in 0..dst_height as usize {
            let row_start = unsafe { dst_ptr.add(row * row_bytes) };
            if row < copy_height {
                let fill_start = unsafe { row_start.add(copy_width) };
                let fill_len = row_bytes.saturating_sub(copy_width);
                if fill_len > 0 {
                    let pixels = unsafe {
                        std::slice::from_raw_parts_mut(fill_start as *mut u32, fill_len / 4)
                    };
                    pixels.fill(0xFF262626u32);
                }
            } else {
                let pixels =
                    unsafe { std::slice::from_raw_parts_mut(row_start as *mut u32, row_bytes / 4) };
                pixels.fill(0xFF262626u32);
            }
        }

        // TO REMOVE: munmap.
        unsafe {
            libc::munmap(dst_ptr as *mut _, dst_size);
        }

        compositor.client_window.commit_blitted_frame();
    }

    if let Some(prev_id) = prev_buf_id {
        if let Some(buf) = compositor.shm.buffers.get(&prev_id) {
            buf.handle.release();
        }
    }

    if let Some(surf) = compositor.surfaces.surfaces.get_mut(&ev.surface_id) {
        surf.fire_frame_callbacks(now);
    }
}

fn main() {
    let ring = Ring::new(RingSettings::default());
    let server = WaylandServer::new("wayland-2", ring.proxy());
    let client_window = ClientWindow::new(ring.proxy(), 1080, 1240, "compositor");
    let start_time = Instant::now();

    let mut app = App::new(Compositor {
        server,
        ring,
        registry: WlRegistryState::new(),
        shm: WlShmState::new(),
        surfaces: SurfaceState::new(),
        seat: WlSeatState::new(),
        xdg_shell: XdgShellState::default(),
        client_window,
        start_time,
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
    .mount(protocols::wl_seat::module())
    .mount(protocols::wl_pointer::module())
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
