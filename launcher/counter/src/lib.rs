#![recursion_limit = "4096"]

mod button;

use assets::BakedFont;
use button::Button;
use ui::EventCtx;

#[derive(Debug)]
pub struct CounterChanged(pub i32);
impl app::Event for CounterChanged {}
use taffy::prelude::*;
use taffy::Style;
use ui::widgets::{Div, Text};
use ui::{Point, RenderCommand, Widget, WidgetList, WidgetTree};
use utils::Color;

type RowDiv = Div<(Button, Button)>;
type RootDiv = Div<(Text, Text, RowDiv)>;

pub struct CounterUi {
    root: RootDiv,
    pub count: i32,
    minus_rect: utils::Rect,
    plus_rect: utils::Rect,
}

impl CounterUi {
    pub fn new(font_24: &'static BakedFont, font_100: &'static BakedFont) -> Self {
        Self {
            root: make_root(font_24, font_100),
            count: 0,
            minus_rect: utils::Rect::ZERO,
            plus_rect: utils::Rect::ZERO,
        }
    }
}

impl WidgetList for CounterUi {
    fn build_children(&mut self, tree: &mut WidgetTree) -> Vec<taffy::NodeId> {
        vec![self.root.build_tree(tree)]
    }

    fn render_children(&mut self, tree: &WidgetTree, parent_abs: Point) -> Vec<RenderCommand> {
        let commands = self.root.render_children(tree, parent_abs);
        let minus_id: u64 = self.root.children.2.children.0.node_id().into();
        let plus_id: u64 = self.root.children.2.children.1.node_id().into();
        for cmd in &commands {
            if let RenderCommand::RegisterHitArea { id, rect } = cmd {
                if *id == minus_id {
                    self.minus_rect = *rect;
                } else if *id == plus_id {
                    self.plus_rect = *rect;
                }
            }
        }
        commands
    }

    fn on_event(&mut self, ctx: &mut EventCtx) {
        if ctx.interactivity().is_clicked(self.minus_rect) {
            self.count -= 1;
            self.root
                .children
                .1
                .set_text(ctx.tree(), self.count.to_string());
            println!("Event Sent!");
            ctx.dispatch(CounterChanged(self.count));
        } else if ctx.interactivity().is_clicked(self.plus_rect) {
            self.count += 1;
            self.root
                .children
                .1
                .set_text(ctx.tree(), self.count.to_string());
            ctx.dispatch(CounterChanged(self.count));
        }
    }

    fn touch_config(&self) -> Option<interactivity::touch::TouchConfig> {
        Some(interactivity::touch::TouchConfig {
            tap_max_distance: 10.0,
            tap_max_duration: std::time::Duration::from_millis(250),
        })
    }

    fn gesture_config(&self) -> Option<interactivity::gesture::GestureConfig> {
        Some(interactivity::gesture::GestureConfig {
            swipe_min_distance: 30.0,
            swipe_max_duration: std::time::Duration::from_millis(400),
        })
    }
}

fn make_root(font_24: &'static BakedFont, font_100: &'static BakedFont) -> RootDiv {
    let mut title = Text::new(Style::default());
    title.font = Some(font_24);
    title.text = "Counter".to_string();
    title.color = Color::WHITE;
    title.z = 0.95;

    let mut count_text = Text::new(Style::default());
    count_text.font = Some(font_100);
    count_text.text = "0".to_string();
    count_text.color = Color::WHITE;
    count_text.z = 0.95;

    let mut minus = Button::new("-");
    minus.div.color = Color::rgb(0.2, 0.4, 0.9);
    minus.div.border_color = Color::rgb(0.4, 0.6, 1.0);
    minus.div.border_radius = 12.0;
    minus.div.border_thickness = 2.0;
    minus.div.z = 1.0;
    minus.div.children.font = Some(font_24);
    minus.div.children.color = Color::WHITE;
    minus.div.children.z = 0.4;

    let mut plus = Button::new("+");
    plus.div.color = Color::rgb(0.2, 0.7, 0.3);
    plus.div.border_color = Color::rgb(0.4, 0.9, 0.5);
    plus.div.border_radius = 12.0;
    plus.div.border_thickness = 2.0;
    plus.div.z = 1.0;
    plus.div.children.font = Some(font_24);
    plus.div.children.color = Color::WHITE;
    plus.div.children.z = 0.5;

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
    let row = Div::new(row_style, (minus, plus));

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
    let mut root = Div::new(root_style, (title, count_text, row));
    root.color = Color::rgb(0.16, 0.16, 0.18);
    root.border_color = Color::rgb(0.30, 0.30, 0.35);
    root.border_radius = 20.0;
    root.border_thickness = 2.0;

    root
}
