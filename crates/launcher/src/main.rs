#![allow(unused_variables, unused_mut, dead_code)]
use anyhow::Result;
use launcher::{profile_function, profile_scope};
use renderer::primitives::RenderablePrimitive as _;
use renderer::{GpuImage, Image, Quad, Rect as RenderRect, Renderer, TextMetrics, TextSystem};
use std::time::{Duration, Instant};
use utils::asset_manager::AssetManager;
use utils::font::FontAsset;
use utils::image::ImageAsset;
use wayland_protocols::connection::Connection;
use wayland_protocols::wl_callback::SyncCallback;
use wayland_protocols::wl_display::Display;
use wayland_protocols::wl_registry::Registry;
use wayland_protocols::xdg_surface::XdgSurf;
use wayland_protocols::xdg_toplevel::Toplevel;
use wayland_protocols::xdg_wm_base::WmBase;
use wayland_protocols::zwp_linux_dmabuf::DmaBuf;
use wayland_protocols::*;

use layout::{
    AlignItems, Dimension, Display as FlexDisplay, Edges, FlexDirection, JustifyContent, Layout,
    LengthPercentage, LengthPercentageAuto, Measure, Position, Rect as LayoutRect, Size, Style,
};

struct Widget {
    measured: Option<TextMetrics>,
}

impl Measure for Widget {
    fn measure(
        &self,
        _known: Size<Option<f32>>,
        _available: Size<layout::AvailableSpace>,
    ) -> Size<f32> {
        match &self.measured {
            Some(m) => Size {
                width: m.width,
                height: m.height(),
            },
            None => Size::ZERO,
        }
    }
}

fn no_measure() -> Widget {
    Widget { measured: None }
}
fn measured(m: TextMetrics) -> Widget {
    Widget { measured: Some(m) }
}

fn to_rect(r: LayoutRect) -> RenderRect {
    RenderRect {
        x: r.x,
        y: r.y,
        w: r.w,
        h: r.h,
    }
}

fn main() -> Result<()> {
    profile_function!();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    #[cfg(feature = "profile")]
    let _puffin_server = {
        puffin::set_scopes_on(true);
        let server_addr = format!("127.0.0.1:{}", puffin_http::DEFAULT_PORT);
        match puffin_http::Server::new(&server_addr) {
            Ok(server) => {
                eprintln!("Puffin HTTP server running on {server_addr}");
                Some(server)
            }
            Err(e) => {
                eprintln!("Failed to start Puffin server: {e}");
                None
            }
        }
    };

    // ── Wayland setup ─────────────────────────────────────────────────────────

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
    let mut dmabuf = DmaBuf::new(dmabuf_inner);

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

    const WIDTH: u32 = 1028;
    const HEIGHT: u32 = 1080;

    let mut renderer = Renderer::new(WIDTH, HEIGHT)?;
    renderer.register::<Quad>()?;
    renderer.register::<renderer::MonoSprite>()?;
    renderer.register::<Image>()?;

    let mut assets = AssetManager::new();
    let font_handle = assets.load::<FontAsset, _>("assets/Inter-Regular.ttf")?;
    let logo_handle = assets.load::<ImageAsset, _>("assets/logo.png")?;

    let mut text_sys = TextSystem::new(renderer.gl(), 1024)?;
    let font_id = text_sys.load_font(&assets.get(&font_handle).unwrap().data)?;

    assets
        .process_pending(&mut renderer.image_processor())
        .into_iter()
        .collect::<anyhow::Result<Vec<_>>>()?;

    let logo = logo_handle.get_processed::<GpuImage>(&assets).unwrap();
    let logo_w = logo.width as f32;
    let logo_h = logo.height as f32;
    let logo_tex = logo.id();

    // ── Layout (computed once at startup) ─────────────────────────────────────

    let label_m = text_sys.measure_text("Welcome", font_id, 36.0);
    let button_m = text_sys.measure_text("Get Started", font_id, 18.0);

    let (mut layout, (logo_id, label_id, button_id)) = Layout::new(
        Style {
            display: FlexDisplay::Flex,
            flex_direction: FlexDirection::Column,
            justify_content: Some(JustifyContent::Center),
            align_items: Some(AlignItems::Center),
            size: Size {
                width: Dimension::percent(1.0),
                height: Dimension::percent(1.0),
            },
            ..Default::default()
        },
        no_measure(),
        |b| {
            let logo = b.leaf(
                Style {
                    position: Position::Absolute,
                    inset: Edges {
                        top: LengthPercentageAuto::length(20.0),
                        right: LengthPercentageAuto::length(20.0),
                        bottom: LengthPercentageAuto::auto(),
                        left: LengthPercentageAuto::auto(),
                    },
                    size: Size {
                        width: Dimension::length(logo_w),
                        height: Dimension::length(logo_h),
                    },
                    ..Default::default()
                },
                no_measure(),
            );

            let (_, (label, button)) = b.child(
                Style {
                    display: FlexDisplay::Flex,
                    flex_direction: FlexDirection::Column,
                    align_items: Some(AlignItems::Center),
                    gap: Size {
                        width: LengthPercentage::length(0.0),
                        height: LengthPercentage::length(24.0),
                    },
                    ..Default::default()
                },
                no_measure(),
                |b| {
                    let label = b.leaf(Default::default(), measured(label_m));

                    let button = b.leaf(
                        Style {
                            padding: Edges {
                                top: LengthPercentage::length(12.0),
                                right: LengthPercentage::length(28.0),
                                bottom: LengthPercentage::length(12.0),
                                left: LengthPercentage::length(28.0),
                            },
                            ..Default::default()
                        },
                        measured(button_m),
                    );

                    (label, button)
                },
            );

            (logo, label, button)
        },
    );

    layout.compute(LayoutRect {
        x: 0.0,
        y: 0.0,
        w: WIDTH as f32,
        h: HEIGHT as f32,
    });

    let mut scene = renderer.create_scene();
    let render_surface = renderer.create_dmabuf_surface();
    let mut configured = false;
    let mut wl_buf: Option<WlBuffer> = None;

    let mut frame_count = 0u64;
    let mut last_fps_report = Instant::now();

    loop {
        #[cfg(feature = "profile")]
        puffin::GlobalProfiler::lock().new_frame();

        profile_scope!("event_loop");

        while let Some((obj_id, opcode, body)) = conn.try_recv_msg()? {
            dispatch_to!(conn, obj_id, opcode, &body;
                display, registry, dmabuf, wm_base, xdg_surf, toplevel, surface);
        }

        if let Some(serial) = wm_base.pending_pong.take() {
            wm_base.inner.pong(&mut conn, serial)?;
        }

        if let Some(serial) = xdg_surf.pending_ack.take() {
            xdg_surf.inner.ack_configure(&mut conn, serial)?;
            configured = true;
        }

        if configured {
            if wl_buf.is_none() {
                profile_scope!("dmabuf_setup");
                let frame = renderer.present()?;
                let params = ZwpLinuxBufferParamsV1::new(conn.alloc_id());
                dmabuf.inner.create_params(&mut conn, &params)?;
                let mod_hi = (frame.modifier >> 32) as u32;
                let mod_lo = frame.modifier as u32;
                params.add(
                    &mut conn,
                    frame.fd,
                    0,
                    frame.offset,
                    frame.stride,
                    mod_hi,
                    mod_lo,
                )?;
                let buf = WlBuffer::new(conn.alloc_id());
                params.create_immed(
                    &mut conn,
                    &buf,
                    WIDTH as i32,
                    HEIGHT as i32,
                    frame.format,
                    0,
                )?;
                params.destroy(&mut conn)?;
                wl_buf = Some(buf);
            }

            let buf = wl_buf.as_ref().unwrap();

            {
                profile_scope!("render");

                scene.clear_primitives();
                scene.background = (0.97, 0.97, 0.97); // light — logo and text are dark

                // ── Logo (top-right) ──────────────────────────────────────────
                let lr = layout.rect(logo_id);
                Image {
                    bounds: to_rect(lr),
                    texture: logo_tex,
                    clip_rect: None,
                }
                .add_to_scene(&mut scene);

                // ── Label ("Welcome") ─────────────────────────────────────────
                // rect is exactly the text bounding box; origin is baseline-left
                let tr = layout.rect(label_id);
                let lm = layout.data(label_id).measured.as_ref().unwrap();
                text_sys.draw_text(
                    &mut scene,
                    renderer.gl(),
                    "Welcome",
                    font_id,
                    36.0,
                    [0.1, 0.1, 0.1, 1.0],
                    [tr.x, tr.y + lm.ascent],
                )?;

                // ── Button ────────────────────────────────────────────────────
                let br = layout.rect(button_id);
                Quad {
                    bounds: to_rect(br),
                    color: [0.18, 0.46, 0.96, 1.0],
                    clip_rect: None,
                }
                .add_to_scene(&mut scene);

                // center button text within the button rect
                let bm = layout.data(button_id).measured.as_ref().unwrap();
                text_sys.draw_text(
                    &mut scene,
                    renderer.gl(),
                    "Get Started",
                    font_id,
                    18.0,
                    [1.0, 1.0, 1.0, 1.0],
                    [
                        br.x + (br.w - bm.width) / 2.0,
                        br.y + (br.h - bm.height()) / 2.0 + bm.ascent,
                    ],
                )?;

                renderer.begin_frame(&render_surface, scene.background);
                renderer.render_primitive::<Quad>(&scene, &render_surface)?;
                renderer.render_primitive::<renderer::MonoSprite>(&scene, &render_surface)?;
                renderer.render_primitive::<Image>(&scene, &render_surface)?;
                renderer.end_frame();
            }

            {
                profile_scope!("surface_commit");
                surface.attach(&mut conn, buf, 0, 0)?;
                surface.damage(&mut conn, 0, 0, WIDTH as i32, HEIGHT as i32)?;
                surface.commit(&mut conn)?;
            }

            frame_count += 1;
            let now = Instant::now();
            let since_last = now.duration_since(last_fps_report);
            if since_last >= Duration::from_secs(60) {
                let fps = frame_count as f64 / since_last.as_secs_f64();
                tracing::info!(fps = format!("{:.1}", fps), "FPS report");
                frame_count = 0;
                last_fps_report = now;
            }
        }

        if toplevel.closed {
            tracing::info!("window closed");
            break;
        }

        conn.flush()?;
    }

    if let Some(buf) = wl_buf {
        buf.destroy(&mut conn)?;
        conn.flush()?;
    }

    Ok(())
}
