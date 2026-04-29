#![allow(unused_variables, unused_mut, dead_code)]

use std::io;
use tracing::{debug, info};
use wayland_protocols::connection::Connection;
use wayland_protocols::object::Object;
use wayland_protocols::wl_callback::SyncCallback;
use wayland_protocols::wl_display::Display;
use wayland_protocols::wl_registry::Registry;
use wayland_protocols::wl_shm::{ShmHandler, alloc_shm_file};
use wayland_protocols::wlr_layer_shell::{LayerShell, LayerSurface};

use wayland_protocols::*;

// The following acts as a placeholder for the case where no output is found. The protocol requires a valid output object or the null object,
// but we don't have any outputs to bind to in this example, so we use this dummy object that always returns 4 as its ID.
struct NullObject;

impl Object for NullObject {
    fn object_id(&self) -> u32 {
        4
    }
}

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
    let (layer_name, layer_ver) = registry
        .find("zwlr_layer_shell_v1")
        .expect("zwlr_layer_shell_v1 missing");
    let output_name = registry.find("wl_output").map(|(name, _)| name);

    let compositor = WlCompositor::new(conn.alloc_id());
    let shm_inner = WlShm::new(conn.alloc_id());
    let layer_shell_inner = ZwlrLayerShellV1::new(conn.alloc_id());
    let output = output_name.map(|_| WlOutput::new(conn.alloc_id()));

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
    registry.inner.bind(
        &mut conn,
        layer_name,
        "zwlr_layer_shell_v1",
        layer_ver.min(4),
        &layer_shell_inner,
    )?;
    if let (Some(name), Some(output_obj)) = (output_name, output.as_ref()) {
        registry
            .inner
            .bind(&mut conn, name, "wl_output", 1, output_obj)?;
    }

    let mut shm = ShmHandler { inner: shm_inner };
    let mut layer_shell = LayerShell::new(layer_shell_inner);

    const WIDTH: i32 = 640;
    const HEIGHT: i32 = 480;
    let stride = WIDTH * 4;
    let size = (stride * HEIGHT) as usize;

    let fd = alloc_shm_file(size, [0x88, 0x77, 0x44, 0x00])?;

    let pool_obj = WlShmPool::new(conn.alloc_id());
    let mut buffer = WlBuffer::new(conn.alloc_id());
    let mut surface = WlSurface::new(conn.alloc_id());
    let layer_surface_inner = ZwlrLayerSurfaceV1::new(conn.alloc_id());

    shm.inner
        .create_pool(&mut conn, &pool_obj, fd, size as i32)?;
    pool_obj.create_buffer(&mut conn, &buffer, 0, WIDTH, HEIGHT, stride, 1)?;
    compositor.create_surface(&mut conn, &surface)?;

    // zwlr_layer_shell_v1 uses fixed uint values from the protocol enum.
    const LAYER_OVERLAY: u32 = 3;
    const LAYER_BACKGROUND: u32 = 0;
    const LAYER_BOTTOM: u32 = 1;
    const LAYER_TOP: u32 = 2;
    const ANCHOR_TOP: u32 = 1;
    const ANCHOR_BOTTOM: u32 = 2;
    const ANCHOR_LEFT: u32 = 4;
    const ANCHOR_RIGHT: u32 = 8;

    match output.as_ref() {
        Some(output_obj) => layer_shell.inner.get_layer_surface(
            &mut conn,
            &layer_surface_inner,
            &surface,
            output_obj,
            LAYER_OVERLAY,
            "basic-overlay",
        )?,
        None => {
            println!("no output found, asking to create a layer surface without output");
            layer_shell.inner.get_layer_surface(
                &mut conn,
                &layer_surface_inner,
                &surface,
                &NullObject,
                LAYER_OVERLAY,
                "basic-overlay",
            )?
        }
    }

    let mut layer_surface = LayerSurface::new(layer_surface_inner);
    layer_surface
        .inner
        .set_anchor(&mut conn, ANCHOR_TOP | ANCHOR_BOTTOM)?;
    layer_surface
        .inner
        .set_size(&mut conn, WIDTH as u32, HEIGHT as u32)?;
    layer_surface.inner.set_exclusive_zone(&mut conn, -1)?;
    surface.commit(&mut conn)?;
    conn.flush()?;

    info!("surface created, entering event loop");

    let mut attached = false;
    loop {
        let (obj_id, opcode, body) = conn.recv_msg()?;
        debug!(obj_id, opcode, "event");
        dispatch_to!(conn, obj_id, opcode, &body;
            display, registry, shm, layer_shell, layer_surface, surface, buffer);

        if let Some(serial) = layer_surface.pending_ack.take() {
            layer_surface.inner.ack_configure(&mut conn, serial)?;
            if !attached {
                surface.attach(&mut conn, &buffer, 0, 0)?;
                surface.damage(&mut conn, 0, 0, WIDTH, HEIGHT)?;
                surface.commit(&mut conn)?;
                attached = true;
                info!("buffer attached, overlay layer surface should be visible");
            }
        }
        if layer_surface.closed {
            info!("layer surface closed");
            break;
        }
        conn.flush()?;
    }

    Ok(())
}
