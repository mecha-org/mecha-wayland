#![recursion_limit = "4096"]

mod atlas {
    include!(concat!(env!("OUT_DIR"), "/ui_gen.rs"));
}

use animation::Animated;
use app::prelude::*;
use drm_fourcc::DrmFourcc;
use renderer::commands::{ClearColor, DrawMonochromeSprite, DrawQuad};
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

use interactivity::{DragState, InteractivityState, PointerEvent, TouchEvent};
use io_ring::Ring;
use utils::{Color, Point, Rect, Size};
use wayland::Wayland;

// ── CallbackId ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CallbackId(u32);

// ── Constants ─────────────────────────────────────────────────────────────

const HANDLE_WIDTH: f32 = 100.0;
const HANDLE_HEIGHT: f32 = 12.0;
const TRIGGER_BAR_HEIGHT: f32 = 20.0;
const HANDLE_TOP_PAD: f32 = 4.0;
const HANDLE_COLOR: Color = Color::rgba(1.0, 1.0, 1.0, 0.45);

const BTN_SIZE: f32 = 52.0;
const BTN_RADIUS: f32 = 14.0;
const BTN_BG: Color = Color::from_rgb8(42, 42, 48);
const ICON_TINT: Color = Color::WHITE;
const ICON_TO_BTN_RATIO: f32 = 0.55;

const ICON_Y_OFFSETS: [f32; 5] = [60.0, 73.0, 86.0, 73.0, 60.0];
const ICON_SPACING: f32 = 84.0;

const NAV_BG_HEIGHT: f32 = 130.0;
const NAV_SURFACE_HEIGHT: i32 = (NAV_BG_HEIGHT + TRIGGER_BAR_HEIGHT) as i32 + 4;

const TRACKING_MS: u64 = 80;
const APPEAR_MS: u64 = 120;
const DISAPPEAR_MS: u64 = 250;
const GROWTH_MS: u64 = 180;

const ICON_GROWTH_MAX: f32 = 1.30;

const VELOCITY_THRESHOLD: f64 = 0.25;
const VELOCITY_EMA_ALPHA: f64 = 0.4;
const RECENTS_DWELL_MS: u64 = 150;

const SWIPE_ZONE_WIDTH: f32 = 120.0;
const SWIPE_ZONE_HEIGHT: f32 = 20.0;
const SWIPE_ACTIVATION_PX: f32 = 12.0;

const BTN_LEFT: u32 = 272;

// ── NavDragState ──────────────────────────────────────────────────────────

#[derive(Default)]
struct NavDragState {
    active: bool,
    finger_x: f64,
    finger_y: f64,
    velocity_x: f64,
    velocity_y: f64,
    zone: Option<usize>,
    last_selected: Option<usize>,
    pointer_held: bool,
    pointer_start_y: f64,
    recents_entered_at: Option<Duration>,
}

// ── NavBarTextures ────────────────────────────────────────────────────────

#[derive(Default)]
struct NavBarTextures {
    icon: Option<renderer::TextureId>,
    gradient: Option<renderer::TextureId>,
}

// ── NavBarState ───────────────────────────────────────────────────────────

#[derive(State)]
struct NavBarState {
    now: Duration,

    ring: Ring,
    wayland: Wayland,
    renderer: renderer::Renderer,
    interactivity: InteractivityState,

    surface_id: u32,
    surface_size: (i32, i32),
    dmabuf: [Option<renderer::RenderableSurface<renderer::DmaBuf>>; 2],
    wl_buf_ids: [u32; 2],
    buf_in_flight: [bool; 2],
    textures: NavBarTextures,
    needs_redraw: bool,
    pending_callback_id: CallbackId,
    callback_set_at: Option<Instant>,

    drag: NavDragState,
    drag_offset: Animated<f32>,
    icon_growth: [Animated<f32>; 5],
}

impl Default for NavBarState {
    fn default() -> Self {
        let ring = Ring::default();
        let wayland = Wayland::new(ring.get_proxy()).expect("failed to create wayland connection");
        let mut renderer = renderer::Renderer::new().expect("failed to create renderer");

        renderer.init_command_queue::<ClearColor>();
        renderer.init_command_queue::<DrawQuad>();
        renderer.init_command_queue::<DrawMonochromeSprite>();

        let drag_offset = Animated::static_value(0.0_f32);

        let icon_growth = [
            Animated::static_value(1.0_f32),
            Animated::static_value(1.0_f32),
            Animated::static_value(1.0_f32),
            Animated::static_value(1.0_f32),
            Animated::static_value(1.0_f32),
        ];

        Self {
            now: Duration::ZERO,
            ring,
            wayland,
            renderer,
            interactivity: InteractivityState::new(),
            surface_id: 0,
            surface_size: (0, 0),
            dmabuf: [None, None],
            wl_buf_ids: [0, 0],
            buf_in_flight: [false, false],
            textures: NavBarTextures::default(),
            needs_redraw: false,
            pending_callback_id: CallbackId(0),
            callback_set_at: None,
            drag: NavDragState::default(),
            drag_offset,
            icon_growth,
        }
    }
}

impl NavBarState {
    pub fn new() -> Self {
        Self::default()
    }

    // ── helpers ──────────────────────────────────────────────────────────

    fn refresh_now(&mut self) {
        self.now = animation::monotonic_now();
    }

    fn in_swipe_zone(&self, x: f64, y: f64) -> bool {
        let (sw, sh) = self.surface_size;
        if sw == 0 || sh == 0 {
            return false;
        }
        let swf = sw as f32;
        let shf = sh as f32;
        let zone_left = (swf - SWIPE_ZONE_WIDTH) / 2.0;
        let zone_right = zone_left + SWIPE_ZONE_WIDTH;
        let zone_top = shf - SWIPE_ZONE_HEIGHT;
        x >= zone_left as f64 && x <= zone_right as f64 && y >= zone_top as f64 && y <= shf as f64
    }

    fn icon_center(&self, i: usize, vis: f32) -> (f32, f32) {
        let (sw, sh) = self.surface_size;
        let cx = sw as f32 / 2.0 + (i as f32 - 2.0) * ICON_SPACING;
        let rest_y = sh as f32 - ICON_Y_OFFSETS[i];
        let hidden_y = sh as f32 + BTN_SIZE;
        let cy = hidden_y + (rest_y - hidden_y) * vis;
        (cx, cy)
    }

    fn visibility_at(&self) -> f32 {
        (self.drag_offset.get(self.now) / SWIPE_ACTIVATION_PX).clamp(0.0, 1.0)
    }

    fn is_animating_at(&self) -> bool {
        self.drag_offset.is_animating(self.now)
            || self.icon_growth.iter().any(|g| g.is_animating(self.now))
    }

    fn icon_sprite_region(&self, i: usize) -> &'static assets::SpriteRegion {
        match i {
            0 => &atlas::UI_NAV_HEART,
            1 => &atlas::UI_NAV_SEARCH,
            2 => &atlas::UI_NAV_RECENT,
            3 => &atlas::UI_NAV_NOTIFICATIONS,
            4 => &atlas::UI_NAV_SETTINGS,
            _ => unreachable!(),
        }
    }

    fn icon_name(i: usize) -> &'static str {
        match i {
            0 => "heart",
            1 => "search",
            2 => "recent",
            3 => "notifications",
            4 => "settings",
            _ => "?",
        }
    }

    /// Pie-slice sector radiating from the bottom-centre of the screen.
    /// Returns the icon index whose angular sector the finger currently
    /// occupies, or `None` if the finger hasn't moved above the centre yet.
    fn sector_for_point(&self, px: f32, py: f32) -> Option<usize> {
        let (sw, sh) = self.surface_size;
        let cx = sw as f32 / 2.0;
        let cy = sh as f32;
        let dx = px - cx;
        let dy = cy - py;
        if dy < 4.0 {
            return None;
        }
        let angle = dx.atan2(dy);

        // Compute each icon's angular position at full visibility and
        // assign the finger to the nearest one, within expected range.
        let mut closest: Option<usize> = None;
        let mut closest_dist = f32::MAX;
        for i in 0..5 {
            let (icx, icy) = self.icon_center(i, 1.0);
            let ia = (icx - cx).atan2(cy - icy);
            let ad = (angle - ia).abs();
            let ad = ad.min(std::f32::consts::TAU - ad);
            if ad < closest_dist {
                closest_dist = ad;
                closest = Some(i);
            }
        }
        let max_ad = std::f32::consts::PI / 4.0;
        if closest_dist <= max_ad {
            closest
        } else {
            None
        }
    }

    // ── surface management ───────────────────────────────────────────────

    fn try_redraw(&mut self, chain: bool) -> bool {
        let free_idx = if !self.buf_in_flight[0] {
            0
        } else if !self.buf_in_flight[1] {
            1
        } else {
            return false;
        };

        let surface = match &self.dmabuf[free_idx] {
            Some(s) => s,
            None => return false,
        };

        self.renderer.active_surface(surface);
        if let (Some(icon_tex), Some(grad_tex)) = (self.textures.icon, self.textures.gradient) {
            self.render_frame(icon_tex, grad_tex);
            self.renderer.finish();
        }

        if chain && self.pending_callback_id == CallbackId(0) {
            let cb_id = self.wayland.surface.frame(self.surface_id);
            self.wayland.callback.register_frame(cb_id);
            self.pending_callback_id = CallbackId(cb_id);
            self.callback_set_at = Some(Instant::now());
        }

        let (w, h) = self.surface_size;
        self.wayland
            .surface
            .attach(self.surface_id, self.wl_buf_ids[free_idx], 0, 0);
        self.wayland.surface.damage(self.surface_id, 0, 0, w, h);

        self.wayland.surface.commit(self.surface_id);
        self.buf_in_flight[free_idx] = true;

        self.wayland.flush();
        true
    }

    fn request_redraw(&mut self) {
        self.needs_redraw = true;
        if self.pending_callback_id == CallbackId(0) {
            self.needs_redraw = false;
            self.refresh_now();
            if !self.try_redraw(self.is_animating_at()) {
                self.needs_redraw = true;
            }
        }
    }

    // ── render ───────────────────────────────────────────────────────────

    fn render_frame(&mut self, icon_tex: renderer::TextureId, grad_tex: renderer::TextureId) {
        let (sw, sh) = self.surface_size;
        let swf = sw as f32;
        let shf = sh as f32;

        let vis = self.visibility_at();

        let growth: [f32; 5] = [
            self.icon_growth[0].get(self.now),
            self.icon_growth[1].get(self.now),
            self.icon_growth[2].get(self.now),
            self.icon_growth[3].get(self.now),
            self.icon_growth[4].get(self.now),
        ];

        let centers: [(f32, f32); 5] = [
            self.icon_center(0, vis),
            self.icon_center(1, vis),
            self.icon_center(2, vis),
            self.icon_center(3, vis),
            self.icon_center(4, vis),
        ];

        let regions: [&assets::SpriteRegion; 5] = [
            self.icon_sprite_region(0),
            self.icon_sprite_region(1),
            self.icon_sprite_region(2),
            self.icon_sprite_region(3),
            self.icon_sprite_region(4),
        ];

        let renderer = &mut self.renderer;
        renderer.send_command(ClearColor(Color::TRANSPARENT));

        // Gradient shadow rising from bottom
        if vis > 0.005 {
            let grad_h = NAV_BG_HEIGHT * vis;
            let tex_h = NAV_SURFACE_HEIGHT as f32;
            renderer.send_command(DrawMonochromeSprite {
                texture_id: grad_tex,
                region: Rect::new(0.0, tex_h - grad_h, 1.0, grad_h),
                origin: Point::new(0.0, shf - grad_h),
                z: 0.04,
                size: Size::new(swf, grad_h),
                color: Color::rgba(0.0, 0.0, 0.0, vis),
            });

            // Button backgrounds (icons drawn in a separate pass below)
            for i in 0..5 {
                let (cx, cy) = centers[i];
                let scale = vis * growth[i];
                let half = BTN_SIZE * scale / 2.0;

                renderer.send_command(DrawQuad {
                    color: BTN_BG,
                    border_color: Color::TRANSPARENT,
                    origin: Point::new(cx - half, cy - half),
                    z: 0.1 + i as f32 * 0.01,
                    size: Size::new(BTN_SIZE * scale, BTN_SIZE * scale),
                    border_radius: BTN_RADIUS * scale,
                    border_thickness: 0.0,
                });
            }
        }

        // Bottom handle — always visible
        let handle_x = (swf - HANDLE_WIDTH) / 2.0;
        let handle_y = shf - TRIGGER_BAR_HEIGHT + HANDLE_TOP_PAD;
        renderer.send_command(DrawQuad {
            color: HANDLE_COLOR,
            border_color: Color::TRANSPARENT,
            origin: Point::new(handle_x, handle_y),
            z: 0.01,
            size: Size::new(HANDLE_WIDTH, HANDLE_HEIGHT),
            border_radius: HANDLE_HEIGHT / 2.0,
            border_thickness: 0.0,
        });

        // Layer 1: gradient behind buttons
        renderer.process_command_queue::<ClearColor>();
        renderer.process_command_queue::<DrawMonochromeSprite>();
        renderer.process_command_queue::<DrawQuad>();

        // Layer 2: icons on top of buttons
        if vis > 0.005 {
            for i in 0..5 {
                let (cx, cy) = centers[i];
                let scale = vis * growth[i];
                let icon_scale = scale * ICON_TO_BTN_RATIO;
                let icon_sz = BTN_SIZE * icon_scale;
                let r = regions[i];
                renderer.send_command(DrawMonochromeSprite {
                    texture_id: icon_tex,
                    region: Rect::new(r.x, r.y, r.w, r.h),
                    origin: Point::new(cx - icon_sz / 2.0, cy - icon_sz / 2.0),
                    z: 0.2 + i as f32 * 0.01,
                    size: Size::new(icon_sz, icon_sz),
                    color: Color {
                        a: vis,
                        ..ICON_TINT
                    },
                });
            }
        }
        renderer.process_command_queue::<DrawMonochromeSprite>();
    }

    // ── gesture handlers ─────────────────────────────────────────────────

    fn ensure_in_bounds(&mut self, x: f64, y: f64, total_dy: f64) {
        let (sw, sh) = self.surface_size;
        if sw == 0 || sh == 0 {
            return;
        }
        if y < 0.0 {
            self.end_drag(total_dy);
        } else if x < 0.0 || x > sw as f64 || y > sh as f64 {
            self.cancel_drag();
        }
    }

    fn activate_drag(&mut self, x: f64, y: f64) {
        if !self.in_swipe_zone(x, y) {
            return;
        }
        self.drag.active = true;
        self.drag.finger_x = x;
        self.drag.finger_y = y;
        self.drag.velocity_x = 0.0;
        self.drag.velocity_y = 0.0;
        self.drag.zone = None;
        self.drag.recents_entered_at = None;

        if let Some(prev) = self.drag.last_selected.take() {
            self.icon_growth[prev].animate_to(
                self.now,
                1.0_f32,
                animation::AnimationConfig::new(
                    Duration::from_millis(TRACKING_MS),
                    animation::Easing::EaseInOut,
                ),
            );
        }

        self.drag_offset.animate_to(
            self.now,
            0.3_f32,
            animation::AnimationConfig::new(
                Duration::from_millis(APPEAR_MS),
                animation::Easing::EaseOut,
            ),
        );
        self.request_redraw();
    }

    fn on_move(&mut self, x: f64, y: f64, dx: f64, dy: f64, total_dy: f64) {
        if !self.drag.active {
            return;
        }
        self.ensure_in_bounds(x, y, total_dy);
        if !self.drag.active {
            return;
        }
        self.drag.velocity_x =
            self.drag.velocity_x * (1.0 - VELOCITY_EMA_ALPHA) + dx * VELOCITY_EMA_ALPHA;
        self.drag.velocity_y =
            self.drag.velocity_y * (1.0 - VELOCITY_EMA_ALPHA) + dy * VELOCITY_EMA_ALPHA;
        self.drag.finger_x = x;
        self.drag.finger_y = y;
        self.drag_offset
            .set_target(self.now, (-total_dy).max(0.0) as f32);
        self.update_growth();
        self.request_redraw();
    }

    fn update_growth(&mut self) {
        let fx = self.drag.finger_x as f32;
        let fy = self.drag.finger_y as f32;
        let vis = self.visibility_at();
        let new_zone = self.sector_for_point(fx, fy);

        let zone_target = new_zone.map(|i| {
            let (cx, cy) = self.icon_center(i, vis);
            let dist = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let inner = BTN_SIZE * 0.5;
            let outer = ICON_SPACING * 1.5;
            let proximity = if dist <= inner {
                1.0
            } else if dist >= outer {
                0.0
            } else {
                (outer - dist) / (outer - inner)
            };
            1.0 + proximity * (ICON_GROWTH_MAX - 1.0)
        });

        if new_zone == self.drag.zone {
            if let Some(i) = new_zone {
                self.icon_growth[i].set_target(self.now, zone_target.unwrap());
            }
            return;
        }

        // Zone changed — animate the old one back, kick the new one.
        if let Some(old) = self.drag.zone {
            self.icon_growth[old].animate_to(
                self.now,
                1.0_f32,
                animation::AnimationConfig::new(
                    Duration::from_millis(TRACKING_MS),
                    animation::Easing::EaseInOut,
                ),
            );
        }
        self.drag.zone = new_zone;
        if let Some(new) = new_zone {
            self.icon_growth[new].animate_to(
                self.now,
                zone_target.unwrap(),
                animation::AnimationConfig::new(
                    Duration::from_millis(GROWTH_MS),
                    animation::Easing::EaseOut,
                ),
            );
        }

        if self.drag.zone == Some(2) {
            self.drag.recents_entered_at.get_or_insert(self.now);
        } else {
            self.drag.recents_entered_at = None;
        }
    }

    fn end_drag(&mut self, total_dy: f64) {
        if !self.drag.active {
            return;
        }
        self.drag.active = false;

        let speed = (self.drag.velocity_x.powi(2) + self.drag.velocity_y.powi(2)).sqrt();

        let target = self.drag.zone;
        let extrapolated = speed > VELOCITY_THRESHOLD;

        self.drag.zone = None;

        if let Some(sel) = target {
            // Fast flick with nearly no dwell on recents → home.
            let dwell = self
                .drag
                .recents_entered_at
                .map_or(Duration::ZERO, |t| self.now.saturating_sub(t));
            if sel == 2 && extrapolated && dwell < Duration::from_millis(RECENTS_DWELL_MS) {
                self.drag.last_selected = None;
                println!(
                    "[nav-bar] home  —  dwell: {:.0}ms",
                    dwell.as_secs_f64() * 1000.0
                );
            } else {
                self.drag.last_selected = Some(sel);
                self.icon_growth[sel].animate_to(
                    self.now,
                    ICON_GROWTH_MAX,
                    animation::AnimationConfig::new(
                        Duration::from_millis(GROWTH_MS),
                        animation::Easing::EaseOut,
                    ),
                );
                println!(
                    "[nav-bar] {}  —  extrapolated: {}",
                    Self::icon_name(sel),
                    extrapolated
                );
            }
        } else {
            println!("[nav-bar] no target  —  drag_px: {:.0}", -total_dy as f32);
        }

        self.drag_offset.animate_to(
            self.now,
            0.0_f32,
            animation::AnimationConfig::new(
                Duration::from_millis(DISAPPEAR_MS),
                animation::Easing::EaseIn,
            ),
        );

        self.request_redraw();
    }

    fn cancel_drag(&mut self) {
        if !self.drag.active {
            return;
        }
        self.drag.active = false;

        if let Some(old) = self.drag.zone.take() {
            self.icon_growth[old].animate_to(
                self.now,
                1.0_f32,
                animation::AnimationConfig::new(
                    Duration::from_millis(TRACKING_MS),
                    animation::Easing::EaseInOut,
                ),
            );
        }

        self.drag_offset.animate_to(
            self.now,
            0.0_f32,
            animation::AnimationConfig::new(
                Duration::from_millis(DISAPPEAR_MS),
                animation::Easing::EaseIn,
            ),
        );

        self.request_redraw();
    }
}

// ── main ──────────────────────────────────────────────────────────────────

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let state = NavBarState::new();

    let mut app = app::App::new(state)
        .mount(io_ring::module())
        .mount(wayland::module())
        .mount(interactivity::module())
        .mount(
            app::Module::new().on(|s: &mut NavBarState, _: &app::PrePoll| {
                // Clear a stale frame callback so PrePoll can restart the chain
                // when the compositor drops events (VT switch, occlusion).
                if let Some(at) = s.callback_set_at {
                    if at.elapsed().as_millis() > 200 {
                        s.pending_callback_id = CallbackId(0);
                        s.callback_set_at = None;
                    }
                }
                if s.pending_callback_id == CallbackId(0) {
                    s.refresh_now();
                    let animating = s.is_animating_at();
                    if animating || s.needs_redraw {
                        s.needs_redraw = false;
                        if !s.try_redraw(animating) {
                            s.needs_redraw = true;
                        }
                    }
                }
            }),
        )
        .mount(
            app::Module::new().on(|s: &mut NavBarState, _: &wayland::Initilised| {
                use wayland::zwlr_layer_shell::{KeyboardInteractivity, Layer};

                let surface_id = s.wayland.compositor.create_surface();
                s.wayland.surface.register(surface_id);
                s.surface_id = surface_id;

                let layer_surface_id = s.wayland.layer_shell.get_layer_surface(
                    surface_id,
                    0,
                    Layer::Overlay,
                    "nav-bar",
                );
                s.wayland.layer_surface.register(layer_surface_id);
                s.wayland.layer_surface.set_anchor(
                    layer_surface_id,
                    wayland::zwlr_layer_shell::Anchor::Bottom
                        | wayland::zwlr_layer_shell::Anchor::Left
                        | wayland::zwlr_layer_shell::Anchor::Right,
                );
                s.wayland
                    .layer_surface
                    .set_size(layer_surface_id, 0, NAV_SURFACE_HEIGHT as u32);
                s.wayland
                    .layer_surface
                    .set_exclusive_zone(layer_surface_id, 0);
                s.wayland
                    .layer_surface
                    .set_keyboard_interactivity(layer_surface_id, KeyboardInteractivity::None);

                s.wayland.surface.commit(surface_id);
                s.wayland.flush();
            }),
        )
        .mount(app::Module::new().on(
            |s: &mut NavBarState, ev: &wayland::zwlr_layer_shell::LayerSurfaceEvent| {
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
                    NAV_SURFACE_HEIGHT
                } else {
                    *height as i32
                };
                s.surface_size = (w, h);

                let surf0 = s
                    .renderer
                    .create_surface::<renderer::DmaBuf>(w as u32, h as u32)
                    .expect("dmabuf surface 0");
                let surf1 = s
                    .renderer
                    .create_surface::<renderer::DmaBuf>(w as u32, h as u32)
                    .expect("dmabuf surface 1");

                let buf0 = create_wl_buffer(&mut s.wayland, &surf0, w, h);
                let buf1 = create_wl_buffer(&mut s.wayland, &surf1, w, h);
                s.wayland.wl_buffer.register(buf0);
                s.wayland.wl_buffer.register(buf1);
                s.wl_buf_ids = [buf0, buf1];

                if s.textures.icon.is_none() {
                    s.textures.icon = s.renderer.upload_atlas(&atlas::UI).ok();
                }

                if s.textures.gradient.is_none() {
                    let grad_h = NAV_SURFACE_HEIGHT as u32;
                    let mut data = vec![0u8; grad_h as usize];
                    for y in 0..grad_h as usize {
                        let t = y as f32 / (grad_h - 1) as f32;
                        let alpha = t * t;
                        data[y] = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
                    }
                    s.textures.gradient = s
                        .renderer
                        .create_texture(1, grad_h, renderer::TextureFormat::R8, &data)
                        .ok();
                }

                s.dmabuf = [Some(surf0), Some(surf1)];
                s.buf_in_flight = [false, false];

                s.wayland.layer_surface.ack_configure(*id, *serial);
                s.refresh_now();
                s.try_redraw(false);
            },
        ))
        .mount(
            app::Module::new().on(|s: &mut NavBarState, ev: &wayland::WlCallbackEvent| {
                let wayland::WlCallbackEvent::Done {
                    id,
                    callback_data: _,
                } = ev;
                if CallbackId(*id) != s.pending_callback_id {
                    return;
                }
                s.pending_callback_id = CallbackId(0);
                let had_pending = s.callback_set_at.take().is_some();

                s.refresh_now();
                let animating = s.is_animating_at();
                if s.needs_redraw || animating || had_pending {
                    s.needs_redraw = false;
                    if !s.try_redraw(animating) {
                        s.needs_redraw = true;
                    }
                }
            }),
        )
        .mount(
            app::Module::new().on(|s: &mut NavBarState, ev: &wayland::WlBufferEvent| {
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
            app::Module::new().on(|s: &mut NavBarState, ev: &TouchEvent| match ev {
                TouchEvent::Down { x, y, .. } => {
                    s.refresh_now();
                    s.activate_drag(*x, *y);
                }
                TouchEvent::Drag {
                    state,
                    x,
                    y,
                    delta_x,
                    delta_y,
                    total_dy,
                    ..
                } => match state {
                    DragState::Start => {}
                    DragState::Move => {
                        s.refresh_now();
                        s.on_move(*x, *y, *delta_x, *delta_y, *total_dy);
                    }
                    DragState::End => {
                        s.refresh_now();
                        s.end_drag(*total_dy);
                    }
                    DragState::Cancel => {
                        s.refresh_now();
                        s.cancel_drag();
                    }
                },
                _ => {}
            }),
        )
        .mount(
            app::Module::new().on(|s: &mut NavBarState, ev: &PointerEvent| match ev {
                PointerEvent::ButtonPress {
                    button: BTN_LEFT, x, y, ..
                } => {
                    s.drag.pointer_held = true;
                    s.drag.pointer_start_y = *y;
                    s.refresh_now();
                    s.activate_drag(*x, *y);
                }
                PointerEvent::Move { x, y, dx, dy, .. } => {
                    if !s.drag.pointer_held {
                        return;
                    }
                    s.refresh_now();
                    s.on_move(*x, *y, *dx, *dy, *y - s.drag.pointer_start_y);
                }
                PointerEvent::ButtonRelease { button: BTN_LEFT, .. } => {
                    if !s.drag.pointer_held {
                        return;
                    }
                    s.drag.pointer_held = false;
                    s.refresh_now();
                    let total_dy = s.drag.finger_y - s.drag.pointer_start_y;
                    s.end_drag(total_dy);
                }
                PointerEvent::Leave { .. } => {
                    if s.drag.pointer_held {
                        s.drag.pointer_held = false;
                        s.refresh_now();
                        let total_dy = s.drag.finger_y - s.drag.pointer_start_y;
                        s.end_drag(total_dy);
                    }
                }
                _ => {}
            }),
        );

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
