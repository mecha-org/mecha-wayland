use renderer::commands::Color;
use taffy::Style;
use taffy::prelude::*;
use ui::Point;

use ui::Widget;
use ui::WidgetTree;
use ui::widgets::Div;
use ui::{Render, RenderCommand};

#[ui::widget]
pub struct Slider {
    #[widget(child)]
    pub div: Div<Div<()>>,
    pub value: f32,
}

impl Slider {
    pub fn new(value: f32) -> Self {
        let style = Style {
            display: Display::Flex,
            size: Size {
                width: length(25.0_f32),
                height: length(100.0_f32),
            },
            ..Default::default()
        };
        // background parent div
        let div_style = Style {
            // display: Display::Flex,
            size: Size {
                width: percent(1.0_f32),
                height: percent(1.0_f32),
            },
            align_items: Some(AlignItems::End),
            justify_content: Some(JustifyContent::Center),
            ..Default::default()
        };
        // foreground rect
        let mut rect = Div::new(
            Style {
                size: Size {
                    width: percent(1.0_f32),
                    height: percent(value),
                },
                ..Default::default()
            },
            (),
        );
        rect.color = Color::rgb(0.5, 0.5, 0.5); // foreground
        let mut div = Div::new(div_style, rect);
        div.color = Color::rgb(0.2, 0.2, 0.2); // background

        Self {
            node_id: taffy::NodeId::new(u64::MAX),
            style,
            div,
            value,
        }
    }

    pub fn set_value(&mut self, tree: &mut WidgetTree, value: f32) {
        self.value = value;
        self.div.children.set_style(
            tree,
            Style {
                size: Size {
                    width: percent(1.0_f32),
                    height: percent(value),
                },
                ..Default::default()
            },
        );
        tree.mark_dirty(self.div.children.node_id()).unwrap();
    }

    pub fn calculate_delta(&self, x: f64, y: f64, rect: utils::Rect, current: i32) -> i32 {
        let normalized = 1.0 + ((rect.origin.y() - y as f32) / rect.size.height());
        let new_count = (normalized * 10.0).round() as i32;
        new_count - current
    }
}

impl Render for Slider {
    fn render(&self, layout: &taffy::Layout, abs_pos: Point) -> Vec<RenderCommand> {
        let id: u64 = self.node_id.into();
        vec![RenderCommand::RegisterHitArea {
            id,
            rect: utils::Rect::new(
                abs_pos.x(),
                abs_pos.y(),
                layout.size.width,
                layout.size.height,
            ),
        }]
    }
}
