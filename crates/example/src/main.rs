pub mod atlas {
    include!(concat!(env!("OUT_DIR"), "/example_gen.rs"));
}

use app::prelude::*;
use io_ring::Ring;
use renderer::commands::Color;
use taffy::prelude::*;
use taffy::{Size, Style};
use ui::widgets::{BackgroundColor, Button, Disabled, Padding, Text};
use ui::{Damage, OnChange, Point, Render, RenderCommand, Widget};
use utils::Rect;
use wayland::{WlPointerButtonState, WlPointerEvent};
use window_manager::prelude::*;

const FONT: &assets::BakedFont = &atlas::EXAMPLE_FONT_INTER_32;

const BG: Color = Color::from_rgb8(20, 20, 28);
const LABEL: Color = Color::WHITE;

/// Fired by the pointer handler when the left button is pressed.
#[derive(Clone, Copy)]
struct Click(Point);

type DemoButton = Button<(Text,)>;

fn label(s: &str) -> Text {
    let mut t = Text::new(Style::default());
    t.font = Some(FONT);
    t.text = s.into();
    t.color = LABEL;
    t
}

//  Root widget

#[ui::widget]
struct ExampleUi {
    clicks1: u32,
    clicks2: u32,
    clicks3: u32,
    #[widget(child)]
    children: (DemoButton, DemoButton, DemoButton),
}

impl Render for ExampleUi {
    fn render(&self, _layout: &taffy::Layout, _abs_pos: Point) -> Vec<RenderCommand> {
        Vec::new()
    }
}

impl ExampleUi {
    fn new() -> Self {
        Self {
            node_id: taffy::NodeId::new(u64::MAX),
            style: Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                justify_content: Some(JustifyContent::Center),
                align_items: Some(AlignItems::Center),
                size: Size {
                    width: percent(1.0_f32),
                    height: percent(1.0_f32),
                },
                gap: Size {
                    width: length(16.0_f32),
                    height: zero(),
                },
                ..Style::default()
            },
            bounds: Rect::ZERO,
            pending_damage: Damage::None,
            is_opaque: true,
            clicks1: 0,
            clicks2: 0,
            clicks3: 0,
            children: (
                Button::filled(Color::from_rgb8(249, 100, 13), (label("Filled"),))
                    .width(220.0)
                    .height(56.0)
                    .padding(Padding::symmetric(32.0, 12.0))
                    .border_radius(10.0),
                Button::outlined(Color::from_rgb8(99, 102, 241), (label("Outlined"),))
                    .width(220.0)
                    .height(56.0)
                    .padding(Padding::symmetric(32.0, 12.0))
                    .border_radius(10.0),
                Button::new((label("Custom"),))
                    .width(220.0)
                    .height(56.0)
                    .padding(Padding::symmetric(32.0, 12.0))
                    .background(Color::from_rgb8(16, 185, 129))
                    .border_color(Color::from_rgb8(52, 211, 153))
                    .border_thickness(1.5)
                    .border_radius(10.0)
                    .into(),
            ),
        }
    }
}

impl OnChange<Click> for ExampleUi {
    fn damage(&self, _new: &Click) -> Damage {
        Damage::None
    }

    fn change(&mut self, new: Click) {
        if !self.children.0.disabled && self.children.0.own_bounds().contains_point(new.0) {
            self.clicks1 += 1;
            let text = format!("Filled: {}", self.clicks1);
            self.children.0.children.0.set(text);

            let colors = [
                Color::from_rgb8(76, 175, 80),  // Green
                Color::from_rgb8(249, 100, 13), // Orange
            ];
            let new_color = colors[self.clicks1 as usize % colors.len()];
            self.children.0.set(BackgroundColor(new_color));

            if self.clicks1 >= 5 {
                self.children.0.set(Disabled(true));
            }
        } else if !self.children.1.disabled && self.children.1.own_bounds().contains_point(new.0) {
            self.clicks2 += 1;
            let text = format!("Outlined: {}", self.clicks2);
            self.children.1.children.0.set(text);
        } else if !self.children.2.disabled && self.children.2.own_bounds().contains_point(new.0) {
            self.clicks3 += 1;
            let text = format!("Custom: {}", self.clicks3);
            self.children.2.children.0.set(text);
        }
    }
}

// ── App state ─────────────────────────────────────────────────────────────────

#[derive(State)]
struct ExampleState {
    ring: Ring,
    wm: WindowManager,
    #[lens(skip)]
    handle: WindowHandle<ExampleUi>,
    #[lens(skip)]
    pointer: Point,
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() {
    let ring = Ring::default();
    let mut wm = WindowManager::new(ring.proxy());
    wm.upload_atlas(&atlas::EXAMPLE);

    let handle = wm.spawn_window(
        WindowSettings {
            width: 800,
            height: 480,
            clear_color: BG,
            kind: WindowKind::Xdg {
                title: "example".into(),
            },
            touch_config: None,
            gesture_config: None,
        },
        ExampleUi::new(),
    );

    let state = ExampleState {
        ring,
        wm,
        handle,
        pointer: Point::new(-1.0, -1.0),
    };

    let mut app = app::App::new(state)
        .mount(io_ring::module())
        .mount(window_manager::module())
        .mount(app::Module::new().on(on_pointer));

    app.dispatch(&app::Start);
    loop {
        app.dispatch(&app::PrePoll);
        app.dispatch(&app::Poll);
    }
}

// ── Input handler ─────────────────────────────────────────────────────────────

const BTN_LEFT: u32 = 0x110;

fn on_pointer(s: &mut ExampleState, ev: &WlPointerEvent) {
    match ev {
        WlPointerEvent::Enter {
            surface_x,
            surface_y,
            ..
        }
        | WlPointerEvent::Motion {
            surface_x,
            surface_y,
            ..
        } => {
            s.pointer = Point::new(*surface_x, *surface_y);
        }
        WlPointerEvent::Button {
            state: WlPointerButtonState::Pressed,
            button,
            ..
        } if *button == BTN_LEFT => {
            s.handle.set(Click(s.pointer), &mut s.wm);
        }
        _ => {}
    }
}
