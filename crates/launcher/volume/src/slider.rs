use taffy::Style;
use taffy::prelude::*;
use ui::Point;
use utils::Color;

use ui::Widget;
use ui::WidgetTree;
use ui::widgets::Div;
use ui::{Render, RenderCommand};

#[ui::widget]
pub struct Slider {
    #[widget(child)]
    pub div: Div<Div<()>>,
    pub value: f32,
    pub normalized_value: f32,
    pub min: f32,
    pub max: f32,
}

impl Slider {
    pub fn new(value: f32, min: f32, max: f32) -> Self {
        let normalized_value = (value - min) / (max - min);
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
                    height: percent(normalized_value),
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
            normalized_value,
            min,
            max,
        }
    }

    pub fn update_ui(&mut self, tree: &mut WidgetTree) {
        self.div.children.set_style(
            tree,
            Style {
                size: Size {
                    width: percent(1.0_f32),
                    height: percent(self.normalized_value),
                },
                ..Default::default()
            },
        );
        tree.mark_dirty(self.div.children.node_id()).unwrap();
    }

    pub fn set_value(&mut self, value: f32) {
        self.value = value.clamp(self.min, self.max);
        self.normalized_value = self.value / (self.max - self.min);
    }

    pub fn calculate_new_value(&self, y: f32, rect: utils::Rect) -> f32 {
        let normalized = 1.0 + ((rect.origin.y() - y) / rect.size.height());
        normalized.clamp(0.0, 1.0) * (self.max - self.min) + self.min
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
