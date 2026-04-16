#![allow(unused_variables, unused_mut, dead_code)]
use anyhow::Result;
use glow::HasContext;
use renderer::{DmaBuf, Renderer};
use std::os::unix::io::OwnedFd;
use wayland_protocols::connection::Connection;
use wayland_protocols::object::Object as _;
use wayland_protocols::wl_callback::SyncCallback;
use wayland_protocols::wl_display::Display;
use wayland_protocols::wl_registry::Registry;
use wayland_protocols::xdg_surface::XdgSurf;
use wayland_protocols::xdg_toplevel::Toplevel;
use wayland_protocols::xdg_wm_base::WmBase;
use wayland_protocols::zwp_linux_dmabuf::DmaBuf as WlDmaBuf;
use wayland_protocols::*;

const WIDTH: u32 = 1028;
const HEIGHT: u32 = 1080;
const DRM_FORMAT_ARGB8888: u32 = 0x34325241;

// ── Slot: one swap buffer (surface + wl_buffer + state) ──────────────────

#[derive(PartialEq)]
enum SlotState { Free, InFlight }

struct Slot {
    surf:   renderer::RenderableSurface<DmaBuf>,
    wl_buf: WlBuffer,
    state:  SlotState,
}

// ── Helpers ───────────────────────────────────────────────────────────────

/// Create a `wl_buffer` bound to a `RenderableSurface<DmaBuf>` via zwp_linux_dmabuf.
/// The prime_fd is dup'd so the surface retains its own copy.
fn make_wl_buffer(
    conn:   &mut Connection,
    dmabuf: &WlDmaBuf,
    surf:   &renderer::RenderableSurface<DmaBuf>,
) -> Result<WlBuffer> {
    let params = ZwpLinuxBufferParamsV1::new(conn.alloc_id());
    dmabuf.inner.create_params(conn, &params)?;

    let fd: OwnedFd = surf.backend.prime_fd.try_clone()
        .map_err(|e| anyhow::anyhow!("dup prime_fd: {e}"))?;
    let modifier = surf.backend.modifier;
    params.add(
        conn,
        fd,
        0,                          // plane_idx
        0,                          // offset
        surf.backend.stride,
        (modifier >> 32) as u32,    // modifier_hi
        (modifier & 0xffff_ffff) as u32, // modifier_lo
    )?;

    let wl_buf = WlBuffer::new(conn.alloc_id());
    params.create_immed(conn, &wl_buf, WIDTH as i32, HEIGHT as i32, DRM_FORMAT_ARGB8888, 0)?;

    Ok(wl_buf)
}

/// Clear the surface's FBO with lavender and finish before handing to compositor.
fn render_lavender(renderer: &Renderer, surf: &renderer::RenderableSurface<DmaBuf>) {
    unsafe {
        renderer.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(surf.fbo));
        renderer.gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
        renderer.gl.clear_color(0.902, 0.902, 0.980, 1.0); // #E6E6FA
        renderer.gl.clear(glow::COLOR_BUFFER_BIT);
        renderer.gl.finish(); // block until GPU done — compositor reads immediately after commit
    }
}

// ── Main ──────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // ── Wayland setup ─────────────────────────────────────────────────────

    let mut conn = Connection::connect()?;

    let mut display = Display::new(1);
    let mut registry = Registry::new(conn.alloc_id());
    let mut sync = SyncCallback::new(conn.alloc_id());

    display.inner.get_registry(&mut conn, &registry.inner)?;
    display.inner.sync(&mut conn, &sync)?;
    conn.flush()?;

    loop {
        let (obj_id, opcode, body) = conn.recv_msg()?;
        dispatch_to!(conn, obj_id, opcode, &body; display, registry, sync);
        if sync.done {
            break;
        }
    }

    let (comp_name, comp_ver) = registry
        .find("wl_compositor")
        .expect("wl_compositor missing");
    let (xdg_name, _) = registry.find("xdg_wm_base").expect("xdg_wm_base missing");
    let (dmabuf_name, dmabuf_ver) = registry
        .find("zwp_linux_dmabuf_v1")
        .expect("zwp_linux_dmabuf_v1 missing");

    let compositor = WlCompositor::new(conn.alloc_id());
    let wm_inner = XdgWmBase::new(conn.alloc_id());
    let dmabuf_inner = ZwpLinuxDmabufV1::new(conn.alloc_id());

    registry.inner.bind(
        &mut conn,
        comp_name,
        "wl_compositor",
        comp_ver.min(4),
        &compositor,
    )?;
    registry
        .inner
        .bind(&mut conn, xdg_name, "xdg_wm_base", 1, &wm_inner)?;
    registry.inner.bind(
        &mut conn,
        dmabuf_name,
        "zwp_linux_dmabuf_v1",
        dmabuf_ver.min(4),
        &dmabuf_inner,
    )?;

    let mut wm_base = WmBase::new(wm_inner);
    let dmabuf = WlDmaBuf::new(dmabuf_inner);

    let mut surface = WlSurface::new(conn.alloc_id());
    let xdg_inner = XdgSurface::new(conn.alloc_id());
    let top_inner = XdgToplevel::new(conn.alloc_id());

    compositor.create_surface(&mut conn, &surface)?;
    wm_base
        .inner
        .get_xdg_surface(&mut conn, &xdg_inner, &surface)?;

    let mut xdg_surf = XdgSurf::new(xdg_inner);
    let mut toplevel = Toplevel::new(top_inner);

    xdg_surf.inner.get_toplevel(&mut conn, &toplevel.inner)?;
    toplevel.inner.set_title(&mut conn, "Mecha Launcher")?;
    toplevel.inner.set_app_id(&mut conn, "mecha-launcher")?;
    surface.commit(&mut conn)?;
    conn.flush()?;

    // ── Renderer + swap chain setup ───────────────────────────────────────

    let renderer = Renderer::new()?;

    // Two DMA-BUF surfaces for double-buffering — managed entirely in main.
    let surf_a = renderer.create_surface::<DmaBuf>(WIDTH, HEIGHT)?;
    let surf_b = renderer.create_surface::<DmaBuf>(WIDTH, HEIGHT)?;

    // Bind each surface to a persistent wl_buffer.
    let wl_buf_a = make_wl_buffer(&mut conn, &dmabuf, &surf_a)?;
    let wl_buf_b = make_wl_buffer(&mut conn, &dmabuf, &surf_b)?;
    conn.flush()?;

    let mut slots = [
        Slot { surf: surf_a, wl_buf: wl_buf_a, state: SlotState::Free },
        Slot { surf: surf_b, wl_buf: wl_buf_b, state: SlotState::Free },
    ];

    let mut presented = false;

    // ── Event loop ────────────────────────────────────────────────────────

    loop {
        while let Some((obj_id, opcode, body)) = conn.try_recv_msg()? {
            // Check for wl_buffer.release (opcode 0) on any in-flight slot.
            for slot in slots.iter_mut() {
                if obj_id == slot.wl_buf.object_id() && opcode == 0 {
                    tracing::info!(buf_id = obj_id, "wl_buffer released — slot now free");
                    slot.state = SlotState::Free;
                }
            }

            dispatch_to!(conn, obj_id, opcode, &body;
                display, registry, wm_base, xdg_surf, toplevel, surface);
        }

        if let Some(serial) = wm_base.pending_pong.take() {
            wm_base.inner.pong(&mut conn, serial)?;
        }

        if let Some(serial) = xdg_surf.pending_ack.take() {
            xdg_surf.inner.ack_configure(&mut conn, serial)?;

            if !presented {
                // Acquire first free slot and render lavender into it.
                let slot = slots
                    .iter_mut()
                    .find(|s| s.state == SlotState::Free)
                    .expect("no free slot on first frame");

                render_lavender(&renderer, &slot.surf);

                surface.attach(&mut conn, &slot.wl_buf, 0, 0)?;
                surface.damage(&mut conn, 0, 0, WIDTH as i32, HEIGHT as i32)?;
                surface.commit(&mut conn)?;

                slot.state = SlotState::InFlight;
                presented = true;
                tracing::info!("lavender frame committed");
            }
        }

        if toplevel.closed {
            tracing::info!("window closed");
            break;
        }

        conn.flush()?;
    }

    // ── Cleanup ───────────────────────────────────────────────────────────

    // Consume slots to get owned surfaces back for proper cleanup.
    let [slot_a, slot_b] = slots;
    renderer.destroy_surface(slot_a.surf);
    renderer.destroy_surface(slot_b.surf);

    Ok(())
}
