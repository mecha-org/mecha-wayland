use taffy::Style;
use taffy::prelude::*;
use ui::Point;

use ui::widgets::{Div, Text};
use ui::{Render, RenderCommand};

#[ui::widget]
pub struct Button {
    #[widget(child)]
    pub div: Div<Text>,
}

impl Button {
    pub fn new(label: &str) -> Self {
        let style = Style {
            display: Display::Flex,
            size: Size {
                width: length(110.0_f32),
                height: length(52.0_f32),
            },
            ..Default::default()
        };
        let div_style = Style {
            display: Display::Flex,
            size: Size {
                width: percent(1.0_f32),
                height: percent(1.0_f32),
            },
            align_items: Some(AlignItems::Center),
            justify_content: Some(JustifyContent::Center),
            ..Default::default()
        };
        let mut text = Text::new(Style::default());
        text.text = label.to_string();
        let div = Div::new(div_style, text);

        Self {
            node_id: taffy::NodeId::new(u64::MAX),
            style,
            div,
        }
    }
}

impl Render for Button {
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
