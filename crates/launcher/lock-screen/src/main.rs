#![recursion_limit = "4096"]

mod atlas {
    include!(concat!(env!("OUT_DIR"), "/ui_gen.rs"));
}

mod handlers;
mod layer_ui;
mod lock_ui;
mod render;
mod surface;
mod time;
mod widgets;

use std::collections::HashMap;

use app::prelude::State;
use interactivity::hit::HitAreaRegistry;
use interactivity::{InteractivityState, KeyEvent};
use layer_ui::LayerUi;
use lock_ui::LockUi;
use timer::{Timer, TimerEvent, TimerId};
use utils::Color;
use wayland::Wayland;
use widgets::clock::{ClockChanged, ClockUpdate};

const UNLOCK_THRESHOLD: f32 = 150.0;
const COLOR_LAYER_BG: Color = Color::rgb(0.10, 0.10, 0.12);
const COLOR_LOCK_BG: Color = Color::rgb(0.05, 0.05, 0.07);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum LockMode {
    #[default]
    Unlocked,
    Locking,
    Locked,
}

#[derive(Clone, Copy, Debug)]
struct OutputInfo {
    global_name: u32,
    id: u32,
}

#[derive(State)]
struct AppState {
    ring: io_ring::Ring,
    wayland: Wayland,
    renderer: renderer::Renderer,
    timer: Timer,
    interactivity: InteractivityState,

    clock_timer_id: Option<TimerId>,

    /// Maps `wl_callback` id -> `wl_surface` id for frame-callback routing.
    callback_map: HashMap<u32, u32>,

    outputs: Vec<OutputInfo>,
    mode: LockMode,
    lock_id: Option<u32>,

    layer_ui: Option<LayerUi>,

    /// Keyed by `wl_surface_id` of the lock surface.
    lock_uis: HashMap<u32, LockUi>,

    /// Y position where the current pointer drag started (absolute surface coords).
    /// `None` when no button is held, or when the press was outside the circle.
    pub(crate) pointer_drag_start_y: Option<f64>,

    /// Hit areas registered during the last lock-surface render.
    /// Populated by `render::render_frame` from `RegisterHitArea` commands.
    pub(crate) hit_areas: HitAreaRegistry,

    /// `true` while a touch gesture that began on the circle is in progress.
    pub(crate) touch_on_circle: bool,
}

impl AppState {
    fn new() -> Self {
        let ring = io_ring::Ring::default();
        let wayland = Wayland::new(ring.get_proxy()).expect("wayland connection");
        let mut renderer = renderer::Renderer::new().expect("renderer");

        use renderer::commands::*;
        renderer.init_command_queue::<ClearColor>();
        renderer.init_command_queue::<DrawRect>();
        renderer.init_command_queue::<DrawQuad>();
        renderer.init_command_queue::<DrawMonochromeSprite>();
        renderer.init_command_queue::<DrawText>();

        let timer = Timer::new(ring.get_proxy());

        Self {
            ring,
            wayland,
            renderer,
            timer,
            interactivity: InteractivityState::new(),
            clock_timer_id: None,
            callback_map: HashMap::new(),
            outputs: Vec::new(),
            mode: LockMode::default(),
            lock_id: None,
            layer_ui: None,
            lock_uis: HashMap::new(),
            pointer_drag_start_y: None,
            hit_areas: HitAreaRegistry::new(),
            touch_on_circle: false,
        }
    }

    fn alloc_id(&mut self) -> u32 {
        self.wayland.alloc_id()
    }

    pub(crate) fn get_or_create_output(&mut self) -> Option<u32> {
        let (name, ver) = self.wayland.registry.find("wl_output")?;
        if let Some(o) = self.outputs.iter().find(|o| o.global_name == name) {
            Some(o.id)
        } else {
            let id = self.alloc_id();
            self.wayland
                .registry
                .bind(name, "wl_output", ver.min(4), id);
            self.outputs.push(OutputInfo {
                global_name: name,
                id,
            });
            Some(id)
        }
    }

    pub(crate) fn setup_layer_surface(&mut self) {
        use wayland::zwlr_layer_shell::{KeyboardInteractivity, Layer};

        let _ = self.get_or_create_output();

        let wl_surface_id = self.wayland.compositor.create_surface();
        self.wayland.surface.register(wl_surface_id);

        let layer_surface_id =
            self.wayland
                .layer_shell
                .get_layer_surface(wl_surface_id, 0, Layer::Top, "lock-screen");
        self.wayland.layer_surface.register(layer_surface_id);

        self.wayland
            .layer_surface
            .set_size(layer_surface_id, 400, 200);
        self.wayland
            .layer_surface
            .set_keyboard_interactivity(layer_surface_id, KeyboardInteractivity::OnDemand);

        self.layer_ui = Some(LayerUi::new(wl_surface_id));

        self.wayland.surface.commit(wl_surface_id);
        self.wayland.flush();
    }

    pub(crate) fn trigger_lock(&mut self) {
        if self.mode != LockMode::Unlocked {
            return;
        }
        self.mode = LockMode::Locking;

        let lock_id = self.alloc_id();
        self.wayland.session_lock.set_id(lock_id);
        self.wayland.session_lock_manager.lock(lock_id);
        self.lock_id = Some(lock_id);

        if let Some(output_id) = self.get_or_create_output() {
            let wl_surface_id = self.wayland.compositor.create_surface();
            self.wayland.surface.register(wl_surface_id);

            let lock_surface_id = self.alloc_id();
            self.wayland
                .session_lock
                .get_lock_surface(lock_surface_id, wl_surface_id, output_id);
            self.wayland.session_lock_surface.register(lock_surface_id);

            self.lock_uis
                .insert(wl_surface_id, LockUi::new(wl_surface_id, lock_surface_id));
        }

        self.wayland.flush();
    }

    pub(crate) fn trigger_unlock(&mut self) {
        if self.mode != LockMode::Locked {
            return;
        }
        self.wayland.session_lock.unlock_and_destroy();
        self.cleanup_lock();
    }

    pub(crate) fn cleanup_lock(&mut self) {
        for (_, lock_ui) in std::mem::take(&mut self.lock_uis) {
            self.wayland
                .session_lock_surface
                .destroy(lock_ui.lock_surface_id);
            lock_ui.surface.destroy(&mut self.renderer);
        }
        self.lock_id = None;
        self.mode = LockMode::Unlocked;

        let sync_cb = self.alloc_id();
        self.wayland.display.sync(sync_cb);
        self.wayland.flush();
    }

    pub(crate) fn redraw_lock_ui(&mut self, wl_surface_id: u32) {
        if let Some(ui) = self.lock_uis.get_mut(&wl_surface_id) {
            ui.recompute_layout();
            let cmds = ui.render_commands();
            render::collect_hit_areas(&mut self.hit_areas, &cmds);
            ui.surface.request_redraw(
                &mut self.renderer,
                &mut self.wayland,
                &mut self.callback_map,
                |r| {
                    render::render_frame(r, cmds, COLOR_LOCK_BG);
                },
            );
        }
    }

    pub(crate) fn redraw_all_lock_surfaces(&mut self) {
        let keys: Vec<u32> = self.lock_uis.keys().copied().collect();
        for key in keys {
            self.redraw_lock_ui(key);
        }
    }
}

fn main() {
    let state = AppState::new();

    let mut app = app::App::new(state)
        .mount(io_ring::module())
        .mount(wayland::module())
        .mount(timer::module())
        .mount(interactivity::module())
        .mount(app::Module::new().on(|s: &mut AppState, _: &app::Start| {
            s.renderer.upload_atlas(&atlas::UI).expect("atlas upload");
        }))
        .mount(app::Module::new().on(|s: &mut AppState, ev: &TimerEvent| {
            let (h, m, ..) = time::try_clock_tick(s.clock_timer_id, ev)?;
            time::arm_clock(
                &mut s.timer,
                &mut s.clock_timer_id,
                time::Precision::Minutes,
            );
            Some(ClockUpdate(h, m))
        }))
        .mount(app::Module::new().on(|s: &mut AppState, ev: &ClockUpdate| {
            let changed = s
                .lock_uis
                .values_mut()
                .fold(false, |acc, ui| ui.update_clock(ev.0, ev.1) || acc);
            if changed { Some(ClockChanged) } else { None }
        }))
        .mount(app::Module::new().on(|s: &mut AppState, _: &ClockChanged| {
            s.redraw_all_lock_surfaces();
        }))
        .mount(
            app::Module::new().on(|s: &mut AppState, _: &wayland::Initilised| {
                s.setup_layer_surface();
                time::arm_clock(
                    &mut s.timer,
                    &mut s.clock_timer_id,
                    time::Precision::Minutes,
                );
            }),
        )
        .mount(app::Module::new().on(handlers::on_layer_surface_configured))
        .mount(app::Module::new().on(handlers::on_lock_surface_configured))
        .mount(app::Module::new().on(
            |s: &mut AppState, ev: &wayland::ExtSessionLockEvent| match ev {
                wayland::ExtSessionLockEvent::Locked => {
                    s.mode = LockMode::Locked;
                }
                wayland::ExtSessionLockEvent::Finished => {
                    s.cleanup_lock();
                }
            },
        ))
        .mount(app::Module::new().on(handlers::on_frame_done))
        .mount(app::Module::new().on(handlers::on_buffer_release))
        .mount(app::Module::new().on(|s: &mut AppState, ev: &KeyEvent| {
            if let KeyEvent::Press { key, modifiers, .. } = ev {
                // Alt + L -> lock
                if modifiers.alt && *key == 38 && s.mode == LockMode::Unlocked {
                    s.trigger_lock();
                }
            }
        }))
        .mount(app::Module::new().on(handlers::on_touch))
        .mount(app::Module::new().on(handlers::on_pointer));

    app.dispatch(&app::Start);
    loop {
        app.dispatch(&app::PrePoll);
        app.dispatch(&app::Poll);
    }
}
