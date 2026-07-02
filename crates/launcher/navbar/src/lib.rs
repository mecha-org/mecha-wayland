use assets::BakedFont;
use taffy::{NodeId, Style};
use ui::{Point, RenderCommand, Widget, WidgetList, WidgetTree, widgets::Text};
use utils::Color;

pub struct NavbarUi {
    text: Text,
}

impl NavbarUi {
    pub fn new(font: &'static BakedFont) -> Self {
        let mut text = Text::new(Style::default());
        text.text = "Navbar".to_string();
        text.color = Color::WHITE;
        text.z = 0.5;
        text.font = Some(font);
        Self { text }
    }
}

impl WidgetList for NavbarUi {
    fn build_children(&mut self, tree: &mut WidgetTree) -> Vec<NodeId> {
        vec![self.text.build_tree(tree)]
    }

    fn render_children(&mut self, tree: &WidgetTree, parent_abs: Point) -> Vec<RenderCommand> {
        self.text.render_children(tree, parent_abs)
    }
}
