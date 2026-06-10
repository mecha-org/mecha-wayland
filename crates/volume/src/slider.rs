use renderer::commands::Color;
use taffy::Style;
use taffy::prelude::*;
use ui::Point;

// use ui::widgets::{Div, Text};
use ui::widgets::{Div, Rect};
use ui::{Render, RenderCommand};

#[ui::widget]
pub struct Slider {
    #[widget(child)]
    pub div: Div<Rect>,
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
        // background rect parent panel
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
        let mut rect = Rect::new(Style {
            size: Size {
                width: percent(1.0_f32),
                height: percent(value),
            },
            ..Default::default()
        });
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
}

impl Render for Slider {
    fn render(&self, _layout: &taffy::Layout, _abs_pos: Point) -> Vec<RenderCommand> {
        vec![]
    }
}
