#![allow(unused_variables, unused_mut, dead_code)]

use std::io;
use tracing::{debug, info};
use wayland_protocols::connection::Connection;
use wayland_protocols::wl_callback::SyncCallback;
use wayland_protocols::wl_display::Display;
use wayland_protocols::wl_pointer::Pointer;
use wayland_protocols::wl_registry::Registry;
use wayland_protocols::wl_seat::Seat;
use wayland_protocols::wl_shm::{ShmHandler, alloc_shm_file};
use wayland_protocols::wl_touch::Touch;
use wayland_protocols::xdg_surface::XdgSurf;
use wayland_protocols::xdg_toplevel::Toplevel;
use wayland_protocols::xdg_wm_base::WmBase;

use wayland_protocols::*;

fn main() -> io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let mut conn = Connection::connect()?;

    let mut display = Display::new(1);
    let mut registry = Registry::new(conn.alloc_id());
    let mut sync = SyncCallback::new(conn.alloc_id());

    display.inner.get_registry(&mut conn, &registry.inner)?;
    display.inner.sync(&mut conn, &sync)?;
    conn.flush()?;

    info!("waiting for globals");

    loop {
        let (obj_id, opcode, body) = conn.recv_msg()?;
        debug!(obj_id, opcode, bytes = body.len(), "event received");
        dispatch_to!(conn, obj_id, opcode, &body; display, registry, sync);
        if sync.done {
            break;
        }
    }

    info!("sync complete, binding globals");

    let (comp_name, comp_ver) = registry
        .find("wl_compositor")
        .expect("wl_compositor missing");
    let (shm_name, shm_ver) = registry.find("wl_shm").expect("wl_shm missing");
    let (xdg_name, _) = registry.find("xdg_wm_base").expect("xdg_wm_base missing");
    let (seat_name, seat_ver) = registry.find("wl_seat").expect("wl_seat missing");

    let compositor = WlCompositor::new(conn.alloc_id());
    let shm_inner = WlShm::new(conn.alloc_id());
    let wm_inner = XdgWmBase::new(conn.alloc_id());

    registry.inner.bind(
        &mut conn,
        comp_name,
        "wl_compositor",
        comp_ver.min(4),
        &compositor,
    )?;
    registry
        .inner
        .bind(&mut conn, shm_name, "wl_shm", shm_ver.min(1), &shm_inner)?;
    registry
        .inner
        .bind(&mut conn, xdg_name, "xdg_wm_base", 1, &wm_inner)?;

    let mut seat = Seat::new(conn.alloc_id());

    registry.inner.bind(
        &mut conn,
        seat_name,
        "wl_seat",
        seat_ver.min(7),
        &seat.inner,
    )?;

    let mut shm = ShmHandler { inner: shm_inner };
    let mut wm_base = WmBase::new(wm_inner);

    const WIDTH: i32 = 640;
    const HEIGHT: i32 = 480;
    let stride = WIDTH * 4;
    let size = (stride * HEIGHT) as usize;

    let fd = alloc_shm_file(size, [0x88, 0x77, 0x44, 0x00])?;

    let pool_obj = WlShmPool::new(conn.alloc_id());
    let mut buffer = WlBuffer::new(conn.alloc_id());
    let mut surface = WlSurface::new(conn.alloc_id());
    let xdg_inner = XdgSurface::new(conn.alloc_id());
    let top_inner = XdgToplevel::new(conn.alloc_id());

    shm.inner
        .create_pool(&mut conn, &pool_obj, fd, size as i32)?;
    pool_obj.create_buffer(
        &mut conn,
        &buffer,
        0,
        WIDTH,
        HEIGHT,
        stride,
        WlShmFormat::Xrgb8888,
    )?;
    compositor.create_surface(&mut conn, &surface)?;
    wm_base
        .inner
        .get_xdg_surface(&mut conn, &xdg_inner, &surface)?;

    let mut xdg_surf = XdgSurf::new(xdg_inner);
    let mut toplevel = Toplevel::new(top_inner);

    xdg_surf.inner.get_toplevel(&mut conn, &toplevel.inner)?;
    toplevel.inner.set_title(&mut conn, "hello wayland")?;
    toplevel
        .inner
        .set_app_id(&mut conn, "basic-wayland-client")?;
    surface.commit(&mut conn)?;
    conn.flush()?;

    info!("surface created, entering event loop");

    let mut attached = false;

    let mut pointer: Option<Pointer> = None;
    let mut touch: Option<Touch> = None;

    loop {
        let (obj_id, opcode, body) = conn.recv_msg()?;
        debug!(obj_id, opcode, "event");

        // Dispatch to dynamically attached devices like pointer, keyboard, etc.
        let handled = dispatch_optional!(conn, obj_id, opcode, &body; pointer, touch);

        // If it wasn't handled by optional devices, dispatch to the global objects
        if !handled {
            dispatch_to!(conn, obj_id, opcode, &body;
                display, registry, shm, wm_base, xdg_surf, toplevel, surface, buffer, seat);
        }

        if seat.has_pointer && pointer.is_none() {
            let new_pointer = Pointer::new(conn.alloc_id());
            seat.inner.get_pointer(&mut conn, &new_pointer.inner)?;
            pointer = Some(new_pointer);

            info!("wl_pointer connected and bound");
        } else if !seat.has_pointer && pointer.is_some() {
            if let Some(p) = &pointer {
                p.inner.release(&mut conn).unwrap();
            }
            pointer = None;
            info!("wl_pointer disconnected and released");
        }

        if seat.has_touch && touch.is_none() {
            let new_touch = Touch::new(conn.alloc_id());
            seat.inner.get_touch(&mut conn, &new_touch.inner)?;
            touch = Some(new_touch);
            info!("wl_touch connected and bound");
        } else if !seat.has_touch && touch.is_some() {
            if let Some(t) = &touch {
                t.inner.release(&mut conn).unwrap();
            }
            touch = None;
            info!("wl_touch disconnected and released");
        }

        if let Some(serial) = wm_base.pending_pong.take() {
            wm_base.inner.pong(&mut conn, serial)?;
        }
        if let Some(serial) = xdg_surf.pending_ack.take() {
            xdg_surf.inner.ack_configure(&mut conn, serial)?;
            if !attached {
                surface.attach(&mut conn, &buffer, 0, 0)?;
                surface.damage(&mut conn, 0, 0, WIDTH, HEIGHT)?;
                surface.commit(&mut conn)?;
                attached = true;
                info!("buffer attached, window should be visible");
            }
        }
        if toplevel.closed {
            info!("window closed");
            break;
        }
        conn.flush()?;
    }

    Ok(())
}
