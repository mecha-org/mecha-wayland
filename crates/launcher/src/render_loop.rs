use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use event_manager::{Builder, Component, EventContext, EventHandler};
use renderer::commands::{ClearColor, DrawMonochromeSprite, DrawQuad, DrawRect};
use renderer::{DmaBuf, Renderer, TextureId};
use wayland_protocols::connection::Connection;
use wayland_protocols::object::Object as _;
use wayland_protocols::WlSurface;

use crate::waiter::WaitUntil;

const FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

pub struct Configured;
pub struct BufferReleased {
    pub id: u32,
}

#[derive(PartialEq)]
pub(crate) enum SlotState {
    Free,
    InFlight,
}

pub(crate) struct Slot {
    pub surf: renderer::RenderableSurface<DmaBuf>,
    pub wl_buf: wayland_protocols::WlBuffer,
    pub state: SlotState,
}

pub struct RenderLoop {
    renderer: Renderer,
    slots: [Slot; 2],
    surface: WlSurface,
    conn: Rc<RefCell<Connection>>,
    icon_tex: TextureId,
    configured: bool,
    fps_frame_count: u32,
    fps_timer: Instant,
}

impl RenderLoop {
    pub fn new(
        renderer: Renderer,
        slots: [Slot; 2],
        surface: WlSurface,
        conn: Rc<RefCell<Connection>>,
        icon_tex: TextureId,
    ) -> Self {
        Self {
            renderer,
            slots,
            surface,
            conn,
            icon_tex,
            configured: false,
            fps_frame_count: 0,
            fps_timer: Instant::now(),
        }
    }

    pub fn buffer_ids(&self) -> [u32; 2] {
        [
            self.slots[0].wl_buf.object_id(),
            self.slots[1].wl_buf.object_id(),
        ]
    }
}

impl EventHandler<WaitUntil> for RenderLoop {
    fn handle(&mut self, _event: &WaitUntil, ctx: &EventContext) {
        ctx.send(WaitUntil(Instant::now() + FRAME_INTERVAL));

        if !self.configured {
            return;
        }

        let Some(slot) = self.slots.iter_mut().find(|s| s.state == SlotState::Free) else {
            return;
        };

        self.renderer.active_surface(&slot.surf);

        self.renderer.send_command(ClearColor {
            r: 0.32,
            g: 0.32,
            b: 0.32,
            a: 1.0,
        });
        self.renderer.send_command(DrawQuad {
            color: (0.9, 0.2, 0.2, 1.0),
            border_color: (1.0, 1.0, 1.0, 1.0),
            origin: (214.0, 240.0, 0.0),
            size: (600.0, 600.0),
            border_radius: 16.0,
            border_thickness: 3.0,
        });
        self.renderer.send_command(DrawQuad {
            color: (0.2 * 0.9, 0.4, 1.0, 1.0),
            border_color: (1.0, 1.0, 1.0, 1.0),
            origin: (414.0, 440.0, 1.0),
            size: (200.0, 200.0),
            border_radius: 12.0,
            border_thickness: 3.0,
        });
        self.renderer.send_command(DrawMonochromeSprite {
            texture_id: self.icon_tex,
            region: (
                crate::atlas::UI_ICON.x,
                crate::atlas::UI_ICON.y,
                crate::atlas::UI_ICON.w,
                crate::atlas::UI_ICON.h,
            ),
            origin: (450.0, 476.0 + (128.0 - 42.0) / 2.0, 1.0),
            size: (128.0, 42.0),
            color: (1.0, 1.0, 1.0, 1.0),
        });

        self.renderer.process_command_queue::<ClearColor>();
        self.renderer.process_command_queue::<DrawRect>();
        self.renderer.process_command_queue::<DrawQuad>();
        self.renderer.process_command_queue::<DrawMonochromeSprite>();
        unsafe {
            self.renderer.gl.finish();
        }

        let mut conn = self.conn.borrow_mut();
        self.surface
            .attach(&mut conn, &slot.wl_buf, 0, 0)
            .unwrap();
        self.surface
            .damage(&mut conn, 0, 0, crate::WIDTH as i32, crate::HEIGHT as i32)
            .unwrap();
        self.surface.commit(&mut conn).unwrap();
        drop(conn);

        slot.state = SlotState::InFlight;

        self.fps_frame_count += 1;
        let elapsed = self.fps_timer.elapsed();
        if elapsed.as_secs_f32() >= 1.0 {
            tracing::info!(fps = self.fps_frame_count, "FPS");
            self.fps_frame_count = 0;
            self.fps_timer = Instant::now();
        }
    }
}

impl EventHandler<Configured> for RenderLoop {
    fn handle(&mut self, _event: &Configured, _ctx: &EventContext) {
        self.configured = true;
    }
}

impl EventHandler<BufferReleased> for RenderLoop {
    fn handle(&mut self, event: &BufferReleased, _ctx: &EventContext) {
        for slot in &mut self.slots {
            if slot.wl_buf.object_id() == event.id {
                slot.state = SlotState::Free;
            }
        }
    }
}

impl Component for RenderLoop {
    fn register(self, builder: &mut Builder) {
        builder.subscribe::<(WaitUntil, Configured, BufferReleased)>(self);
    }
}
