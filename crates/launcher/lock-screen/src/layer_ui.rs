use ui::widgets::{Div, Text};
use ui::{RenderCommand, Widget, WidgetTree, compute_layout};
use utils::Color;

use crate::atlas;
use crate::surface::Surface;

pub struct LayerUi {
    pub surface: Surface,
    tree: WidgetTree,
    container: Div<Text>,
}

impl LayerUi {
    pub fn new(wl_surface_id: u32) -> Self {
        let mut tree = WidgetTree::new();

        let mut hint = Text::new(taffy::Style::default());
        hint.text = "Press Alt + L or click here to lock.".to_string();
        hint.font = Some(&atlas::UI_FONT_MONO_16);
        hint.color = Color::rgb(0.85, 0.85, 0.85);
        hint.z = 0.5;
        hint.atlas_id = Some(atlas::UI.id);

        let container_style = taffy::Style {
            display: taffy::Display::Flex,
            align_items: Some(taffy::AlignItems::Center),
            justify_content: Some(taffy::JustifyContent::Center),
            size: taffy::Size {
                width: taffy::Dimension::percent(1.0),
                height: taffy::Dimension::percent(1.0),
            },
            ..Default::default()
        };

        let mut container = Div::new(container_style, hint);
        container.build_tree(&mut tree);

        Self {
            surface: Surface::new(wl_surface_id),
            tree,
            container,
        }
    }

    /// Re-run Taffy layout using the current surface dimensions.
    pub fn recompute_layout(&mut self) {
        let (w, h) = self.surface.size;
        compute_layout(
            &mut self.tree,
            self.container.node_id(),
            taffy::Size {
                width: taffy::AvailableSpace::Definite(w as f32),
                height: taffy::AvailableSpace::Definite(h as f32),
            },
        );
    }

    /// Collect render commands for the current frame.
    pub fn render_commands(&self) -> Vec<RenderCommand> {
        let layout = self.tree.layout(self.container.node_id()).unwrap();
        self.container
            .render_node(layout, &self.tree, ui::Point::new(0.0, 0.0))
    }
}
