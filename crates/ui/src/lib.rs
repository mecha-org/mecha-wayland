extern crate self as ui;

use assets::BakedFont;
use interactivity::InteractivityState;
use std::any::Any;
use taffy::{AvailableSpace, Layout, NodeId, Size, Style, TaffyTree};
use utils::{Color, Rect, Size as USize};

pub use utils::Point;

pub use ui_macro::register_events;
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

pub const Z_STEP: f32 = 1e-3;

pub enum RenderCommand {
    DrawQuad {
        color: Color,
        border_color: Color,
        origin: Point,
        z: f32,
        size: USize,
        border_radius: f32,
        border_thickness: f32,
        /// Solid colour behind this quad, inferred by the render walk. Lets a
        /// translucent interior flatten to the opaque pass when `background.a`
        /// (composited with `color`) reaches 1.
        background: Color,
        /// Author opt-out of the opaque fast path (default `true`).
        is_opaque: bool,
    },
    DrawText {
        font: &'static BakedFont,
        text: String,
        origin: Point,
        z: f32,
        color: Color,
        /// Solid colour behind the glyphs, inferred by the render walk.
        background: Color,
        /// Author opt-out of the opaque fast path (default `true`).
        is_opaque: bool,
    },
    DrawMonochromeSprite {
        atlas_id: assets::AtlasId,
        region: assets::SpriteRegion,
        origin: Point,
        z: f32,
        size: USize,
        color: Color,
        /// Solid colour behind the sprite, inferred by the render walk.
        background: Color,
        /// Author opt-out of the opaque fast path (default `true`).
        is_opaque: bool,
    },
    RegisterHitArea {
        id: u64,
        rect: Rect,
    },
}

impl RenderCommand {
    pub fn stamp(&mut self, z_val: f32, bg: Color, opaque: bool) {
        match self {
            RenderCommand::DrawQuad {
                z,
                background,
                is_opaque,
                ..
            }
            | RenderCommand::DrawText {
                z,
                background,
                is_opaque,
                ..
            }
            | RenderCommand::DrawMonochromeSprite {
                z,
                background,
                is_opaque,
                ..
            } => {
                *z = z_val;
                *background = bg;
                *is_opaque = opaque;
            }
            RenderCommand::RegisterHitArea { .. } => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Damage {
    #[default]
    None,
    Paint(Rect),
    Layout,
}

impl Damage {
    #[inline]
    pub fn paint(rect: Rect) -> Damage {
        if rect.is_empty() {
            Damage::None
        } else {
            Damage::Paint(rect)
        }
    }

    #[inline]
    pub fn union(self, other: Damage) -> Damage {
        match (self, other) {
            (Damage::Layout, _) | (_, Damage::Layout) => Damage::Layout,
            (Damage::None, d) | (d, Damage::None) => d,
            (Damage::Paint(a), Damage::Paint(b)) => Damage::paint(a.union(b)),
        }
    }
}

impl std::ops::BitOr for Damage {
    type Output = Damage;
    #[inline]
    fn bitor(self, rhs: Damage) -> Damage {
        self.union(rhs)
    }
}

impl std::ops::BitOrAssign for Damage {
    #[inline]
    fn bitor_assign(&mut self, rhs: Damage) {
        *self = *self | rhs;
    }
}

/// `damage` runs **first**, while the widget still holds its *old* state and is
/// handed the *new* value, so it can return a tight bound. `change` runs
/// **second** and is `&mut self` only: it applies the value and fans out the
/// *model* consequences — it never touches taffy or the renderer.
pub trait OnChange<T> {
    /// Region dirtied by replacing the current state with `new`. Runs before
    /// [`OnChange::change`]; must not mutate.
    fn damage(&self, new: &T) -> Damage;

    /// Apply `new`, mutating fields and fanning the value out to the model.
    fn change(&mut self, new: T);
}

pub trait Measure {
    fn measure(
        &self,
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
    ) -> Size<f32>;
}

pub struct EventCtx<'a> {
    interactivity: &'a InteractivityState,
    tree: &'a mut WidgetTree,
    buffer: &'a mut Vec<Box<dyn Any>>,
}

impl<'a> EventCtx<'a> {
    pub fn new(
        interactivity: &'a InteractivityState,
        tree: &'a mut WidgetTree,
        buffer: &'a mut Vec<Box<dyn Any>>,
    ) -> Self {
        Self {
            interactivity,
            tree,
            buffer,
        }
    }

    pub fn interactivity(&self) -> &'a InteractivityState {
        self.interactivity
    }

    pub fn tree(&mut self) -> &mut WidgetTree {
        self.tree
    }

    pub fn dispatch<T: app::Event>(&mut self, event: T) {
        self.buffer.push(Box::new(event));
    }
}

pub trait Render {
    fn render(&self, layout: &Layout, abs_pos: Point) -> Vec<RenderCommand>;

    fn fill(&self) -> Color {
        Color::TRANSPARENT
    }
}

pub trait Widget: Render {
    fn node_id(&self) -> NodeId;
    fn style(&self) -> &Style;
    fn own_bounds(&self) -> Rect;
    fn build_tree(&mut self, tree: &mut WidgetTree) -> NodeId;
    fn render_node(
        &mut self,
        layout: &Layout,
        tree: &WidgetTree,
        offset: Point,
        z: f32,
        background: Color,
    ) -> Vec<RenderCommand>;
    fn on_event(&mut self, _ctx: &mut EventCtx) {}
    fn drain_damage(&mut self, _tree: &mut WidgetTree) -> Damage {
        Damage::None
    }
}

pub trait WidgetList {
    fn build_children(&mut self, tree: &mut WidgetTree) -> Vec<NodeId>;
    fn render_children(
        &mut self,
        tree: &WidgetTree,
        parent_abs: Point,
        z: f32,
        background: Color,
    ) -> Vec<RenderCommand>;
    fn bounds(&self) -> Rect {
        Rect::ZERO
    }
    fn on_event(&mut self, _ctx: &mut EventCtx) {}
    fn flush_damage(&mut self, _tree: &mut WidgetTree) -> Damage {
        Damage::None
    }
    fn touch_config(&self) -> Option<interactivity::touch::TouchConfig> {
        None
    }
    fn gesture_config(&self) -> Option<interactivity::gesture::GestureConfig> {
        None
    }
    fn wants_input(&self) -> bool {
        true
    }
}

impl WidgetList for () {
    fn build_children(&mut self, _: &mut WidgetTree) -> Vec<NodeId> {
        vec![]
    }
    fn render_children(
        &mut self,
        _: &WidgetTree,
        _: Point,
        _: f32,
        _: Color,
    ) -> Vec<RenderCommand> {
        vec![]
    }
}

impl<W: Widget> WidgetList for W {
    fn build_children(&mut self, tree: &mut WidgetTree) -> Vec<NodeId> {
        vec![self.build_tree(tree)]
    }

    fn render_children(
        &mut self,
        tree: &WidgetTree,
        parent_abs: Point,
        z: f32,
        background: Color,
    ) -> Vec<RenderCommand> {
        if self.style().display == taffy::style::Display::None {
            return vec![];
        }
        let layout = tree.layout(self.node_id()).unwrap();
        self.render_node(layout, tree, parent_abs, z, background)
    }

    fn bounds(&self) -> Rect {
        <W as Widget>::own_bounds(self)
    }

    fn on_event(&mut self, ctx: &mut EventCtx) {
        <W as Widget>::on_event(self, ctx)
    }

    fn flush_damage(&mut self, tree: &mut WidgetTree) -> Damage {
        <W as Widget>::drain_damage(self, tree)
    }
}

impl<A: WidgetList> WidgetList for (A,) {
    fn build_children(&mut self, tree: &mut WidgetTree) -> Vec<NodeId> {
        self.0.build_children(tree)
    }

    fn render_children(
        &mut self,
        tree: &WidgetTree,
        parent_abs: Point,
        z: f32,
        background: Color,
    ) -> Vec<RenderCommand> {
        self.0.render_children(tree, parent_abs, z, background)
    }

    fn bounds(&self) -> Rect {
        self.0.bounds()
    }

    fn on_event(&mut self, ctx: &mut EventCtx) {
        self.0.on_event(ctx)
    }

    fn flush_damage(&mut self, tree: &mut WidgetTree) -> Damage {
        self.0.flush_damage(tree)
    }
}

impl<A: WidgetList, B: WidgetList> WidgetList for (A, B) {
    fn build_children(&mut self, tree: &mut WidgetTree) -> Vec<NodeId> {
        let mut ids = self.0.build_children(tree);
        ids.extend(self.1.build_children(tree));
        ids
    }

    fn render_children(
        &mut self,
        tree: &WidgetTree,
        parent_abs: Point,
        z: f32,
        background: Color,
    ) -> Vec<RenderCommand> {
        let mut commands = self.0.render_children(tree, parent_abs, z, background);
        commands.extend(self.1.render_children(tree, parent_abs, z, background));
        commands
    }

    fn bounds(&self) -> Rect {
        self.0.bounds().union(self.1.bounds())
    }

    fn on_event(&mut self, ctx: &mut EventCtx) {
        self.0.on_event(ctx);
        self.1.on_event(ctx);
    }

    fn flush_damage(&mut self, tree: &mut WidgetTree) -> Damage {
        self.0.flush_damage(tree) | self.1.flush_damage(tree)
    }
}

impl<A: WidgetList, B: WidgetList, C: WidgetList> WidgetList for (A, B, C) {
    fn build_children(&mut self, tree: &mut WidgetTree) -> Vec<NodeId> {
        let mut ids = self.0.build_children(tree);
        ids.extend(self.1.build_children(tree));
        ids.extend(self.2.build_children(tree));
        ids
    }

    fn render_children(
        &mut self,
        tree: &WidgetTree,
        parent_abs: Point,
        z: f32,
        background: Color,
    ) -> Vec<RenderCommand> {
        let mut commands = self.0.render_children(tree, parent_abs, z, background);
        commands.extend(self.1.render_children(tree, parent_abs, z, background));
        commands.extend(self.2.render_children(tree, parent_abs, z, background));
        commands
    }

    fn bounds(&self) -> Rect {
        self.0
            .bounds()
            .union(self.1.bounds())
            .union(self.2.bounds())
    }

    fn on_event(&mut self, ctx: &mut EventCtx) {
        self.0.on_event(ctx);
        self.1.on_event(ctx);
        self.2.on_event(ctx);
    }

    fn flush_damage(&mut self, tree: &mut WidgetTree) -> Damage {
        self.0.flush_damage(tree) | self.1.flush_damage(tree) | self.2.flush_damage(tree)
    }
}
