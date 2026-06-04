use taffy::{Layout, Style};
use utils::{Color, Point, Size as USize};

use crate::{Render, RenderCommand, WidgetList};

#[crate::widget]
pub struct Div<T: WidgetList> {
    pub color: Color,
    pub border_color: Color,
    pub border_radius: f32,
    pub border_thickness: f32,
    pub z: f32,
    #[widget(child)]
    pub children: T,
}

impl<T: WidgetList> Div<T> {
    pub fn new(style: Style, children: T) -> Self {
        Self {
            node_id: taffy::NodeId::new(u64::MAX),
            style,
            color: Color::TRANSPARENT,
            border_color: Color::TRANSPARENT,
            border_radius: 0.0,
            border_thickness: 0.0,
            z: 0.0,
            children,
        }
    }
}

impl<T: WidgetList> Render for Div<T> {
    fn render(&self, layout: &Layout, abs_pos: Point) -> Vec<RenderCommand> {
        vec![RenderCommand::DrawQuad {
            color: self.color,
            border_color: self.border_color,
            origin: abs_pos,
            z: self.z,
            size: USize::new(layout.size.width, layout.size.height),
            border_radius: self.border_radius,
            border_thickness: self.border_thickness,
        }]
    }
}
