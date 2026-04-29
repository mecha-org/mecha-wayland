use criterion::Criterion;
use std::io;
use wayland_protocols::connection::Connection;
use wayland_protocols::wl_callback::SyncCallback;
use wayland_protocols::wl_display::Display;
use wayland_protocols::wl_registry::Registry;
use wayland_protocols::wl_shm::{ShmHandler, alloc_shm_file};
use wayland_protocols::xdg_surface::XdgSurf;
use wayland_protocols::xdg_toplevel::Toplevel;
use wayland_protocols::xdg_wm_base::WmBase;
use wayland_protocols::*;

const WIDTH: i32 = 640;
const HEIGHT: i32 = 480;

struct FrameBenchState {
    conn: Connection,
    display: Display,
    registry: Registry,
    shm: ShmHandler,
    wm_base: WmBase,
    xdg_surf: XdgSurf,
    toplevel: Toplevel,
    surface: WlSurface,
    buffer: WlBuffer,
}

impl FrameBenchState {
    fn setup() -> io::Result<Self> {
        let mut conn = Connection::connect()?;

        let mut display = Display::new(1);
        let mut registry = Registry::new(conn.alloc_id());
        let mut sync = SyncCallback::new(conn.alloc_id());

        display.inner.get_registry(&mut conn, &registry.inner)?;
        display.inner.sync(&mut conn, &sync)?;
        conn.flush()?;

        while !sync.done {
            let (obj_id, opcode, body) = conn.recv_msg()?;
            dispatch_to!(conn, obj_id, opcode, &body; display, registry, sync);
        }

        let (comp_name, comp_ver) = registry
            .find("wl_compositor")
            .ok_or_else(|| io::Error::other("wl_compositor missing"))?;
        let (shm_name, shm_ver) = registry
            .find("wl_shm")
            .ok_or_else(|| io::Error::other("wl_shm missing"))?;
        let (xdg_name, _) = registry
            .find("xdg_wm_base")
            .ok_or_else(|| io::Error::other("xdg_wm_base missing"))?;

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

        let mut shm = ShmHandler { inner: shm_inner };
        let mut wm_base = WmBase::new(wm_inner);

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
        pool_obj.create_buffer(&mut conn, &buffer, 0, WIDTH, HEIGHT, stride, 1)?;
        compositor.create_surface(&mut conn, &surface)?;
        wm_base
            .inner
            .get_xdg_surface(&mut conn, &xdg_inner, &surface)?;

        let mut xdg_surf = XdgSurf::new(xdg_inner);
        let mut toplevel = Toplevel::new(top_inner);

        xdg_surf.inner.get_toplevel(&mut conn, &toplevel.inner)?;
        toplevel.inner.set_title(&mut conn, "bench frame")?;
        toplevel.inner.set_app_id(&mut conn, "wayland-bench")?;
        surface.commit(&mut conn)?;
        conn.flush()?;

        let mut attached = false;
        while !attached {
            let (obj_id, opcode, body) = conn.recv_msg()?;
            dispatch_to!(conn, obj_id, opcode, &body;
				display, registry, shm, wm_base, xdg_surf, toplevel, surface, buffer);

            if let Some(serial) = wm_base.pending_pong.take() {
                wm_base.inner.pong(&mut conn, serial)?;
            }
            if let Some(serial) = xdg_surf.pending_ack.take() {
                xdg_surf.inner.ack_configure(&mut conn, serial)?;
                surface.attach(&mut conn, &buffer, 0, 0)?;
                surface.damage(&mut conn, 0, 0, WIDTH, HEIGHT)?;
                surface.commit(&mut conn)?;
                attached = true;
            }
            if toplevel.closed {
                return Err(io::Error::other("toplevel closed during benchmark setup"));
            }
            conn.flush()?;
        }

        Ok(Self {
            conn,
            display,
            registry,
            shm,
            wm_base,
            xdg_surf,
            toplevel,
            surface,
            buffer,
        })
    }

    fn run_attached_loop_until_sync_done(&mut self) -> io::Result<()> {
        // Spawns a new callback object for every syncronization request created.
        let mut sync = SyncCallback::new(self.conn.alloc_id());
        self.display.inner.sync(&mut self.conn, &sync)?;
        self.conn.flush()?;

        // 
        loop {
            let (obj_id, opcode, body) = self.conn.recv_msg()?;
            
            dispatch_to!(self.conn, obj_id, opcode, &body;
				self.display,
				self.registry,
				self.shm,
				self.wm_base,
				self.xdg_surf,
				self.toplevel,
				self.surface,
				self.buffer,
				sync);

            // Pong event : Tells the compositor we're alive and well, and prevents it from killing us for being unresponsive.
            if let Some(serial) = self.wm_base.pending_pong.take() {
                self.wm_base.inner.pong(&mut self.conn, serial)?;
            }

            // Ack configure : Acknowledge the configure, and commit a new frame. This keeps the event loop running with steady-state frame commits
            // damage | commit | 
            if let Some(serial) = self.xdg_surf.pending_ack.take() {
                self.xdg_surf.inner.ack_configure(&mut self.conn, serial)?;
                // Surface is pre-attached in setup; benchmark keeps steady-state frame commits.
                self.surface.damage(&mut self.conn, 0, 0, WIDTH, HEIGHT)?;
                self.surface.commit(&mut self.conn)?;
            }

            if self.toplevel.closed {
                return Err(io::Error::other("toplevel closed during benchmark"));
            }

            self.conn.flush()?;
            if sync.done {
                break;
            }
        }

        Ok(())
    }
}

pub fn bench_attached_surface_event_loop(c: &mut Criterion) {
    let mut state = match FrameBenchState::setup() {
        Ok(s) => s,
        Err(err) => {
            eprintln!("Skipping event-loop frame benchmark: {err}");
            return;
        }
    };

    c.bench_function("event_loop_iteration: attached_surface_frame", |b| {
        b.iter_custom(|iters| {
            let mut total = std::time::Duration::ZERO;
            for _ in 0..iters {
                let t0 = std::time::Instant::now();
                state.run_attached_loop_until_sync_done().unwrap();
                total += t0.elapsed();
            }
            total
        });
    });
}
