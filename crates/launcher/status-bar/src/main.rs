#![recursion_limit = "4096"]

mod atlas {
    include!(concat!(env!("OUT_DIR"), "/ui_gen.rs"));
}
mod widgets;

use drm_fourcc::DrmFourcc;
use std::os::fd::AsRawFd;
use std::time::Duration;

use io_ring::Ring;
use layout::layout;
use timer::{Timer, TimerId, TimerSettings};
use wayland::Wayland;
use widgets::{battery, bluetooth, clock, wifi};

// ── REMOVE: demo ──────────────────────────────────────────────────────────
// Replace DemoDriver with real hardware event sources (D-Bus, sysfs, etc.)
struct DemoDriver {
    battery_step: u8,
    bluetooth_step: u8,
    wifi_step: u8,
}

impl DemoDriver {
    fn tick_battery(&mut self) -> battery::BatteryUpdate {
        const STATES: &[(u8, bool)] = &[
            (100, false),
            (100, true),
            (80, false),
            (80, true),
            (60, false),
            (60, true),
            (40, false),
            (40, true),
            (20, false),
            (20, true),
            (0, false),
            (0, true),
        ];
        let (pct, charging) = STATES[self.battery_step as usize];
        self.battery_step = (self.battery_step + 1) % (STATES.len() as u8);
        battery::BatteryUpdate { pct, charging }
    }

    fn tick_bluetooth(&mut self) -> bluetooth::BluetoothUpdate {
        use bluetooth::BluetoothState;
        let states = [
            BluetoothState::Off,
            BluetoothState::On,
            BluetoothState::Connected,
        ];
        let s = states[self.bluetooth_step as usize];
        self.bluetooth_step = (self.bluetooth_step + 1) % (states.len() as u8);
        bluetooth::BluetoothUpdate(s)
    }

    fn tick_wifi(&mut self) -> wifi::WifiUpdate {
        use wifi::WifiState;
        let states = [
            WifiState::High,
            WifiState::Medium,
            WifiState::Low,
            WifiState::None,
            WifiState::X,
        ];
        let ws = states[self.wifi_step as usize];
        self.wifi_step = (self.wifi_step + 1) % (states.len() as u8);
        wifi::WifiUpdate(ws)
    }
}
// ── END REMOVE: demo ──────────────────────────────────────────────────────

const BAR_HEIGHT: u32 = 36;
const ICON_SIZE: f32 = 24.0;
const GAP: f32 = 12.0;
const PADDING: f32 = 12.0;

// ── REMOVE: charging overlay ─────────────────────────────────────────────
// Monochrome sprites lose the SVG's green fill, so we fake charging with a
// green DrawRect over the juice area. Remove these constants and the
// DrawRect block in render_bar when multicolor sprites arrive; switch back
// to the charging sprite variants in battery.rs.
const JUICE_X_PAD: f32 = 6.0;
const JUICE_Y_PAD: f32 = 9.0;
const JUICE_MAX_W: f32 = 12.0;
const JUICE_H: f32 = 6.0;
// ── END REMOVE: charging overlay ──────────────────────────────────────────

fn secs_to_next_minute() -> u64 {
    60 - (unsafe { libc::time(std::ptr::null_mut()) } as u64 % 60)
}

struct StatusBarState {
    ring: Ring,
    timer: Timer,
    wayland: Wayland,
    renderer: renderer::Renderer,
    surface_id: u32,
    surface_size: (i32, i32),
    dmabuf: [Option<renderer::RenderableSurface<renderer::DmaBuf>>; 2],
    wl_buf_ids: [u32; 2],
    buf_in_flight: [bool; 2],
    icon_tex: Option<renderer::TextureId>,
    gradient_tex: Option<renderer::TextureId>,
    battery: battery::BatteryWidget,
    clock: clock::ClockWidget,
    bluetooth: bluetooth::BluetoothWidget,
    wifi: wifi::WifiWidget,
    // REMOVE: demo — replace with real hardware event source
    demo: DemoDriver,
    clock_timer_id: Option<TimerId>,
}

impl StatusBarState {
    fn new() -> Self {
        let ring = Ring::default();
        let timer = Timer::new(ring.get_proxy());
        let wayland = Wayland::new(ring.get_proxy()).expect("failed to create wayland connection");
        let mut renderer = renderer::Renderer::new().expect("failed to create renderer");

        use renderer::commands::*;
        renderer.init_command_queue::<ClearColor>();
        renderer.init_command_queue::<DrawRect>();
        renderer.init_command_queue::<DrawQuad>();
        renderer.init_command_queue::<DrawMonochromeSprite>();
        renderer.init_command_queue::<DrawText>();

        Self {
            ring,
            timer,
            wayland,
            renderer,
            surface_id: 0,
            surface_size: (0, 0),
            dmabuf: Default::default(),
            wl_buf_ids: [0, 0],
            buf_in_flight: [false, false],
            icon_tex: None,
            gradient_tex: None,
            battery: battery::BatteryWidget::new(),
            clock: clock::ClockWidget::new(),
            bluetooth: bluetooth::BluetoothWidget::new(),
            wifi: wifi::WifiWidget::new(),
            // REMOVE: demo
            demo: DemoDriver {
                battery_step: 0,
                bluetooth_step: 0,
                wifi_step: 0,
            },
            clock_timer_id: None,
        }
    }

    fn schedule_clock_tick(&mut self) {
        self.clock_timer_id = Some(self.timer.start_timer(TimerSettings {
            duration: Duration::from_secs(secs_to_next_minute()),
            repeat: false,
        }));
    }

    fn try_redraw(&mut self) {
        let free_idx = if !self.buf_in_flight[0] {
            0
        } else if !self.buf_in_flight[1] {
            1
        } else {
            return;
        };

        let surface = match &self.dmabuf[free_idx] {
            Some(s) => s,
            None => return,
        };

        self.renderer.active_surface(surface);
        if let Some(icon_tex) = self.icon_tex {
            let win_w = self.surface_size.0 as f32;
            self.render_bar(win_w, icon_tex);
            self.renderer.finish();
        }

        let (w, h) = self.surface_size;
        self.wayland
            .surface
            .attach(self.surface_id, self.wl_buf_ids[free_idx], 0, 0);
        self.wayland.surface.damage(self.surface_id, 0, 0, w, h);
        self.wayland.surface.commit(self.surface_id);
        self.buf_in_flight[free_idx] = true;
        self.wayland.flush();
    }

    fn render_bar(&mut self, win_w: f32, icon_tex: renderer::TextureId) {
        use renderer::commands::*;
        let renderer = &mut self.renderer;

        renderer.send_command(ClearColor(Color::TRANSPARENT));

        if let Some(grad_tex) = self.gradient_tex {
            renderer.send_command(DrawMonochromeSprite {
                texture_id: grad_tex,
                region: Rect::new(0.0, 0.0, 1.0, BAR_HEIGHT as f32),
                origin: Point::new(0.0, 0.0),
                z: 0.0,
                size: Size::new(win_w, BAR_HEIGHT as f32),
                color: Color::BLACK,
            });
        }

        let battery_w = self.battery.slot_width();
        let bluetooth_w = self.bluetooth.slot_width();
        let wifi_w = self.wifi.slot_width();
        let clock_w = self.clock.slot_width();

        let right_visible = [wifi_w, bluetooth_w, battery_w]
            .iter()
            .filter(|&&w| w > 0.0)
            .count() as f32;
        let right_w = wifi_w + bluetooth_w + battery_w + GAP * (right_visible - 1.0).max(0.0);

        layout!(
            {
                available_width: win_w,
                available_height: BAR_HEIGHT as f32,
                direction: row,
                justify: space_between,
                padding_left: PADDING,
                padding_right: PADDING,

                layout!({
                    width: clock_w,
                    height: BAR_HEIGHT as f32,
                }, {
                    let font = &atlas::UI_FONT_INTER_16;
                    let baseline = y + font.get_baseline_offset(BAR_HEIGHT as f32);
                    renderer.send_command(DrawText {
                        font,
                        texture_id: icon_tex,
                        text: self.clock.time_str.clone(),
                        origin: Point::new(x, baseline),
                        z: 0.5,
                        color: Color::WHITE,
                    });
                }),

                layout!({
                    direction: row,
                    width: right_w,
                    height: BAR_HEIGHT as f32,
                    gap: GAP,

                    layout!({
                        width: bluetooth_w,
                        height: ICON_SIZE,
                    }, {
                        if self.bluetooth.visible() {
                            let region = self.bluetooth.sprite_region();
                            renderer.send_command(DrawMonochromeSprite {
                                texture_id: icon_tex,
                                region: Rect::new(region.x, region.y, region.w, region.h),
                                origin: Point::new(x, y),
                                z: 0.1,
                                size: Size::new(ICON_SIZE, ICON_SIZE),
                                color: Color::WHITE,
                            });
                        }
                    }),

                    layout!({
                        width: wifi_w,
                        height: ICON_SIZE,
                    }, {
                        let region = self.wifi.sprite_region();
                        renderer.send_command(DrawMonochromeSprite {
                            texture_id: icon_tex,
                            region: Rect::new(region.x, region.y, region.w, region.h),
                            origin: Point::new(x, y),
                            z: 0.1,
                            size: Size::new(ICON_SIZE, ICON_SIZE),
                            color: Color::WHITE,
                        });
                    }),

                    layout!({
                        width: battery_w,
                        height: ICON_SIZE,
                    }, {
                        let region = self.battery.sprite_region();
                        renderer.send_command(DrawMonochromeSprite {
                            texture_id: icon_tex,
                            region: Rect::new(region.x, region.y, region.w, region.h),
                            origin: Point::new(x, y),
                            z: 0.1,
                            size: Size::new(ICON_SIZE, ICON_SIZE),
                            color: Color::WHITE,
                        });

                        // REMOVE: charging overlay — use charging sprite variants instead
                        if self.battery.state.charging {
                            let juice_w =
                                JUICE_MAX_W * self.battery.state.pct as f32 / 100.0;
                            renderer.send_command(DrawRect {
                                color: Color::rgb(0.2, 0.85, 0.2),
                                origin: Point::new(x + JUICE_X_PAD, y + JUICE_Y_PAD),
                                z: 0.15,
                                size: Size::new(juice_w, JUICE_H),
                            });
                        }
                        // END REMOVE: charging overlay
                    }),

                }, {
                }),
            },
            {
            }
        );

        renderer.process_command_queue::<ClearColor>();
        renderer.process_command_queue::<DrawRect>();
        renderer.process_command_queue::<DrawQuad>();
        renderer.process_command_queue::<DrawMonochromeSprite>();
        renderer.process_command_queue::<DrawText>();
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let state = StatusBarState::new();

    let mut app = app::App::new(state)
        .mount(|s| &mut s.ring, io_ring::module())
        .mount(|s| &mut s.timer, timer::module())
        .mount(|s| &mut s.wayland, wayland::module())
        .mount(
            |s| s,
            app::Module::new().on(|s: &mut StatusBarState, _: &wayland::Initilised| {
                use wayland::zwlr_layer_shell::{KeyboardInteractivity, Layer};

                let surface_id = s.wayland.compositor.create_surface();
                s.wayland.surface.register(surface_id);
                s.surface_id = surface_id;

                let layer_surface_id = s.wayland.layer_shell.get_layer_surface(
                    surface_id,
                    0,
                    Layer::Top,
                    "status-bar",
                );
                s.wayland.layer_surface.register(layer_surface_id);
                s.wayland.layer_surface.set_anchor(
                    layer_surface_id,
                    wayland::zwlr_layer_shell::Anchor::Top
                        | wayland::zwlr_layer_shell::Anchor::Left
                        | wayland::zwlr_layer_shell::Anchor::Right,
                );
                s.wayland
                    .layer_surface
                    .set_size(layer_surface_id, 0, BAR_HEIGHT);
                s.wayland
                    .layer_surface
                    .set_exclusive_zone(layer_surface_id, BAR_HEIGHT as i32);
                s.wayland
                    .layer_surface
                    .set_keyboard_interactivity(layer_surface_id, KeyboardInteractivity::OnDemand);

                s.wayland.surface.commit(surface_id);
                s.wayland.flush();

                s.timer.start_timer(TimerSettings {
                    duration: Duration::from_secs(1),
                    repeat: true,
                });
                s.schedule_clock_tick();
            }),
        )
        .mount(
            |s| s,
            app::Module::new().on(
                |s: &mut StatusBarState, ev: &wayland::zwlr_layer_shell::LayerSurfaceEvent| {
                    use wayland::zwlr_layer_shell::LayerSurfaceEvent;

                    let LayerSurfaceEvent::Configured {
                        id,
                        serial,
                        width,
                        height,
                    } = ev
                    else {
                        return;
                    };

                    let w = if *width == 0 { 1920i32 } else { *width as i32 };
                    let h = if *height == 0 {
                        BAR_HEIGHT as i32
                    } else {
                        *height as i32
                    };
                    s.surface_size = (w, h);

                    let surface0 = s
                        .renderer
                        .create_surface::<renderer::DmaBuf>(w as u32, h as u32)
                        .expect("dmabuf surface 0");
                    let surface1 = s
                        .renderer
                        .create_surface::<renderer::DmaBuf>(w as u32, h as u32)
                        .expect("dmabuf surface 1");

                    let buf_id0 = create_wl_buffer(&mut s.wayland, &surface0, w, h);
                    let buf_id1 = create_wl_buffer(&mut s.wayland, &surface1, w, h);
                    s.wayland.wl_buffer.register(buf_id0);
                    s.wayland.wl_buffer.register(buf_id1);
                    s.wl_buf_ids = [buf_id0, buf_id1];

                    if s.icon_tex.is_none() {
                        s.icon_tex = s.renderer.upload_atlas(atlas::UI.png_bytes).ok();
                    }

                    if s.gradient_tex.is_none() {
                        let mut data = vec![0u8; BAR_HEIGHT as usize];
                        // TUNE THESE ───────────────────────────
                        const TOP: f32 = 0.95; // opacity at top of bar
                        const MID: f32 = 0.30; // opacity where curve bends
                        const CUT: f32 = 0.70; // where the bend happens (0..1, top..bottom)
                        // ──────────────────────────────────────
                        for y in 0..BAR_HEIGHT as usize {
                            let t = y as f32 / (BAR_HEIGHT - 1) as f32;
                            let alpha = if t <= CUT {
                                TOP - (TOP - MID) * (t / CUT)
                            } else {
                                MID * (1.0 - (t - CUT) / (1.0 - CUT))
                            };
                            data[y] = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
                        }
                        s.gradient_tex = s
                            .renderer
                            .create_texture(1, BAR_HEIGHT, renderer::TextureFormat::R8, &data)
                            .ok();
                    }

                    s.dmabuf = [Some(surface0), Some(surface1)];
                    s.buf_in_flight = [false, false];

                    s.wayland.layer_surface.ack_configure(*id, *serial);
                    s.try_redraw();
                },
            ),
        )
        .mount(
            |s| s,
            app::Module::new().on(|s: &mut StatusBarState, ev: &wayland::WlBufferEvent| {
                let wayland::WlBufferEvent::Release { id } = ev;
                for i in 0..2 {
                    if s.wl_buf_ids[i] == *id {
                        s.buf_in_flight[i] = false;
                        break;
                    }
                }
            }),
        )
        .mount(
            |s| s,
            app::Module::new().on(|_: &mut StatusBarState, ev: &wayland::KeyboardEvent| {
                if let wayland::KeyboardEvent::Key { key, state, .. } = ev {
                    if (*key == 1 || *key == 16) && *state == wayland::KeyState::Pressed {
                        std::process::exit(0);
                    }
                }
            }),
        )
        .mount(
            |s| s,
            // REMOVE: demo — replace with real hardware polling
            app::Module::new().on(|s: &mut StatusBarState, _: &timer::TimerEvent| {
                Some(s.demo.tick_battery())
            }),
        )
        .mount(
            |s| s,
            app::Module::new().on(|s: &mut StatusBarState, _: &timer::TimerEvent| {
                Some(s.demo.tick_bluetooth())
            }),
        )
        .mount(
            |s| s,
            app::Module::new().on(|s: &mut StatusBarState, _: &timer::TimerEvent| {
                Some(s.demo.tick_wifi())
            }),
        )
        .mount(
            |s| s,
            app::Module::new().on(
                |s: &mut StatusBarState, ev: &timer::TimerEvent| {
                    if s.clock_timer_id != Some(ev.id()) {
                        return None;
                    }
                    s.schedule_clock_tick();
                    let (h, m) = clock::local_time();
                    Some(clock::ClockUpdate(h, m))
                },
            ),
        )
        .mount(
            |s| s,
            app::Module::new().on(|s: &mut StatusBarState, _: &battery::BatteryChanged| {
                s.try_redraw();
            }),
        )
        .mount(
            |s| s,
            app::Module::new().on(|s: &mut StatusBarState, _: &clock::ClockChanged| {
                s.try_redraw();
            }),
        )
        .mount(
            |s| s,
            app::Module::new().on(|s: &mut StatusBarState, _: &bluetooth::BluetoothChanged| {
                s.try_redraw();
            }),
        )
        .mount(
            |s| s,
            app::Module::new().on(|s: &mut StatusBarState, _: &wifi::WifiChanged| {
                s.try_redraw();
            }),
        )
        .mount(|s| &mut s.battery, battery::module())
        .mount(|s| &mut s.clock, clock::module())
        .mount(|s| &mut s.bluetooth, bluetooth::module())
        .mount(|s| &mut s.wifi, wifi::module());

    app.dispatch(&app::Start);
    loop {
        app.dispatch(&app::PrePoll);
        app.dispatch(&app::Poll);
    }
}

fn create_wl_buffer(
    wayland: &mut Wayland,
    surface: &renderer::RenderableSurface<renderer::DmaBuf>,
    width: i32,
    height: i32,
) -> u32 {
    let modifier = surface.backend.modifier;
    let modifier_hi = (modifier >> 32) as u32;
    let modifier_lo = (modifier & 0xffff_ffff) as u32;
    let fd = unsafe { libc::dup(surface.backend.prime_fd.as_raw_fd()) };
    if fd < 0 {
        panic!(
            "failed to dup prime fd: {}",
            std::io::Error::last_os_error()
        );
    }

    let params_id = wayland.dmabuf.create_params();
    wayland.buf_params.register(params_id);
    wayland.buf_params.add(
        params_id,
        fd,
        0,
        0,
        surface.backend.stride,
        modifier_hi,
        modifier_lo,
    );
    let buf_id =
        wayland
            .buf_params
            .create_immed(params_id, width, height, DrmFourcc::Argb8888 as u32, 0);
    wayland.buf_params.destroy(params_id);
    buf_id
}
