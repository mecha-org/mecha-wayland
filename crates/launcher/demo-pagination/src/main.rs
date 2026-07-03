#![recursion_limit = "4096"]

mod atlas {
    include!(concat!(env!("OUT_DIR"), "/ui_gen.rs"));
}
mod pagination;

use app::prelude::*;
use interactivity::InteractivityState;
use interactivity::hit::{HitArea, HitAreaRegistry};
use renderer::commands::{DrawMonochromeSprite, DrawText};
use std::os::fd::AsRawFd;

use pagination::{PagerState, Pages, process_pager_events};
use taffy::Style;
use taffy::prelude::*;
use ui::widgets::{Div, Text};
use ui::{RenderCommand, Widget, WidgetTree};

use ::renderer::commands::{ClearColor, Color, DrawQuad, DrawRect};
use io_ring::Ring;
use wayland::Wayland;

const DRM_FORMAT_ARGB8888: u32 = 0x34325241;

// Widget layout structures
type Page1Div = Div<Div<Text>>;
type Page2Div = Div<Div<()>>;
type Page3Div = Div<Div<()>>;
type PagerType = Pages<(Page1Div, Page2Div, Page3Div)>;
type RootDiv = Div<(PagerType,)>;

struct UiState {
    tree: WidgetTree,
    root: RootDiv,
    surface_id: u32,
    surface_size: (i32, i32),
    dmabuf: [Option<::renderer::RenderableSurface<::renderer::DmaBuf>>; 2],
    wl_buf_ids: [u32; 2],
    buf_in_flight: [bool; 2],
    hit_areas: HitAreaRegistry,
    frame_callback_pending: bool,
    dirty: bool,
}

impl UiState {
    fn new() -> Self {
        let (tree, root) = build_ui();
        Self {
            tree,
            root,
            surface_id: 0,
            surface_size: (0, 0),
            dmabuf: [None, None],
            wl_buf_ids: [0, 0],
            buf_in_flight: [false, false],
            hit_areas: HitAreaRegistry::new(),
            frame_callback_pending: false,
            dirty: true,
        }
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
    wayland: Wayland,
    interactivity: InteractivityState,
    renderer: ::renderer::Renderer,
    ui: UiState,
    was_animating: bool,
}

impl AppState {
    fn new() -> Self {
        let ring = Ring::default();
        let wayland = Wayland::new(ring.get_proxy()).expect("failed to create wayland connection");
        let mut renderer = ::renderer::Renderer::new().expect("failed to create renderer");

        renderer.init_command_queue::<::renderer::commands::ClearColor>();
        renderer.init_command_queue::<::renderer::commands::DrawRect>();
        renderer.init_command_queue::<::renderer::commands::DrawQuad>();
        renderer.init_command_queue::<::renderer::commands::DrawMonochromeSprite>();
        renderer.init_command_queue::<::renderer::commands::DrawText>();

        Self {
            ring,
            wayland,
            renderer,
            interactivity: InteractivityState::new(),
            ui: UiState::new(),
            was_animating: false,
        }
    }
}

fn main() {
    let state = AppState::new();

    let mut app = app::App::new(state)
        .mount(io_ring::module())
        .mount(wayland::module())
        .mount(interactivity::module())
        .mount(app::Module::new().on(|s: &mut AppState, _: &app::Start| {
            s.renderer.upload_atlas(&atlas::UI).expect("atlas upload");
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
                    "launcher-demo-pagination",
                );
                s.wayland.layer_surface.register(layer_surface_id);
                s.wayland.layer_surface.set_size(layer_surface_id, 540, 620);
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

                let w = if *width == 0 { 480i32 } else { *width as i32 };
                let h = if *height == 0 { 800i32 } else { *height as i32 };
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

                s.ui.dmabuf = [Some(surface0), Some(surface1)];
                s.ui.buf_in_flight = [false, false];

                s.wayland.layer_surface.ack_configure(*id, *serial);

                s.ui.dirty = true;
                redraw(s);
            },
        ))
        .mount(
            app::Module::new().on(|s: &mut AppState, _: &wayland::WlCallbackEvent| {
                s.ui.frame_callback_pending = false;

                let is_animating = s.ui.root.children.0.state.animation_offset.is_animating();

                if s.ui.dirty || is_animating || s.was_animating {
                    s.was_animating = is_animating;
                    redraw(s);
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

                if s.ui.dirty {
                    redraw(s);
                }
            }),
        )
        .mount(
            app::Module::new().on(|s: &mut AppState, ev: &interactivity::KeyEvent| {
                if let interactivity::KeyEvent::Press { key: 1, .. } = ev {
                    std::process::exit(0);
                }

                let pager_width = s.ui.surface_size.0 as f32;
                let pager_hit_id = s.ui.root.children.0.node_id().into();

                let mutated = process_pager_events(
                    &mut s.ui.root.children.0.state,
                    None,
                    None,
                    Some(ev),
                    pager_width,
                    pager_hit_id,
                    Some(pager_hit_id),
                );

                if mutated {
                    redraw(s);
                }
            }),
        )
        .mount(
            app::Module::new().on(|s: &mut AppState, ev: &interactivity::PointerEvent| {
                let pager_width = s.ui.surface_size.0 as f32;
                let pager_hit_id = s.ui.root.children.0.node_id().into();

                let active_hit_id = match ev {
                    interactivity::PointerEvent::ButtonPress { x, y, .. } => {
                        s.ui.hit_areas.hit_test(*x, *y)
                    }
                    interactivity::PointerEvent::Move { x, y, .. } => {
                        s.ui.hit_areas.hit_test(*x, *y)
                    }
                    _ => None,
                };

                let is_dragging = s.ui.root.children.0.state.is_dragging;
                let mutated = process_pager_events(
                    &mut s.ui.root.children.0.state,
                    Some(ev),
                    None,
                    None,
                    pager_width,
                    pager_hit_id,
                    active_hit_id.or({
                        if is_dragging {
                            Some(pager_hit_id)
                        } else {
                            None
                        }
                    }),
                );

                if mutated {
                    redraw(s);
                }
            }),
        )
        .mount(
            app::Module::new().on(|s: &mut AppState, ev: &interactivity::TouchEvent| {
                let pager_width = s.ui.surface_size.0 as f32;
                let pager_hit_id = s.ui.root.children.0.node_id().into();

                let active_hit_id = match ev {
                    interactivity::TouchEvent::Drag { x, y, .. } => s.ui.hit_areas.hit_test(*x, *y),
                    _ => None,
                };

                let is_dragging = s.ui.root.children.0.state.is_dragging;
                let mutated = process_pager_events(
                    &mut s.ui.root.children.0.state,
                    None,
                    Some(ev),
                    None,
                    pager_width,
                    pager_hit_id,
                    active_hit_id.or({
                        if is_dragging {
                            Some(pager_hit_id)
                        } else {
                            None
                        }
                    }),
                );

                if mutated {
                    redraw(s);
                }
            }),
        );

    app.dispatch(&app::Start);
    loop {
        app.dispatch(&app::PrePoll);
        app.dispatch(&app::Poll);
    }
}

fn redraw(s: &mut AppState) {
    if s.ui.frame_callback_pending {
        s.ui.dirty = true;
        return;
    }

    let free_idx = if !s.ui.buf_in_flight[0] {
        0
    } else if !s.ui.buf_in_flight[1] {
        1
    } else {
        s.ui.dirty = true;
        return;
    };

    s.ui.dirty = false;

    let surface = s.ui.dmabuf[free_idx].as_ref().unwrap();
    s.renderer.active_surface(surface);
    // s.ui.recompute_layout();
    render_ui(&mut s.renderer, &s.ui);

    let (w, h) = s.ui.surface_size;
    s.wayland
        .surface
        .attach(s.ui.surface_id, s.ui.wl_buf_ids[free_idx], 0, 0);
    s.wayland.surface.damage(s.ui.surface_id, 0, 0, w, h);

    let cb_id = s.wayland.surface.frame(s.ui.surface_id);
    s.wayland.callback.register_frame(cb_id);
    s.ui.frame_callback_pending = true;

    s.wayland.surface.commit(s.ui.surface_id);
    s.ui.buf_in_flight[free_idx] = true;
    s.wayland.flush();
}

fn render_ui(renderer: &mut ::renderer::Renderer, ui: &UiState) {
    renderer.send_command(ClearColor::rgb(0.08, 0.08, 0.10));

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
            ui::RenderCommand::DrawText { font, text, origin, z, color } => {
                let texture_id = renderer.get_texture_id(font.atlas_id);
                renderer.send_command(::renderer::commands::DrawText {
                    font,
                    texture_id,
                    text,
                    origin,
                    z,
                    color,
                });
            }
            ui::RenderCommand::RegisterHitArea { .. } => {}
            _ => {}
        }
    }

    renderer.process_command_queue::<ClearColor>();
    renderer.process_command_queue::<DrawRect>();
    renderer.process_command_queue::<DrawMonochromeSprite>();
    renderer.process_command_queue::<DrawText>();
    renderer.process_command_queue::<DrawQuad>();
    renderer.finish();
}

fn build_ui() -> (WidgetTree, RootDiv) {
    let mut tree = WidgetTree::new();

    let card_style = Style {
        display: Display::Flex,
        justify_content: Some(JustifyContent::Center),
        align_items: Some(AlignItems::Center),
        size: Size {
            width: percent(0.85_f32),
            height: percent(0.85_f32),
        },
        ..Default::default()
    };

    let mut text1 = Text::new(Style::default());
    text1.text = "Page 1".to_string();
    text1.font = Some(&atlas::UI_FONT_MONO_24);
    text1.color = Color::rgb(1.0, 1.0, 1.0);
    text1.z = 0.5;

    let mut card1 = Div::new(card_style.clone(), text1);
    card1.color = Color::rgb(0.9, 0.35, 0.3);
    card1.border_radius = 24.0;
    card1.z = 0.2;

    let mut card2 = Div::new(card_style.clone(), ());
    card2.color = Color::rgb(0.2, 0.65, 0.45);
    card2.border_radius = 24.0;
    card2.z = 0.2;

    let mut card3 = Div::new(card_style.clone(), ());
    card3.color = Color::rgb(0.3, 0.45, 0.9);
    card3.border_radius = 24.0;
    card3.z = 0.2;

    let page_style = Style {
        display: Display::Flex,
        justify_content: Some(JustifyContent::Center),
        align_items: Some(AlignItems::Center),
        size: Size {
            width: percent(1.0_f32),
            height: percent(1.0_f32),
        },
        flex_shrink: 0.0,
        ..Default::default()
    };

    let page1 = Div::new(page_style.clone(), card1);
    let page2 = Div::new(page_style.clone(), card2);
    let page3 = Div::new(page_style.clone(), card3);

    let pager_style = Style {
        size: Size {
            width: percent(1.0_f32),
            height: percent(1.0_f32),
        },
        ..Default::default()
    };

    let pager_state = PagerState::new(3);
    let pager = Pages::new(pager_style, pager_state, (page1, page2, page3));

    let root_style = Style {
        display: Display::Flex,
        flex_direction: FlexDirection::Column,
        justify_content: Some(JustifyContent::Center),
        align_items: Some(AlignItems::Center),
        size: Size {
            width: percent(1.0_f32),
            height: percent(1.0_f32),
        },
        ..Default::default()
    };

    let mut root = Div::new(root_style, (pager,));
    root.color = Color::rgb(0.08, 0.08, 0.10);

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
