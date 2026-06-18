use assets::{BakedFont, GlyphInfo};
use taffy::prelude::*;
use taffy::{AvailableSpace, Size, Style};
use ui::widgets::Text;
use ui::{self, Measure, Render, RenderCommand, Widget, WidgetTree, compute_layout};
use utils::Color;

static TEST_FONT: BakedFont = BakedFont {
    size: 16.0,
    line_height: 20.0,
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

#[test]
fn text_build_tree_registers_node() {
    let mut tree = WidgetTree::new();
    let mut text = Text::placeholder();
    let id = text.build_tree(&mut tree);
    assert_eq!(text.node_id(), id);
    assert!(tree.layout(id).is_ok());
}

#[test]
fn text_layout_produces_correct_size() {
    let mut tree = WidgetTree::new();
    let style = Style {
        size: Size {
            width: length(100.0_f32),
            height: length(50.0_f32),
        },
        ..Style::default()
    };
    let mut text = Text::new(style);
    let id = text.build_tree(&mut tree);
    compute_layout(&mut tree, id, Size::MAX_CONTENT);
    let layout = tree.layout(id).unwrap();
    assert_eq!(layout.size.width, 100.0);
    assert_eq!(layout.size.height, 50.0);
}

#[test]
fn text_placeholder_measure_returns_zero() {
    let text = Text::placeholder();
    let result = text.measure(
        Size {
            width: None,
            height: None,
        },
        Size {
            width: AvailableSpace::MaxContent,
            height: AvailableSpace::MaxContent,
        },
    );
    assert_eq!(result, Size::ZERO);
}

#[test]
fn text_render_emits_draw_text_with_correct_fields() {
    let mut text = Text::new(Style::default());
    text.font = Some(&TEST_FONT);
    text.text = "hi".to_string();
    text.color = Color::WHITE;

    let mut tree = WidgetTree::new();
    let id = text.build_tree(&mut tree);
    compute_layout(&mut tree, id, Size::MAX_CONTENT);
    let layout = tree.layout(id).unwrap();
    let abs_pos = ui::Point::new(layout.location.x, layout.location.y);
    let commands = text.render(layout, abs_pos);

    assert_eq!(commands.len(), 1);
    match &commands[0] {
        RenderCommand::DrawText {
            font,
            text: t,
            origin,
            color,
            ..
        } => {
            assert!(std::ptr::eq(*font as *const _, &TEST_FONT as *const _));
            assert_eq!(t, "hi");
            assert_eq!(origin.x(), abs_pos.x());
            assert_eq!(origin.y(), abs_pos.y());
            assert_eq!(*color, Color::WHITE);
        }
        _ => panic!("expected DrawText"),
    }
}

#[test]
fn text_measure_uses_font_metrics() {
    let mut text = Text::new(Style::default());
    text.font = Some(&TEST_FONT);
    text.text = "ab".to_string(); // 2 glyphs × advance 10.0 = 20.0 wide

    let result = text.measure(
        Size {
            width: None,
            height: None,
        },
        Size {
            width: AvailableSpace::MaxContent,
            height: AvailableSpace::MaxContent,
        },
    );
    assert_eq!(result.width, 20.0);
    assert_eq!(result.height, TEST_FONT.line_height);
}

#[test]
fn text_set_text_triggers_layout_recompute() {
    let mut text = Text::new(Style::default());
    text.font = Some(&TEST_FONT);
    text.text = "ab".to_string(); // 2 chars × advance 10.0 = 20.0

    let mut tree = WidgetTree::new();
    let id = text.build_tree(&mut tree);
    compute_layout(&mut tree, id, Size::MAX_CONTENT);
    assert_eq!(tree.layout(id).unwrap().size.width, 20.0);

    // Changing text should invalidate layout
    text.set_text(&mut tree, "abcde".to_string()); // 5 chars × 10.0 = 50.0
    compute_layout(&mut tree, id, Size::MAX_CONTENT);
    assert_eq!(tree.layout(id).unwrap().size.width, 50.0);
}
