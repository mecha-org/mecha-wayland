use assets::{AtlasId, BakedFont};
use taffy::{AvailableSpace, Layout, Size, Style};
use utils::{Color, Point};

use crate::{Measure, Render, RenderCommand, WidgetTree};

#[crate::widget(measure)]
#[derive(Clone)]
pub struct Text {
    pub font: Option<&'static BakedFont>,
    pub text: String,
    pub color: Color,
    pub z: f32,
    pub atlas_id: Option<AtlasId>,
}

impl Text {
    pub fn new(style: Style) -> Self {
        Self {
            node_id: taffy::NodeId::new(u64::MAX),
            style,
            font: None,
            text: String::new(),
            color: Color::WHITE,
            z: 0.0,
            atlas_id: None,
        }
    }

    pub fn placeholder() -> Self {
        Self::new(Style::default())
    }

    pub fn set_text(&mut self, tree: &mut WidgetTree, text: String) {
        self.text = text;
        tree.set_node_context(self.node_id, Some(Box::new(self.clone()))).unwrap();
        tree.mark_dirty(self.node_id).unwrap();
    }

    pub fn set_font(&mut self, tree: &mut WidgetTree, font: Option<&'static BakedFont>) {
        self.font = font;
        tree.set_node_context(self.node_id, Some(Box::new(self.clone()))).unwrap();
        tree.mark_dirty(self.node_id).unwrap();
    }
}

impl Measure for Text {
    fn measure(&self, _known_dimensions: Size<Option<f32>>, _available_space: Size<AvailableSpace>) -> Size<f32> {
        match self.font {
            None => Size::ZERO,
            Some(font) => Size { width: font.measure_width(&self.text), height: font.line_height },
        }
    }
}

impl Render for Text {
    fn render(&self, _layout: &Layout, abs_pos: Point) -> Vec<RenderCommand> {
        let Some(font) = self.font else { return vec![] };
        // DrawText expects baseline-left origin; abs_pos is the text node top-left.
        let origin = Point::new(abs_pos.x(), abs_pos.y() + font.ascent);
        vec![RenderCommand::DrawText {
            font,
            text: self.text.clone(),
            origin,
            z: self.z,
            color: self.color,
            atlas_id: self.atlas_id,
        }]
    }
}
