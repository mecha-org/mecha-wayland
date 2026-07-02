use std::cell::RefCell;
use std::rc::Rc;

use taffy::prelude::*;
use taffy::{Layout, NodeId, Size, Style};
use ui::{self, Render, RenderCommand, Widget, WidgetTree, compute_layout};

#[test]
fn div_set_style_triggers_layout_recompute() {
    use ui::widgets::Div;

    let style = Style {
        size: Size {
            width: length(100.0_f32),
            height: length(50.0_f32),
        },
        ..Style::default()
    };
    let mut div = Div::new(style, (ui::widgets::Text::placeholder(),));
    let mut tree = WidgetTree::new();
    let id = div.build_tree(&mut tree);
    compute_layout(&mut tree, id, Size::MAX_CONTENT);
    assert_eq!(tree.layout(id).unwrap().size.width, 100.0);

    let new_style = Style {
        size: Size {
            width: length(200.0_f32),
            height: length(80.0_f32),
        },
        ..Style::default()
    };
    div.set_style(&mut tree, new_style);
    compute_layout(&mut tree, id, Size::MAX_CONTENT);
    assert_eq!(tree.layout(id).unwrap().size.width, 200.0);
    assert_eq!(tree.layout(id).unwrap().size.height, 80.0);
}

#[test]
fn div_build_tree_wires_children() {
    let mut tree = WidgetTree::new();
    let mut div = build_div();
    let div_id = div.build_tree(&mut tree);
    assert_eq!(div.node_id(), div_id);
    let children = tree.children(div_id).unwrap();
    assert_eq!(children.len(), 1);
}

#[test]
fn div_render_node_is_preorder() {
    let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(vec![]));

    let child = LoggingWidget {
        node_id: NodeId::new(u64::MAX),
        style: Style::default(),
        label: "child",
        log: log.clone(),
    };
    let mut div = LoggingDiv {
        node_id: NodeId::new(u64::MAX),
        style: Style::default(),
        child,
        label: "parent",
        log: log.clone(),
    };

    let mut tree = WidgetTree::new();
    let div_id = div.build_tree(&mut tree);
    compute_layout(&mut tree, div_id, Size::MAX_CONTENT);

    let layout = tree.layout(div_id).unwrap();
    div.render_node(layout, &tree, ui::Point::new(0.0, 0.0));

    assert_eq!(*log.borrow(), vec!["parent", "child"]);
}

#[test]
fn div_render_emits_draw_quad_at_layout_bounds() {
    use ui::widgets::Div;
    use utils::Color;

    let style = Style {
        size: Size {
            width: length(100.0_f32),
            height: length(50.0_f32),
        },
        ..Style::default()
    };
    let mut div = Div::new(style, (ui::widgets::Text::placeholder(),));
    div.color = Color::rgb(1.0, 0.0, 0.0);

    let mut tree = WidgetTree::new();
    let id = div.build_tree(&mut tree);
    compute_layout(&mut tree, id, Size::MAX_CONTENT);
    let layout = tree.layout(id).unwrap();
    let abs_pos = ui::Point::new(layout.location.x, layout.location.y);
    let commands = div.render(layout, abs_pos);

    assert_eq!(commands.len(), 1);
    match &commands[0] {
        RenderCommand::DrawQuad {
            color,
            origin,
            size,
            ..
        } => {
            assert_eq!(*color, Color::rgb(1.0, 0.0, 0.0));
            assert_eq!(origin.x(), abs_pos.x());
            assert_eq!(origin.y(), abs_pos.y());
            assert_eq!(size.width(), layout.size.width);
            assert_eq!(size.height(), layout.size.height);
        }
        _ => panic!("expected DrawQuad"),
    }
}

#[test]
fn div_render_node_collects_commands_preorder() {
    use assets::{BakedFont, GlyphInfo};
    use ui::widgets::{Div, Text};
    use utils::Color;

    static FONT: BakedFont = BakedFont {
        atlas_id: assets::AtlasId(0),
        size: 16.0,
        line_height: 20.0,
        ascent: 0.0,
        glyphs: [GlyphInfo {
            x: 0.0,
            y: 0.0,
            w: 8.0,
            h: 14.0,
            bearing_x: 0.0,
            bearing_y: 11.0,
            advance: 10.0,
        }; 95],
    };

    let mut text = Text::new(Style::default());
    text.font = Some(&FONT);
    text.text = "x".to_string();

    let div_style = Style {
        size: Size {
            width: length(100.0_f32),
            height: length(50.0_f32),
        },
        ..Style::default()
    };
    let mut div = Div::new(div_style, (text,));
    div.color = Color::rgb(0.0, 0.0, 1.0);

    let mut tree = WidgetTree::new();
    let id = div.build_tree(&mut tree);
    compute_layout(&mut tree, id, Size::MAX_CONTENT);
    let layout = tree.layout(id).unwrap();
    let commands = div.render_node(layout, &tree, ui::Point::new(0.0, 0.0));

    // First command: DrawQuad from Div; second: DrawText from Text
    assert_eq!(commands.len(), 2);
    assert!(matches!(commands[0], RenderCommand::DrawQuad { .. }));
    assert!(matches!(commands[1], RenderCommand::DrawText { .. }));
}

#[test]
fn nested_div_renders_children_at_absolute_position() {
    // Regression: children of a non-root Div used to render at parent-relative
    // coordinates because render_node never accumulated the parent's offset.
    use ui::widgets::{Div, Text};
    use utils::Color;

    // Outer div: positioned 50px from the top (padding-top).
    let outer_style = Style {
        display: Display::Flex,
        flex_direction: FlexDirection::Column,
        size: Size {
            width: length(200.0_f32),
            height: length(200.0_f32),
        },
        padding: Rect {
            top: length(50.0_f32),
            left: zero(),
            right: zero(),
            bottom: zero(),
        },
        ..Style::default()
    };
    // Inner div: 40x40, sits inside outer's content area.
    let inner_style = Style {
        size: Size {
            width: length(40.0_f32),
            height: length(40.0_f32),
        },
        ..Style::default()
    };
    let mut inner = Div::new(inner_style, (Text::placeholder(),));
    inner.color = Color::rgb(1.0, 0.0, 0.0);
    let mut outer = Div::new(outer_style, (inner,));

    let mut tree = WidgetTree::new();
    let root_id = outer.build_tree(&mut tree);
    compute_layout(&mut tree, root_id, Size::MAX_CONTENT);

    let root_layout = tree.layout(root_id).unwrap();
    let commands = outer.render_node(root_layout, &tree, ui::Point::new(0.0, 0.0));

    // Commands: outer DrawQuad, inner DrawQuad.
    let inner_quad = commands.iter().find(|c| matches!(c, RenderCommand::DrawQuad { color, .. } if *color == Color::rgb(1.0, 0.0, 0.0)));
    let inner_quad = inner_quad.expect("inner DrawQuad not found");
    match inner_quad {
        RenderCommand::DrawQuad { origin, .. } => {
            // Inner div is in outer's content area which starts at y=50 (padding-top).
            // Its own location.y inside that content area is 0, so absolute y = 50.
            assert_eq!(
                origin.y(),
                50.0,
                "inner div should render at absolute y=50, not at its parent-relative y=0"
            );
        }
        _ => unreachable!(),
    }
}

// ── test helpers ──────────────────────────────────────────────────────────────

fn build_div() -> impl Widget {
    use ui::widgets::{Div, Text};
    Div::new(Style::default(), (Text::placeholder(),))
}

struct LoggingWidget {
    node_id: NodeId,
    style: Style,
    label: &'static str,
    log: Rc<RefCell<Vec<&'static str>>>,
}

impl Render for LoggingWidget {
    fn render(&self, _layout: &Layout, _abs_pos: ui::Point) -> Vec<RenderCommand> {
        self.log.borrow_mut().push(self.label);
        vec![]
    }
}

impl Widget for LoggingWidget {
    fn node_id(&self) -> NodeId {
        self.node_id
    }
    fn style(&self) -> &Style {
        &self.style
    }
    fn build_tree(&mut self, tree: &mut WidgetTree) -> NodeId {
        let id = tree.new_leaf(self.style.clone()).unwrap();
        self.node_id = id;
        id
    }
    fn render_node(
        &mut self,
        layout: &Layout,
        _tree: &WidgetTree,
        offset: ui::Point,
    ) -> Vec<RenderCommand> {
        let abs_pos = ui::Point::new(
            offset.x() + layout.location.x,
            offset.y() + layout.location.y,
        );
        self.render(layout, abs_pos)
    }
}

struct LoggingDiv {
    node_id: NodeId,
    style: Style,
    child: LoggingWidget,
    label: &'static str,
    log: Rc<RefCell<Vec<&'static str>>>,
}

impl Render for LoggingDiv {
    fn render(&self, _layout: &Layout, _abs_pos: ui::Point) -> Vec<RenderCommand> {
        self.log.borrow_mut().push(self.label);
        vec![]
    }
}

impl Widget for LoggingDiv {
    fn node_id(&self) -> NodeId {
        self.node_id
    }
    fn style(&self) -> &Style {
        &self.style
    }
    fn build_tree(&mut self, tree: &mut WidgetTree) -> NodeId {
        let child_id = self.child.build_tree(tree);
        let id = tree
            .new_with_children(self.style.clone(), &[child_id])
            .unwrap();
        self.node_id = id;
        id
    }
    fn render_node(
        &mut self,
        layout: &Layout,
        tree: &WidgetTree,
        offset: ui::Point,
    ) -> Vec<RenderCommand> {
        let abs_pos = ui::Point::new(
            offset.x() + layout.location.x,
            offset.y() + layout.location.y,
        );
        let mut commands = self.render(layout, abs_pos);
        let child_layout = tree.layout(self.child.node_id()).unwrap();
        commands.extend(self.child.render_node(child_layout, tree, abs_pos));
        commands
    }
}

#[test]
fn root_percent_height_resolves_against_available_space() {
    use taffy::prelude::*;
    use taffy::{AvailableSpace, Style};

    let mut tree = ui::WidgetTree::new();
    let mk = |t: &mut ui::WidgetTree, w: f32, h: f32| {
        t.new_leaf(Style {
            size: Size {
                width: length(w),
                height: length(h),
            },
            ..Default::default()
        })
        .unwrap()
    };

    let c1 = mk(&mut tree, 80.0, 24.0);
    let c2 = mk(&mut tree, 60.0, 120.0);
    let c3 = mk(&mut tree, 400.0, 52.0);

    let root_pct = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                justify_content: Some(JustifyContent::Center),
                size: Size {
                    width: percent(1.0),
                    height: percent(1.0),
                },
                gap: Size {
                    width: zero(),
                    height: length(40.0),
                },
                ..Default::default()
            },
            &[c1, c2, c3],
        )
        .unwrap();

    ui::compute_layout(
        &mut tree,
        root_pct,
        Size {
            width: AvailableSpace::Definite(400.0),
            height: AvailableSpace::Definite(360.0),
        },
    );

    let root_h = tree.layout(root_pct).unwrap().size.height;
    let c1_y = tree.layout(c1).unwrap().location.y;
    println!("percent: root_h={root_h}, c1.y={c1_y}");
    // If percent doesn't resolve: root_h = content height ~276, c1.y = 0
    // If percent resolves to 360: root_h = 360, c1.y = (360-276)/2 = 42
    assert_eq!(root_h, 360.0, "root height should fill available space");
}
