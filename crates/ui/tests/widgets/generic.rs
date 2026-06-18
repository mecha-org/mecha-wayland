use taffy::prelude::*;
use taffy::{Layout, Size, Style};
use ui::widgets::{Div, Text};
use ui::{self, Render, RenderCommand, Widget, WidgetList, WidgetTree, compute_layout, widget};

/// A generic container widget built with the macro — exercises the generic
/// widget + #[widget(child)] path end-to-end.
#[widget]
struct Card<T: WidgetList> {
    #[widget(child)]
    children: T,
}

impl<T: WidgetList> Card<T> {
    fn new(children: T) -> Self {
        Self {
            node_id: taffy::NodeId::new(u64::MAX),
            style: Style::default(),
            children,
        }
    }
}

impl<T: WidgetList> Render for Card<T> {
    fn render(&self, _layout: &Layout, _abs_pos: ui::Point) -> Vec<RenderCommand> {
        vec![]
    }
}

#[test]
fn generic_widget_build_tree_wires_children() {
    let mut tree = WidgetTree::new();
    let mut card = Card::new((Text::placeholder(), Text::placeholder()));
    let card_id = card.build_tree(&mut tree);

    assert_eq!(card.node_id(), card_id);
    let children = tree.children(card_id).unwrap();
    assert_eq!(children.len(), 2);
}

#[test]
fn generic_widget_render_node_collects_child_commands() {
    let mut tree = WidgetTree::new();
    let mut card = Card::new((Text::placeholder(),));
    let card_id = card.build_tree(&mut tree);
    compute_layout(&mut tree, card_id, Size::MAX_CONTENT);
    let layout = tree.layout(card_id).unwrap();

    // Card itself emits nothing; child Text with no font also emits nothing —
    // but the call must not panic and must return a vec.
    let commands = card.render_node(layout, &tree, ui::Point::new(0.0, 0.0));
    assert!(commands.is_empty());
}

/// Verifies a widget whose child field is a concrete composed type, not bare T.
#[widget]
struct Card2<T: Widget> {
    #[widget(child)]
    children: Div<(Text, T)>,
}

impl<T: Widget> Card2<T> {
    fn new(inner: T) -> Self {
        Self {
            node_id: taffy::NodeId::new(u64::MAX),
            style: Style::default(),
            children: Div::new(Style::default(), (Text::placeholder(), inner)),
        }
    }
}

impl<T: Widget> Render for Card2<T> {
    fn render(&self, _layout: &Layout, _abs_pos: ui::Point) -> Vec<RenderCommand> {
        vec![]
    }
}

#[test]
fn composed_child_type_build_tree_wires_correctly() {
    let mut tree = WidgetTree::new();
    let mut card = Card2::new(Text::placeholder());
    let id = card.build_tree(&mut tree);

    // Card2 has one child (the Div); the Div has two children (Text, Text).
    let card_children = tree.children(id).unwrap();
    assert_eq!(card_children.len(), 1);
    let div_children = tree.children(card_children[0]).unwrap();
    assert_eq!(div_children.len(), 2);
}
