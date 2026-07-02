#![recursion_limit = "4096"]

mod atlas {
    include!(concat!(env!("OUT_DIR"), "/ui_gen.rs"));
}
mod notification_entry;
mod renderer;

use app::prelude::*;
use interactivity::InteractivityState;
use interactivity::hit::{HitArea, HitAreaRegistry};
use interactivity::pointer::PointerEvent;
use interactivity::touch::{DragState, SwipeDirection, TouchEvent};
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

use drm_fourcc::DrmFourcc;
use notification_entry::{
    CardContent, DISMISS_SIGNAL, DRAG_THRESHOLD, EntryPhase, NotificationEntry,
    PlainNotificationContent,
};
use taffy::Style;
use taffy::prelude::*;
use ui::widgets::{Div, Text};
use ui::{RenderCommand, Widget, WidgetTree};

use ::renderer::commands::{ClearColor, Color, DrawMonochromeSprite, DrawQuad, DrawRect, DrawText};
use io_ring::Ring;
use timer::Timer;
use wayland::Wayland;

const TOUCH_SLOP: f64 = 20.0;
const HOLD_DURATION_MS: u32 = 500;
const CLICK_MAX_DISTANCE: f64 = 10.0;
const CLICK_MAX_DURATION_MS: u64 = 300;
const BTN_LEFT: u32 = 272;

type NotificationList = Div<(
    NotificationEntry<CardContent>,
    NotificationEntry<CardContent>,
    NotificationEntry<CardContent>,
)>;
type RootDiv = Div<(Text, NotificationList)>;

struct ActiveGesture {
    entry_idx: usize,
    start_x: f64,
    start_y: f64,
    start_time: u32,
    mono_start: Duration,
    hold_cancelled: bool,
    hold_fired: bool,
}

struct UiState {
    tree: WidgetTree,
    root: RootDiv,
    now: Duration,
    active_gesture: Option<ActiveGesture>,
    surface_id: u32,
    surface_size: (i32, i32),
    dmabuf: [Option<::renderer::RenderableSurface<::renderer::DmaBuf>>; 2],
    wl_buf_ids: [u32; 2],
    buf_in_flight: [bool; 2],
    hit_areas: HitAreaRegistry,
    pending_callback_id: u32,
    callback_set_at: Option<Instant>,
    needs_redraw: bool,
    cached_commands: Vec<RenderCommand>,
}

impl UiState {
    fn new() -> Self {
        let (mut tree, mut root) = build_ui();
        compute_initial_layout(&mut tree, &mut root);

        Self {
            tree,
            root,
            now: animation::monotonic_now(),
            active_gesture: None,
            surface_id: 0,
            surface_size: (0, 0),
            dmabuf: [None, None],
            wl_buf_ids: [0, 0],
            buf_in_flight: [false, false],
            hit_areas: HitAreaRegistry::new(),
            pending_callback_id: 0,
            callback_set_at: None,
            needs_redraw: false,
            cached_commands: Vec::new(),
        }
    }

    fn find_entry_idx(&self, x: f64, y: f64) -> Option<usize> {
        let hit_id = self.hit_areas.hit_test(x, y)?;
        for i in 0..3 {
            let entry_id: u64 = match i {
                0 => self.root.children.1.children.0.node_id().into(),
                1 => self.root.children.1.children.1.node_id().into(),
                2 => self.root.children.1.children.2.node_id().into(),
                _ => continue,
            };
            if hit_id == entry_id && self.entry(i).phase != EntryPhase::Animating {
                return Some(i);
            }
        }
        None
    }

    fn entry(&self, idx: usize) -> &NotificationEntry<CardContent> {
        match idx {
            0 => &self.root.children.1.children.0,
            1 => &self.root.children.1.children.1,
            2 => &self.root.children.1.children.2,
            _ => panic!("invalid entry index"),
        }
    }

    fn entry_mut(&mut self, idx: usize) -> &mut NotificationEntry<CardContent> {
        match idx {
            0 => &mut self.root.children.1.children.0,
            1 => &mut self.root.children.1.children.1,
            2 => &mut self.root.children.1.children.2,
            _ => panic!("invalid entry index"),
        }
    }

    fn begin_gesture(&mut self, idx: usize, start_x: f64, start_y: f64, start_time: u32) {
        self.active_gesture = Some(ActiveGesture {
            entry_idx: idx,
            start_x,
            start_y,
            start_time,
            mono_start: animation::monotonic_now(),
            hold_cancelled: false,
            hold_fired: false,
        });
    }

    fn end_gesture(&mut self) {
        self.active_gesture = None;
    }

    fn refresh_now(&mut self) {
        self.now = animation::monotonic_now();
    }

    fn tick_animations(&mut self) -> bool {
        let mut any_active = false;

        for i in 0..3 {
            let entry = match i {
                0 => &mut self.root.children.1.children.0,
                1 => &mut self.root.children.1.children.1,
                2 => &mut self.root.children.1.children.2,
                _ => continue,
            };
            any_active |= entry.tick(&mut self.tree, self.now);
        }

        let hold_idx = self.active_gesture.as_ref().and_then(|g| {
            if !g.hold_fired
                && !g.hold_cancelled
                && self.now.saturating_sub(g.mono_start)
                    >= Duration::from_millis(HOLD_DURATION_MS as u64)
            {
                Some(g.entry_idx)
            } else {
                None
            }
        });
        if let Some(idx) = hold_idx {
            self.active_gesture.as_mut().unwrap().hold_fired = true;
            let entry = match idx {
                0 => &mut self.root.children.1.children.0,
                1 => &mut self.root.children.1.children.1,
                2 => &mut self.root.children.1.children.2,
                _ => return any_active,
            };
            entry.trigger_hold(&mut self.tree);
        }

        any_active
    }

    fn recompute_layout(&mut self) {
        let (w, h) = self.surface_size;
        ui::compute_layout(
            &mut self.tree,
            self.root.node_id(),
            taffy::Size {
                width: AvailableSpace::Definite(w as f32),
                height: AvailableSpace::Definite(h as f32),
            },
        );
        self.cached_commands = self.render_commands();
        self.rebuild_hit_areas();
    }

    fn rebuild_hit_areas(&mut self) {
        self.hit_areas.clear();
        for cmd in &self.cached_commands {
            if let RenderCommand::RegisterHitArea { id, rect } = cmd {
                self.hit_areas.push(HitArea {
                    id: *id,
                    rect: *rect,
                });
            }
        }
    }

    fn render_commands(&self) -> Vec<RenderCommand> {
        let layout = self.tree.layout(self.root.node_id()).unwrap();
        self.root
            .render_node(layout, &self.tree, ui::Point::new(0.0, 0.0))
    }
}

#[derive(State)]
struct AppState {
    ring: Ring,
    timer: Timer,
    wayland: Wayland,
    interactivity: InteractivityState,
    renderer: ::renderer::Renderer,
    ui: UiState,
}

impl AppState {
    fn new() -> Self {
        let ring = Ring::default();
        let timer = Timer::new(ring.get_proxy());
        let wayland = Wayland::new(ring.get_proxy()).expect("failed to create wayland connection");
        let renderer = ::renderer::Renderer::new().expect("failed to create renderer");
        let ui = UiState::new();
        Self {
            ring,
            timer,
            wayland,
            renderer,
            interactivity: InteractivityState::new(),
            ui,
        }
    }
}

fn main() {
    let state = AppState::new();

    let mut app = app::App::new(state)
        .mount(io_ring::module())
        .mount(timer::module())
        .mount(renderer::module())
        .mount(wayland::module())
        .mount(interactivity::module())
        .mount(app::Module::new().on(|s: &mut AppState, _: &app::Start| {
            s.renderer
                .upload_atlas(&atlas::UI)
                .expect("failed to upload atlas");
        }))
        .mount(
            app::Module::new().on(|s: &mut AppState, _: &wayland::Initilised| {
                use wayland::zwlr_layer_shell::{KeyboardInteractivity, Layer};
                let surface_id = s.wayland.compositor.create_surface();
                s.wayland.surface.register(surface_id);
                s.ui.surface_id = surface_id;
                let layer_surface_id = s.wayland.layer_shell.get_layer_surface(
                    surface_id,
                    0,
                    Layer::Top,
                    "notification",
                );
                s.wayland.layer_surface.register(layer_surface_id);
                s.wayland.layer_surface.set_size(layer_surface_id, 400, 500);
                s.wayland
                    .layer_surface
                    .set_keyboard_interactivity(layer_surface_id, KeyboardInteractivity::OnDemand);
                s.wayland.surface.commit(surface_id);
                s.wayland.flush();
            }),
        )
        .mount(app::Module::new().on(
            |s: &mut AppState, ev: &wayland::zwlr_layer_shell::LayerSurfaceEvent| {
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
                let w = if *width == 0 { 400i32 } else { *width as i32 };
                let h = if *height == 0 { 500i32 } else { *height as i32 };
                s.ui.surface_size = (w, h);
                let surface0 = s
                    .renderer
                    .create_surface::<::renderer::DmaBuf>(w as u32, h as u32)
                    .expect("dmabuf 0");
                let surface1 = s
                    .renderer
                    .create_surface::<::renderer::DmaBuf>(w as u32, h as u32)
                    .expect("dmabuf 1");
                let buf_id0 = create_wl_buffer(&mut s.wayland, &surface0, w, h);
                let buf_id1 = create_wl_buffer(&mut s.wayland, &surface1, w, h);
                s.wayland.wl_buffer.register(buf_id0);
                s.wayland.wl_buffer.register(buf_id1);
                s.ui.wl_buf_ids = [buf_id0, buf_id1];
                s.ui.dmabuf = [Some(surface0), Some(surface1)];
                s.ui.buf_in_flight = [false, false];
                s.wayland.layer_surface.ack_configure(*id, *serial);
                s.ui.refresh_now();
                try_redraw(s, false);
            },
        ))
        .mount(
            app::Module::new().on(|s: &mut AppState, _: &app::PrePoll| {
                if let Some(at) = s.ui.callback_set_at {
                    if at.elapsed().as_millis() > 200 {
                        s.ui.pending_callback_id = 0;
                        s.ui.callback_set_at = None;
                    }
                }
                if s.ui.pending_callback_id == 0 {
                    s.ui.refresh_now();
                    let animating = s.ui.tick_animations();
                    if animating || s.ui.needs_redraw {
                        s.ui.needs_redraw = false;
                        if !try_redraw(s, animating) {
                            s.ui.needs_redraw = true;
                        }
                    }
                }
            }),
        )
        .mount(
            app::Module::new().on(|s: &mut AppState, ev: &wayland::WlCallbackEvent| {
                let wayland::WlCallbackEvent::Done {
                    id,
                    callback_data: _,
                } = ev;
                if *id != s.ui.pending_callback_id {
                    return;
                }
                s.ui.pending_callback_id = 0;
                let had_pending = s.ui.callback_set_at.take().is_some();

                s.ui.refresh_now();
                let animating = s.ui.tick_animations();
                let active_gesture = s.ui.active_gesture.is_some();

                if s.ui.needs_redraw || animating || active_gesture || had_pending {
                    s.ui.needs_redraw = false;
                    if !try_redraw(s, animating) {
                        s.ui.needs_redraw = true;
                    }
                }
            }),
        )
        .mount(
            app::Module::new().on(|s: &mut AppState, ev: &wayland::WlBufferEvent| {
                let wayland::WlBufferEvent::Release { id } = ev;
                for i in 0..2 {
                    if s.ui.wl_buf_ids[i] == *id {
                        s.ui.buf_in_flight[i] = false;
                        break;
                    }
                }
            }),
        )
        .mount(
            app::Module::new().on(|_: &mut AppState, ev: &interactivity::KeyEvent| {
                if let interactivity::KeyEvent::Press { key: 1, .. } = ev {
                    std::process::exit(0);
                }
            }),
        )
        .mount(
            app::Module::new().on(|s: &mut AppState, ev: &TouchEvent| match ev {
                TouchEvent::Down { x, y, time, .. } => {
                    if let Some(idx) = s.ui.find_entry_idx(*x, *y) {
                        s.ui.begin_gesture(idx, *x, *y, *time);
                        request_redraw(s);
                    }
                }

                TouchEvent::Motion { x, y, time, .. } => {
                    let hold: Option<usize>;
                    {
                        let Some(ref mut a) = s.ui.active_gesture else {
                            return;
                        };
                        let elapsed = time.saturating_sub(a.start_time);
                        let td = (x - a.start_x, y - a.start_y);
                        if (td.0 * td.0 + td.1 * td.1).sqrt() > TOUCH_SLOP {
                            a.hold_cancelled = true;
                        }
                        hold = if !a.hold_fired && !a.hold_cancelled && elapsed >= HOLD_DURATION_MS
                        {
                            a.hold_fired = true;
                            Some(a.entry_idx)
                        } else {
                            None
                        };
                    }
                    if let Some(idx) = hold {
                        let entry = match idx {
                            0 => &mut s.ui.root.children.1.children.0,
                            1 => &mut s.ui.root.children.1.children.1,
                            2 => &mut s.ui.root.children.1.children.2,
                            _ => return,
                        };
                        entry.trigger_hold(&mut s.ui.tree);
                        request_redraw(s);
                    }
                }

                TouchEvent::Drag {
                    state,
                    total_dx,
                    total_dy,
                    ..
                } => {
                    let Some(ref mut a) = s.ui.active_gesture else {
                        return;
                    };
                    if (total_dx * total_dx + total_dy * total_dy).sqrt() > TOUCH_SLOP {
                        a.hold_cancelled = true;
                    }
                    let idx = a.entry_idx;
                    match state {
                        DragState::Move => {
                            let o = (*total_dx as f32).clamp(-200.0, 200.0);
                            s.ui.entry_mut(idx).set_drag_offset(o);
                            request_redraw(s);
                        }
                        DragState::End => {
                            let dx = *total_dx as f32;
                            if dx.abs() >= DRAG_THRESHOLD {
                                s.ui.entry_mut(idx)
                                    .finish_drag(animation::monotonic_now(), dx);
                            }
                            s.ui.end_gesture();
                            request_redraw(s);
                        }
                        _ => {}
                    }
                }

                TouchEvent::Swipe { direction, .. } => {
                    let idx = s.ui.active_gesture.as_ref().map(|a| a.entry_idx);
                    if let Some(idx) = idx {
                        let dir = match direction {
                            SwipeDirection::Right => DISMISS_SIGNAL,
                            SwipeDirection::Left => -DISMISS_SIGNAL,
                            _ => 0.0,
                        };
                        if dir != 0.0 {
                            s.ui.entry_mut(idx).dismiss(animation::monotonic_now(), dir);
                        }
                    }
                    s.ui.end_gesture();
                    request_redraw(s);
                }

                TouchEvent::Tap { x, y, .. } => {
                    if let Some(idx) = s.ui.find_entry_idx(*x, *y) {
                        s.ui.entry_mut(idx).tap_flash();
                        request_redraw(s);
                    }
                    s.ui.end_gesture();
                }

                TouchEvent::Up { .. } => {
                    let (idx, offset) = match s.ui.active_gesture.as_ref() {
                        Some(a) => (a.entry_idx, s.ui.entry(a.entry_idx).current_offset()),
                        None => {
                            s.ui.end_gesture();
                            return;
                        }
                    };
                    if offset.abs() >= DRAG_THRESHOLD {
                        s.ui.entry_mut(idx)
                            .finish_drag(animation::monotonic_now(), offset);
                    }
                    s.ui.end_gesture();
                    request_redraw(s);
                }

                TouchEvent::Cancel => s.ui.end_gesture(),
                _ => {}
            }),
        )
        .mount(
            app::Module::new().on(|s: &mut AppState, ev: &PointerEvent| match ev {
                PointerEvent::ButtonPress {
                    button: BTN_LEFT,
                    x,
                    y,
                    time,
                } => {
                    if let Some(idx) = s.ui.find_entry_idx(*x, *y) {
                        s.ui.begin_gesture(idx, *x, *y, *time);
                        request_redraw(s);
                    }
                }

                PointerEvent::Move { x, y, time, .. } => {
                    let hold: Option<usize>;
                    {
                        let Some(ref mut a) = s.ui.active_gesture else {
                            return;
                        };
                        let elapsed = time.saturating_sub(a.start_time);
                        let td = (x - a.start_x, y - a.start_y);
                        if (td.0 * td.0 + td.1 * td.1).sqrt() > TOUCH_SLOP {
                            a.hold_cancelled = true;
                        }
                        hold = if !a.hold_fired && !a.hold_cancelled && elapsed >= HOLD_DURATION_MS
                        {
                            a.hold_fired = true;
                            Some(a.entry_idx)
                        } else {
                            None
                        };
                        let idx = a.entry_idx;
                        let o = (td.0 as f32).clamp(-200.0, 200.0);
                        s.ui.entry_mut(idx).set_drag_offset(o);
                    }
                    if let Some(idx) = hold {
                        let entry = match idx {
                            0 => &mut s.ui.root.children.1.children.0,
                            1 => &mut s.ui.root.children.1.children.1,
                            2 => &mut s.ui.root.children.1.children.2,
                            _ => return,
                        };
                        entry.trigger_hold(&mut s.ui.tree);
                    }
                    request_redraw(s);
                }

                PointerEvent::ButtonRelease {
                    button: BTN_LEFT,
                    x,
                    y,
                    time,
                } => {
                    let (idx, dx, dy, dur) = match s.ui.active_gesture.as_ref() {
                        Some(a) => {
                            let dur = if *time > a.start_time {
                                *time - a.start_time
                            } else {
                                0
                            };
                            (a.entry_idx, x - a.start_x, y - a.start_y, dur)
                        }
                        None => {
                            s.ui.end_gesture();
                            return;
                        }
                    };
                    let dist = (dx * dx + dy * dy).sqrt();

                    if dist < CLICK_MAX_DISTANCE && (dur as u64) < CLICK_MAX_DURATION_MS {
                        s.ui.entry_mut(idx).tap_flash();
                        s.ui.entry_mut(idx).spring_back(animation::monotonic_now());
                    } else {
                        let dx = dx as f32;
                        if dx.abs() >= DRAG_THRESHOLD {
                            s.ui.entry_mut(idx)
                                .finish_drag(animation::monotonic_now(), dx);
                        }
                    }
                    s.ui.end_gesture();
                    request_redraw(s);
                }

                PointerEvent::Leave { .. } => {
                    if let Some(ref a) = s.ui.active_gesture {
                        let idx = a.entry_idx;
                        let offset = s.ui.entry(idx).current_offset();
                        let now = animation::monotonic_now();
                        if offset.abs() >= DRAG_THRESHOLD {
                            s.ui.entry_mut(idx).finish_drag(now, offset);
                        }
                        s.ui.end_gesture();
                        request_redraw(s);
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

fn try_redraw(s: &mut AppState, animating: bool) -> bool {
    let free_idx = if !s.ui.buf_in_flight[0] {
        0
    } else if !s.ui.buf_in_flight[1] {
        1
    } else {
        return false;
    };

    let surface = s.ui.dmabuf[free_idx].as_ref().unwrap();
    s.renderer.active_surface(surface);

    s.ui.recompute_layout();
    render_ui(&mut s.renderer, &s.ui.cached_commands);

    if animating && s.ui.pending_callback_id == 0 {
        let cb_id = s.wayland.surface.frame(s.ui.surface_id);
        s.wayland.callback.register_frame(cb_id);
        s.ui.pending_callback_id = cb_id;
        s.ui.callback_set_at = Some(Instant::now());
    }

    let (w, h) = s.ui.surface_size;
    s.wayland
        .surface
        .attach(s.ui.surface_id, s.ui.wl_buf_ids[free_idx], 0, 0);
    s.wayland.surface.damage(s.ui.surface_id, 0, 0, w, h);
    s.wayland.surface.commit(s.ui.surface_id);
    s.ui.buf_in_flight[free_idx] = true;
    s.wayland.flush();
    true
}

fn request_redraw(s: &mut AppState) {
    s.ui.refresh_now();
    let animating = s.ui.tick_animations();
    s.ui.needs_redraw = true;
    if s.ui.pending_callback_id == 0 {
        s.ui.needs_redraw = false;
        if !try_redraw(s, animating) {
            s.ui.needs_redraw = true;
        }
    }
}

fn build_ui() -> (WidgetTree, RootDiv) {
    let mut tree = WidgetTree::new();

    let mut header = Text::new(Style::default());
    header.font = Some(&atlas::UI_FONT_INTER_24);
    header.text = "Notifications".to_string();
    header.color = Color::WHITE;
    header.z = 0.95;

    let mk = |color, title: &str, body: &str| -> NotificationEntry<CardContent> {
        let card = PlainNotificationContent::new(color, title, body);
        let mut e = NotificationEntry::new(card);
        e.font = Some(&atlas::UI_FONT_INTER_16); // bg label font
        e.card.children.1.children.0.font = Some(&atlas::UI_FONT_INTER_16); // title font
        e.card.children.1.children.1.font = Some(&atlas::UI_FONT_INTER_14); // body font
        e.card.children.1.children.1.font = Some(&atlas::UI_FONT_INTER_14);
        e
    };

    let list = Div::new(
        Style {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            size: Size {
                width: percent(1.0_f32),
                height: auto(),
            },
            gap: Size {
                width: zero(),
                height: length(12.0_f32),
            },
            ..Default::default()
        },
        (
            mk(
                Color::rgb(0.29, 0.56, 0.85), // icon: blue
                "Message",
                "Hey, how are you doing today?",
            ),
            mk(
                Color::rgb(0.29, 0.72, 0.45), // icon: green
                "System Update",
                "A new system update is available",
            ),
            mk(
                Color::rgb(0.90, 0.55, 0.20), // icon: orange
                "Reminder",
                "Meeting in 10 minutes",
            ),
        ),
    );

    let mut root = Div::new(
        Style {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            size: Size {
                width: percent(1.0_f32),
                height: percent(1.0_f32),
            },
            padding: taffy::Rect {
                left: length(16.0_f32), // root horizontal padding
                right: length(16.0_f32),
                top: length(20.0_f32),    // root top padding
                bottom: length(20.0_f32), // root bottom padding
            },
            gap: Size {
                width: zero(),
                height: length(20.0_f32), // gap between header and list
            },
            ..Default::default()
        },
        (header, list),
    );
    root.color = Color::rgb(0.14, 0.14, 0.16); // root background: very dark gray
    root.z = 0.0;

    root.build_tree(&mut tree);
    (tree, root)
}

fn compute_initial_layout(tree: &mut WidgetTree, root: &mut RootDiv) {
    ui::compute_layout(
        tree,
        root.node_id(),
        taffy::Size {
            width: AvailableSpace::Definite(400.0), // bootstrap dimensions
            height: AvailableSpace::Definite(500.0), // overwritten by Configured
        },
    );
}

fn render_ui(renderer: &mut ::renderer::Renderer, commands: &[RenderCommand]) {
    renderer.send_command(ClearColor::rgb(0.14, 0.14, 0.16)); // matches root background
    for cmd in commands {
        match cmd {
            ui::RenderCommand::DrawQuad {
                color,
                border_color,
                origin,
                z,
                size,
                border_radius,
                border_thickness,
            } => {
                renderer.send_command(::renderer::commands::DrawQuad {
                    color: *color,
                    border_color: *border_color,
                    origin: *origin,
                    z: *z,
                    size: *size,
                    border_radius: *border_radius,
                    border_thickness: *border_thickness,
                });
            }
            RenderCommand::DrawText { font, text, origin, z, color } => {
                let texture_id = renderer.get_texture_id(font.atlas_id);
                renderer.send_command(::renderer::commands::DrawText {
                    font: *font,
                    texture_id,
                    text: text.clone(),
                    origin: *origin,
                    z: *z,
                    color: *color,
                });
            }
            RenderCommand::RegisterHitArea { .. } => {}
            _ => {}
        }
    }
    renderer.process_command_queue::<ClearColor>();
    renderer.process_command_queue::<DrawRect>();
    renderer.process_command_queue::<DrawQuad>();
    renderer.process_command_queue::<DrawMonochromeSprite>();
    renderer.process_command_queue::<DrawText>();
    renderer.finish();
}

fn create_wl_buffer(
    wayland: &mut Wayland,
    surface: &::renderer::RenderableSurface<::renderer::DmaBuf>,
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
    let buf_id = wayland
        .buf_params
        .create_immed(params_id, width, height, DrmFourcc::Argb8888 as u32, 0);
    wayland.buf_params.destroy(params_id);
    buf_id
}
