#![recursion_limit = "4096"]

mod atlas {
    include!(concat!(env!("OUT_DIR"), "/ui_gen.rs"));
}
mod button;
mod renderer;
mod slider;

use app::prelude::*;
use interactivity::InteractivityState;
use interactivity::hit::{HitArea, HitAreaRegistry};
use std::os::fd::AsRawFd;
use std::time::Duration;

use assets::AtlasId;
use button::Button;
use slider::Slider;
use taffy::Style;
use taffy::prelude::*;
use ui::widgets::{Div, Text};
use ui::{RenderCommand, Widget, WidgetTree};

use ::renderer::commands::{ClearColor, Color, DrawMonochromeSprite, DrawQuad, DrawRect, DrawText};
use io_ring::Ring;
use timer::{Relative, Timer};
use wayland::Wayland;

const DRM_FORMAT_ARGB8888: u32 = 0x34325241;
const MIN_VOLUME: i32 = 0;
const MAX_VOLUME: i32 = 100;
const STEP_SIZE: i32 = 10;

type RowDiv = Div<(Button, Text, Button)>;
type RootDiv = Div<(Text, Slider, RowDiv)>;

struct UiState {
    tree: WidgetTree,
    root: RootDiv,
    count: i32,
    surface_id: u32,
    surface_size: (i32, i32),
    dmabuf: [Option<::renderer::RenderableSurface<::renderer::DmaBuf>>; 2],
    wl_buf_ids: [u32; 2],
    buf_in_flight: [bool; 2],
    hit_areas: HitAreaRegistry,
}

impl UiState {
    fn new() -> Self {
        let (tree, root) = build_ui(atlas::UI.id);
        Self {
            tree,
            root,
            count: 0,
            surface_id: 0,
            surface_size: (0, 0),
            dmabuf: [None, None],
            wl_buf_ids: [0, 0],
            buf_in_flight: [false, false],
            hit_areas: HitAreaRegistry::new(),
        }
    }

    fn set_count(&mut self, count: i32) {
        self.count = count.clamp(MIN_VOLUME, MAX_VOLUME);
        self.root
            .children
            .2
            .children
            .1
            .set_text(&mut self.tree, format!("{}", self.count));
        self.root.children.1.set_value(self.count as f32);
        self.root.children.1.update_ui(&mut self.tree);
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
        self.rebuild_hit_areas();
    }

    fn rebuild_hit_areas(&mut self) {
        self.hit_areas.clear();
        for cmd in self.render_commands() {
            if let RenderCommand::RegisterHitArea { id, rect } = cmd {
                self.hit_areas.push(HitArea { id, rect });
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

        Self {
            ring,
            timer,
            wayland,
            renderer,
            interactivity: InteractivityState::new(),
            ui: UiState::new(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum CounterEvent {
    Updated { new_count: i32 },
}

impl app::Event for CounterEvent {}

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

                let layer_surface_id =
                    s.wayland
                        .layer_shell
                        .get_layer_surface(surface_id, 0, Layer::Top, "counter");
                s.wayland.layer_surface.register(layer_surface_id);
                s.wayland.layer_surface.set_size(layer_surface_id, 400, 360);
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

                let w = if *width == 0 { 256i32 } else { *width as i32 };
                let h = if *height == 0 { 256i32 } else { *height as i32 };
                s.ui.surface_size = (w, h);

                let surface0 = s
                    .renderer
                    .create_surface::<::renderer::DmaBuf>(w as u32, h as u32)
                    .expect("dmabuf surface 0");
                let surface1 = s
                    .renderer
                    .create_surface::<::renderer::DmaBuf>(w as u32, h as u32)
                    .expect("dmabuf surface 1");

                let buf_id0 = create_wl_buffer(&mut s.wayland, &surface0, w, h);
                let buf_id1 = create_wl_buffer(&mut s.wayland, &surface1, w, h);
                s.wayland.wl_buffer.register(buf_id0);
                s.wayland.wl_buffer.register(buf_id1);
                s.ui.wl_buf_ids = [buf_id0, buf_id1];

                s.ui.recompute_layout();

                s.renderer.active_surface(&surface0);
                render_ui(&mut s.renderer, &s.ui);

                s.ui.dmabuf = [Some(surface0), Some(surface1)];
                s.ui.buf_in_flight = [true, false];

                s.wayland.layer_surface.ack_configure(*id, *serial);
                s.wayland.surface.attach(s.ui.surface_id, buf_id0, 0, 0);
                s.wayland.surface.damage(s.ui.surface_id, 0, 0, w, h);

                let cb_id = s.wayland.surface.frame(s.ui.surface_id);
                s.wayland.callback.register_frame(cb_id);

                s.wayland.surface.commit(s.ui.surface_id);
                s.wayland.flush();
            },
        ))
        .mount(
            app::Module::new().on(|s: &mut AppState, _: &wayland::WlCallbackEvent| {
                let free_idx = if !s.ui.buf_in_flight[0] {
                    0
                } else if !s.ui.buf_in_flight[1] {
                    1
                } else {
                    return;
                };

                let surface = s.ui.dmabuf[free_idx].as_ref().unwrap();
                s.renderer.active_surface(surface);
                render_ui(&mut s.renderer, &s.ui);

                let (w, h) = s.ui.surface_size;
                s.wayland
                    .surface
                    .attach(s.ui.surface_id, s.ui.wl_buf_ids[free_idx], 0, 0);
                s.wayland.surface.damage(s.ui.surface_id, 0, 0, w, h);

                let cb_id = s.wayland.surface.frame(s.ui.surface_id);
                s.wayland.callback.register_frame(cb_id);

                s.wayland.surface.commit(s.ui.surface_id);
                s.ui.buf_in_flight[free_idx] = true;
                s.wayland.flush();
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
                // Key 1 is escape
                if let interactivity::KeyEvent::Press { key: 1, .. } = ev {
                    std::process::exit(0);
                }
            }),
        )
        .mount(
            app::Module::new().on(|s: &mut AppState, ev: &interactivity::PointerEvent| {
                if let interactivity::PointerEvent::ButtonPress {
                    button: 272, x, y, ..
                } = ev
                    && let Some(delta) = calculate_delta(&s.ui, *x, *y)
                {
                    let new_count = s.ui.count + delta;
                    s.ui.set_count(new_count);
                    s.ui.recompute_layout();
                }
            }),
        )
        .mount(
            app::Module::new().on(|s: &mut AppState, ev: &wayland::TouchEvent| {
                if let wayland::TouchEvent::Down { x, y, .. } = ev {
                    if let Some(delta) = calculate_delta(&s.ui, *x, *y) {
                        let new_count = s.ui.count + delta;
                        s.ui.set_count(new_count);
                        s.ui.recompute_layout();
                    }
                }
            }),
        )
        .mount(
            app::Module::new().on(|s: &mut AppState, ev: &interactivity::TouchEvent| {
                if let interactivity::TouchEvent::Motion { x, y, .. } = ev
                    && let Some(delta) = calculate_delta(&s.ui, *x, *y)
                {
                    let new_count = s.ui.count + delta;
                    s.ui.set_count(new_count);
                    s.ui.recompute_layout();
                }
            }),
        )
        .mount(app::Module::new().on(|s: &mut AppState, _: &app::Start| {
            s.timer.start_timer(Relative {
                duration: Duration::from_secs(2),
                repeat: true,
            });
        }))
        .mount(app::Module::new().on(|_: &mut AppState, _: &timer::TimerEvent| {}));

    app.dispatch(&app::Start);
    loop {
        app.dispatch(&app::PrePoll);
        app.dispatch(&app::Poll);
    }
}

fn calculate_delta(ui: &UiState, x: f64, y: f64) -> Option<i32> {
    let hit_id = ui.hit_areas.hit_test(x, y)?;
    let minus_id: u64 = ui.root.children.2.children.0.node_id().into();
    let plus_id: u64 = ui.root.children.2.children.2.node_id().into();
    let slider = &ui.root.children.1;
    let slider_id: u64 = slider.node_id().into();
    if hit_id == minus_id {
        Some(-1 * STEP_SIZE)
    } else if hit_id == plus_id {
        Some(STEP_SIZE)
    } else if hit_id == slider_id {
        let layout = ui.tree.layout(slider.node_id()).unwrap();
        let r = utils::Rect::new(
            layout.location.x,
            layout.location.y,
            layout.size.width,
            layout.size.height,
        );
        let new_value = slider.calculate_new_value(y, r);
        Some((new_value.round() as i32) - ui.count)
    } else {
        None
    }
}

fn render_ui(renderer: &mut ::renderer::Renderer, ui: &UiState) {
    renderer.send_command(ClearColor::rgb(0.32, 0.32, 0.32));

    for cmd in ui.render_commands() {
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
                    color,
                    border_color,
                    origin,
                    z,
                    size,
                    border_radius,
                    border_thickness,
                });
            }
            RenderCommand::DrawText {
                font,
                text,
                origin,
                z,
                color,
                atlas_id: Some(aid),
            } => {
                let texture_id = renderer.get_texture_id(aid);
                renderer.send_command(::renderer::commands::DrawText {
                    font,
                    texture_id,
                    text,
                    origin,
                    z,
                    color,
                });
            }
            RenderCommand::DrawText { atlas_id: None, .. } => {}
            RenderCommand::RegisterHitArea { .. } => {}
        }
    }

    renderer.process_command_queue::<ClearColor>();
    renderer.process_command_queue::<DrawRect>();
    renderer.process_command_queue::<DrawQuad>();
    renderer.process_command_queue::<DrawMonochromeSprite>();
    renderer.process_command_queue::<DrawText>();
    renderer.finish();
}

fn build_ui(atlas_id: AtlasId) -> (WidgetTree, RootDiv) {
    let mut tree = WidgetTree::new();

    let mut title = Text::new(Style::default());
    title.font = Some(&atlas::UI_FONT_INTER_24);
    title.text = "Volume".to_string();
    title.color = Color::WHITE;
    title.z = 0.95;
    title.atlas_id = Some(atlas_id);

    let slider = Slider::new(MIN_VOLUME as f32, MIN_VOLUME as f32, MAX_VOLUME as f32);

    let mut minus = Button::new("-");
    minus.div.color = Color::rgb(0.2, 0.4, 0.9);
    minus.div.border_color = Color::rgb(0.4, 0.6, 1.0);
    minus.div.border_radius = 12.0;
    minus.div.border_thickness = 2.0;
    minus.div.z = 1.0;
    minus.div.children.font = Some(&atlas::UI_FONT_INTER_24);
    minus.div.children.color = Color::WHITE;
    minus.div.children.z = 0.4;
    minus.div.children.atlas_id = Some(atlas_id);

    let mut count_text = Text::new(Style::default());
    count_text.font = Some(&atlas::UI_FONT_INTER_24);
    count_text.text = "0".to_string();
    count_text.color = Color::WHITE;
    count_text.z = 0.95;
    count_text.atlas_id = Some(atlas_id);

    let mut plus = Button::new("+");
    plus.div.color = Color::rgb(0.2, 0.7, 0.3);
    plus.div.border_color = Color::rgb(0.4, 0.9, 0.5);
    plus.div.border_radius = 12.0;
    plus.div.border_thickness = 2.0;
    plus.div.z = 1.0;
    plus.div.children.font = Some(&atlas::UI_FONT_INTER_24);
    plus.div.children.color = Color::WHITE;
    plus.div.children.z = 0.5;
    plus.div.children.atlas_id = Some(atlas_id);

    let row_style = Style {
        display: Display::Flex,
        flex_direction: FlexDirection::Row,
        size: Size {
            width: percent(1.0_f32),
            height: length(52.0_f32),
        },
        padding: Rect {
            left: length(60.0_f32),
            right: length(60.0_f32),
            top: zero(),
            bottom: zero(),
        },
        justify_content: Some(JustifyContent::SpaceBetween),
        align_items: Some(AlignItems::Center),
        ..Default::default()
    };
    let row = Div::new(row_style, (minus, count_text, plus));

    let root_style = Style {
        display: Display::Flex,
        flex_direction: FlexDirection::Column,
        justify_content: Some(JustifyContent::Center),
        align_items: Some(AlignItems::Center),
        size: Size {
            width: percent(1.0_f32),
            height: percent(1.0_f32),
        },
        gap: Size {
            width: zero(),
            height: length(40.0_f32),
        },
        ..Default::default()
    };
    let mut root = Div::new(root_style, (title, slider, row));
    root.color = Color::rgb(0.16, 0.16, 0.18);
    root.border_color = Color::rgb(0.30, 0.30, 0.35);
    root.border_radius = 20.0;
    root.border_thickness = 2.0;

    root.build_tree(&mut tree);

    (tree, root)
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
        .create_immed(params_id, width, height, DRM_FORMAT_ARGB8888, 0);
    wayland.buf_params.destroy(params_id);
    buf_id
}
