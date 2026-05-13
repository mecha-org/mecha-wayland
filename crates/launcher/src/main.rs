use anyhow::Result;
use glow::HasContext;
use renderer::commands::{ClearColor, DrawMonochromeSprite, DrawQuad, DrawRect, DrawText};
use renderer::{DmaBuf, Renderer};

mod atlas {
    include!(concat!(env!("OUT_DIR"), "/ui_gen.rs"));
}
use std::os::unix::io::OwnedFd;
use std::time::Instant;
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
enum SlotState {
    Free,
    InFlight,
}

struct Slot {
    surf: renderer::RenderableSurface<DmaBuf>,
    wl_buf: WlBuffer,
    state: SlotState,
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn make_wl_buffer(
    conn: &mut Connection,
    dmabuf: &WlDmaBuf,
    surf: &renderer::RenderableSurface<DmaBuf>,
) -> Result<WlBuffer> {
    let params = ZwpLinuxBufferParamsV1::new(conn.alloc_id());
    dmabuf.inner.create_params(conn, &params)?;

    let fd: OwnedFd = surf
        .backend
        .prime_fd
        .try_clone()
        .map_err(|e| anyhow::anyhow!("dup prime_fd: {e}"))?;
    let modifier = surf.backend.modifier;
    params.add(
        conn,
        fd,
        0,
        0,
        surf.backend.stride,
        (modifier >> 32) as u32,
        (modifier & 0xffff_ffff) as u32,
    )?;

    let wl_buf = WlBuffer::new(conn.alloc_id());
    params.create_immed(
        conn,
        &wl_buf,
        WIDTH as i32,
        HEIGHT as i32,
        DRM_FORMAT_ARGB8888,
        0,
    )?;

    Ok(wl_buf)
}

struct AppState {
    counter: Counter,
}

// -- Counter Module --
#[derive(Default)]
pub struct Counter {
    count: u32,
}

#[derive(Debug, Clone)]
pub struct CounterIncremented;

#[derive(Debug, Clone)]
pub struct CounterDecremented;

impl app::event::Event for CounterDecremented {}
impl app::event::Event for CounterIncremented {}

macro_rules! counter_module {
    () => {
        app::module::Module::new()
            .on(|c: &mut Counter, _: &CounterDecremented| {
                if c.count > 1 {
                    c.count -= 1;
                }
            })
            .on(|c: &mut Counter, _: &CounterIncremented| {
                c.count += 1;
            })
    };
}
// -- Counter Module Ends --

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let state = AppState {
        counter: Counter::default(),
    };
    let mut app = app::App::new(state).register_module(|app| &mut app.counter, counter_module!());

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

    // ── Renderer setup ────────────────────────────────────────────────────

    let mut renderer = Renderer::new()?;
    renderer.init_command_queue::<ClearColor>();
    renderer.init_command_queue::<DrawRect>();
    renderer.init_command_queue::<DrawQuad>();
    renderer.init_command_queue::<DrawMonochromeSprite>();
    renderer.init_command_queue::<DrawText>();

    let icon_tex = renderer.upload_atlas(atlas::UI.png_bytes)?;

    let surf_a = renderer.create_surface::<DmaBuf>(WIDTH, HEIGHT)?;
    let surf_b = renderer.create_surface::<DmaBuf>(WIDTH, HEIGHT)?;

    let wl_buf_a = make_wl_buffer(&mut conn, &dmabuf, &surf_a)?;
    let wl_buf_b = make_wl_buffer(&mut conn, &dmabuf, &surf_b)?;
    conn.flush()?;

    let mut slots = [
        Slot {
            surf: surf_a,
            wl_buf: wl_buf_a,
            state: SlotState::Free,
        },
        Slot {
            surf: surf_b,
            wl_buf: wl_buf_b,
            state: SlotState::Free,
        },
    ];

    let mut configured = false;
    let mut fps_frame_count: u32 = 0;
    let mut fps_timer = Instant::now();
    let mut counter_timer = Instant::now();

    loop {
        while let Some((obj_id, opcode, body)) = conn.try_recv_msg()? {
            for slot in slots.iter_mut() {
                if obj_id == slot.wl_buf.object_id() && opcode == 0 {
                    slot.state = SlotState::Free;
                }
            }

            dispatch_to!(conn, obj_id, opcode, &body;
                display, registry, wm_base, xdg_surf, toplevel, surface);
        }

        if counter_timer.elapsed().as_secs() >= 1 {
            app.dispatch(CounterIncremented);
            counter_timer = Instant::now();
        }

        if let Some(serial) = wm_base.pending_pong.take() {
            wm_base.inner.pong(&mut conn, serial)?;
        }

        if let Some(serial) = xdg_surf.pending_ack.take() {
            xdg_surf.inner.ack_configure(&mut conn, serial)?;
            configured = true;
        }

        if configured {
            if let Some(slot) = slots.iter_mut().find(|s| s.state == SlotState::Free) {
                // BeginFrame
                unsafe {
                    renderer
                        .gl
                        .bind_framebuffer(glow::FRAMEBUFFER, Some(slot.surf.fbo));
                    renderer
                        .gl
                        .viewport(0, 0, slot.surf.width as i32, slot.surf.height as i32);
                }
                renderer.set_width(slot.surf.width);
                renderer.set_height(slot.surf.height);

                renderer.send_command(ClearColor {
                    r: 0.32,
                    g: 0.32,
                    b: 0.32,
                    a: 1.0,
                });

                // Card
                renderer.send_command(DrawQuad {
                    color: (0.16, 0.16, 0.18, 1.0),
                    border_color: (0.30, 0.30, 0.35, 1.0),
                    origin: (314.0, 360.0, 0.0),
                    size: (400.0, 360.0),
                    border_radius: 20.0,
                    border_thickness: 2.0,
                });

                // Title
                renderer.send_command(DrawText {
                    font: &atlas::UI_FONT_INTER_24,
                    texture_id: icon_tex,
                    text: "Counter".to_string(),
                    origin: (330.0, 408.0, 0.5),
                    color: (1.0, 1.0, 1.0, 1.0),
                });

                // Count value
                renderer.send_command(DrawText {
                    font: &atlas::UI_FONT_INTER_100,
                    texture_id: icon_tex,
                    text: format!("{}", app.state.counter.count),
                    origin: (464.0, 548.0, 0.5),
                    color: (1.0, 1.0, 1.0, 1.0),
                });

                // Minus button
                renderer.send_command(DrawQuad {
                    color: (0.2, 0.4, 0.9, 1.0),
                    border_color: (0.4, 0.6, 1.0, 1.0),
                    origin: (374.0, 598.0, 1.0),
                    size: (110.0, 52.0),
                    border_radius: 12.0,
                    border_thickness: 2.0,
                });
                renderer.send_command(DrawText {
                    font: &atlas::UI_FONT_INTER_24,
                    texture_id: icon_tex,
                    text: "-".to_string(),
                    origin: (419.0, 634.0, 0.4),
                    color: (1.0, 1.0, 1.0, 1.0),
                });

                // Plus button
                renderer.send_command(DrawQuad {
                    color: (0.2, 0.7, 0.3, 1.0),
                    border_color: (0.4, 0.9, 0.5, 1.0),
                    origin: (544.0, 598.0, 1.0),
                    size: (110.0, 52.0),
                    border_radius: 12.0,
                    border_thickness: 2.0,
                });
                renderer.send_command(DrawText {
                    font: &atlas::UI_FONT_INTER_24,
                    texture_id: icon_tex,
                    text: "+".to_string(),
                    origin: (589.0, 634.0, 0.5),
                    color: (1.0, 1.0, 1.0, 1.0),
                });

                // EndFrame
                renderer.process_command_queue::<ClearColor>();
                renderer.process_command_queue::<DrawRect>();
                renderer.process_command_queue::<DrawQuad>();
                renderer.process_command_queue::<DrawMonochromeSprite>();
                unsafe {
                    renderer.gl.finish();
                }

                surface.attach(&mut conn, &slot.wl_buf, 0, 0)?;
                surface.damage(&mut conn, 0, 0, WIDTH as i32, HEIGHT as i32)?;
                surface.commit(&mut conn)?;

                slot.state = SlotState::InFlight;
                fps_frame_count += 1;
                let elapsed = fps_timer.elapsed();
                if elapsed.as_secs_f32() >= 1.0 {
                    tracing::info!(fps = fps_frame_count, "FPS");
                    fps_frame_count = 0;
                    fps_timer = Instant::now();
                }
            }
        }

        if toplevel.closed {
            tracing::info!("window closed");
            break;
        }

        conn.flush()?;
    }

    // ── Cleanup ───────────────────────────────────────────────────────────

    let [slot_a, slot_b] = slots;
    renderer.destroy_surface(slot_a.surf);
    renderer.destroy_surface(slot_b.surf);

    Ok(())
}
