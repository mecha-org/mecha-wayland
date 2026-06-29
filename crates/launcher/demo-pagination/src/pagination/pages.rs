use taffy::{Layout, NodeId, Style};
use ui::{Point, Render, RenderCommand, Widget, WidgetList, WidgetTree};
use utils::Rect;

use crate::pagination::state::PagerState;

pub struct Pages<T: WidgetList> {
    node_id: NodeId,
    style: Style,
    pub state: PagerState,
    pub children: T,
}

impl<T: WidgetList> Pages<T> {
    pub fn new(style: Style, state: PagerState, children: T) -> Self {
        Self {
            node_id: NodeId::new(0),
            style,
            state,
            children,
        }
    }
}

impl<T: WidgetList> Render for Pages<T> {
    fn render(&self, layout: &Layout, abs_pos: Point) -> Vec<RenderCommand> {
        vec![RenderCommand::RegisterHitArea {
            id: self.node_id.into(),
            rect: Rect::new(
                abs_pos.x(),
                abs_pos.y(),
                layout.size.width,
                layout.size.height,
            ),
        }]
    }
}

impl<T: WidgetList> Widget for Pages<T> {
    fn node_id(&self) -> taffy::NodeId {
        self.node_id
    }

    fn style(&self) -> &Style {
        &self.style
    }

    fn build_tree(&mut self, tree: &mut WidgetTree) -> taffy::NodeId {
        let child_ids = self.children.build_children(tree);
        let mut style = self.style.clone();
        style.display = taffy::Display::Flex;
        style.flex_direction = taffy::FlexDirection::Row;
        let id = tree.new_with_children(style, &child_ids).unwrap();
        self.node_id = id;
        id
    }

    fn render_node(&self, layout: &Layout, tree: &WidgetTree, offset: Point) -> Vec<RenderCommand> {
        let abs_pos = Point::new(
            offset.x() + layout.location.x,
            offset.y() + layout.location.y,
        );
        let mut commands = self.render(layout, abs_pos);

        // Slide all children by the offset calculated from animation_offset + drag_offset
        let visual_index = self.state.current_visual_offset();
        let page_w = layout.size.width;
        let dx = -(visual_index * page_w) + self.state.drag_offset;
        let translation = Point::new(dx, 0.0);

        // Shift the parent's absolute coordinates passed down to rendering children
        let adjusted_offset =
            Point::new(abs_pos.x() + translation.x(), abs_pos.y() + translation.y());
        commands.extend(self.children.render_children(tree, adjusted_offset));

        commands
    }
}
