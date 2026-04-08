use std::collections::HashMap;

use taffy::TaffyTree;

// ── Re-exports — callers only need `layout` as a dependency ──────────────────

pub use taffy::AvailableSpace;
pub use taffy::NodeId;
pub use taffy::geometry::Rect as Edges; // top/right/bottom/left — for padding, margin, inset
pub use taffy::geometry::Size;
pub use taffy::style::{
    AlignItems, AlignSelf, Dimension, Display, FlexDirection, FlexWrap, JustifyContent,
    JustifyItems, LengthPercentage, LengthPercentageAuto, Position, Style,
};

// ── Screen-space Rect ─────────────────────────────────────────────────────────

/// Absolute screen-space rectangle produced by [`Layout::compute`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

// ── Measure trait ─────────────────────────────────────────────────────────────

/// Implemented by the data type `T` to provide intrinsic (content-driven) sizing
/// for leaf nodes. Returning [`Size::ZERO`] (the default) tells taffy to use the
/// node's `Style` dimensions exclusively.
///
/// Text nodes should pre-measure at build time and return the stored size here.
/// Container nodes and fixed-size nodes can use the default no-op implementation.
pub trait Measure {
    fn measure(
        &self,
        known:     Size<Option<f32>>,
        available: Size<AvailableSpace>,
    ) -> Size<f32> {
        let _ = (known, available);
        Size::ZERO
    }
}

// ── Layout ────────────────────────────────────────────────────────────────────

pub struct Layout<T: Measure> {
    taffy:    TaffyTree<T>,
    root:     NodeId,
    computed: HashMap<NodeId, Rect>,
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Scoped tree builder handed to the closure in [`Layout::new`].
pub struct Builder<'a, T: Measure> {
    layout: &'a mut Layout<T>,
    parent: NodeId,
}

impl<'a, T: Measure> Builder<'a, T> {
    /// Add a leaf node (no children) under the current parent.
    pub fn leaf(&mut self, style: Style, data: T) -> NodeId {
        let id = self.layout.taffy
            .new_leaf_with_context(style, data)
            .unwrap();
        self.layout.taffy.add_child(self.parent, id).unwrap();
        id
    }

    /// Add a container node under the current parent. The closure receives a
    /// `Builder` scoped to this node and may return any value `R` (typically
    /// a tuple of `NodeId`s for the children).
    pub fn child<R>(
        &mut self,
        style: Style,
        data:  T,
        build: impl FnOnce(&mut Builder<T>) -> R,
    ) -> (NodeId, R) {
        let id = self.layout.taffy
            .new_leaf_with_context(style, data)
            .unwrap();
        self.layout.taffy.add_child(self.parent, id).unwrap();
        let mut b = Builder { layout: self.layout, parent: id };
        let r = build(&mut b);
        (id, r)
    }
}

// ── Layout impl ───────────────────────────────────────────────────────────────

impl<T: Measure> Layout<T> {
    /// Create the layout tree. The closure receives a [`Builder`] for the root
    /// node and may return any value `R` — typically a tuple of `NodeId`s that
    /// the caller stores for later querying.
    ///
    /// ```ignore
    /// let (mut layout, (label_id, button_id)) = Layout::new(root_style, root_data, |b| {
    ///     let label  = b.leaf(label_style,  label_data);
    ///     let button = b.leaf(button_style, button_data);
    ///     (label, button)
    /// });
    /// ```
    pub fn new<R>(
        style: Style,
        data:  T,
        build: impl FnOnce(&mut Builder<T>) -> R,
    ) -> (Self, R) {
        let mut taffy = TaffyTree::new();
        let root = taffy.new_leaf_with_context(style, data).unwrap();
        let mut layout = Layout { taffy, root, computed: HashMap::new() };
        let mut builder = Builder { layout: &mut layout, parent: root };
        let r = build(&mut builder);
        (layout, r)
    }

    /// Compute layout for the given available screen rectangle.
    ///
    /// Calls `T::measure` for every leaf node (via taffy's measure function),
    /// then resolves all relative positions to absolute screen coordinates.
    /// Call this once at startup, or again whenever the available space changes.
    pub fn compute(&mut self, available: Rect) {
        self.taffy
            .compute_layout_with_measure(
                self.root,
                Size {
                    width:  AvailableSpace::Definite(available.w),
                    height: AvailableSpace::Definite(available.h),
                },
                |known, avail, _id, ctx, _style| {
                    ctx.map(|t| t.measure(known, avail))
                        .unwrap_or(Size::ZERO)
                },
            )
            .unwrap();

        let mut computed = HashMap::new();
        Self::walk(&self.taffy, self.root, available.x, available.y, &mut computed);
        self.computed = computed;
    }

    fn walk(
        taffy: &TaffyTree<T>,
        id:    NodeId,
        ox:    f32,
        oy:    f32,
        map:   &mut HashMap<NodeId, Rect>,
    ) {
        let l = taffy.layout(id).unwrap();
        let abs = Rect {
            x: ox + l.location.x,
            y: oy + l.location.y,
            w: l.size.width,
            h: l.size.height,
        };
        map.insert(id, abs);
        for child in taffy.children(id).unwrap() {
            Self::walk(taffy, child, abs.x, abs.y, map);
        }
    }

    /// Absolute screen rect for `id`. Panics if [`compute`](Self::compute) has
    /// not been called yet.
    pub fn rect(&self, id: NodeId) -> Rect {
        self.computed[&id]
    }

    pub fn data(&self, id: NodeId) -> &T {
        self.taffy.get_node_context(id).unwrap()
    }

    pub fn data_mut(&mut self, id: NodeId) -> &mut T {
        self.taffy.get_node_context_mut(id).unwrap()
    }
}
