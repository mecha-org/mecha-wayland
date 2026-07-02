extern crate self as ui;

use assets::BakedFont;
use interactivity::InteractivityState;
use taffy::{AvailableSpace, Layout, NodeId, Size, Style, TaffyTree};
use utils::{Color, Rect, Size as USize};

pub use utils::Point;

pub use ui_macro::widget;

pub mod widgets;

pub type WidgetTree = TaffyTree<Box<dyn Measure>>;

pub fn compute_layout(tree: &mut WidgetTree, node: NodeId, available_space: Size<AvailableSpace>) {
    tree.compute_layout_with_measure(
        node,
        available_space,
        |known_dims, avail, _node_id, ctx, _style| {
            ctx.map_or(Size::ZERO, |m| m.measure(known_dims, avail))
        },
    )
    .unwrap();
}

pub enum RenderCommand {
    DrawQuad {
        color: Color,
        border_color: Color,
        origin: Point,
        z: f32,
        size: USize,
        border_radius: f32,
        border_thickness: f32,
    },
    DrawText {
        font: &'static BakedFont,
        text: String,
        origin: Point,
        z: f32,
        color: Color,
    },
    DrawMonochromeSprite {
        atlas_id: assets::AtlasId,
        region: assets::SpriteRegion,
        origin: Point,
        z: f32,
        size: USize,
        color: Color,
    },
    RegisterHitArea {
        id: u64,
        rect: Rect,
    },
}

pub trait Measure {
    fn measure(
        &self,
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
    ) -> Size<f32>;
}

pub trait Render {
    fn render(&self, layout: &Layout, abs_pos: Point) -> Vec<RenderCommand>;
}

pub trait Widget: Render {
    fn node_id(&self) -> NodeId;
    fn style(&self) -> &Style;
    fn build_tree(&mut self, tree: &mut WidgetTree) -> NodeId;
    fn render_node(&mut self, layout: &Layout, tree: &WidgetTree, offset: Point) -> Vec<RenderCommand>;
}

pub trait WidgetList {
    fn build_children(&mut self, tree: &mut WidgetTree) -> Vec<NodeId>;
    fn render_children(&mut self, tree: &WidgetTree, parent_abs: Point) -> Vec<RenderCommand>;
    fn on_event(&mut self, _interactivity: &InteractivityState, _tree: &mut WidgetTree) -> bool {
        false
    }
    fn touch_config(&self) -> Option<interactivity::touch::TouchConfig> {
        None
    }
    fn gesture_config(&self) -> Option<interactivity::gesture::GestureConfig> {
        None
    }
}

impl WidgetList for () {
    fn build_children(&mut self, _: &mut WidgetTree) -> Vec<NodeId> {
        vec![]
    }
    fn render_children(&mut self, _: &WidgetTree, _: Point) -> Vec<RenderCommand> {
        vec![]
    }
}

impl<W: Widget> WidgetList for W {
    fn build_children(&mut self, tree: &mut WidgetTree) -> Vec<NodeId> {
        vec![self.build_tree(tree)]
    }

    fn render_children(&mut self, tree: &WidgetTree, parent_abs: Point) -> Vec<RenderCommand> {
        let layout = tree.layout(self.node_id()).unwrap();
        self.render_node(layout, tree, parent_abs)
    }
}

impl<A: WidgetList> WidgetList for (A,) {
    fn build_children(&mut self, tree: &mut WidgetTree) -> Vec<NodeId> {
        self.0.build_children(tree)
    }

    fn render_children(&mut self, tree: &WidgetTree, parent_abs: Point) -> Vec<RenderCommand> {
        self.0.render_children(tree, parent_abs)
    }

    fn on_event(&mut self, interactivity: &InteractivityState, tree: &mut WidgetTree) -> bool {
        self.0.on_event(interactivity, tree)
    }
}

impl<A: WidgetList, B: WidgetList> WidgetList for (A, B) {
    fn build_children(&mut self, tree: &mut WidgetTree) -> Vec<NodeId> {
        let mut ids = self.0.build_children(tree);
        ids.extend(self.1.build_children(tree));
        ids
    }

    fn render_children(&mut self, tree: &WidgetTree, parent_abs: Point) -> Vec<RenderCommand> {
        let mut commands = self.0.render_children(tree, parent_abs);
        commands.extend(self.1.render_children(tree, parent_abs));
        commands
    }

    fn on_event(&mut self, interactivity: &InteractivityState, tree: &mut WidgetTree) -> bool {
        let a = self.0.on_event(interactivity, tree);
        let b = self.1.on_event(interactivity, tree);
        a || b
    }
}

impl<A: WidgetList, B: WidgetList, C: WidgetList> WidgetList for (A, B, C) {
    fn build_children(&mut self, tree: &mut WidgetTree) -> Vec<NodeId> {
        let mut ids = self.0.build_children(tree);
        ids.extend(self.1.build_children(tree));
        ids.extend(self.2.build_children(tree));
        ids
    }

    fn render_children(&mut self, tree: &WidgetTree, parent_abs: Point) -> Vec<RenderCommand> {
        let mut commands = self.0.render_children(tree, parent_abs);
        commands.extend(self.1.render_children(tree, parent_abs));
        commands.extend(self.2.render_children(tree, parent_abs));
        commands
    }

    fn on_event(&mut self, interactivity: &InteractivityState, tree: &mut WidgetTree) -> bool {
        let a = self.0.on_event(interactivity, tree);
        let b = self.1.on_event(interactivity, tree);
        let c = self.2.on_event(interactivity, tree);
        a || b || c
    }
}
