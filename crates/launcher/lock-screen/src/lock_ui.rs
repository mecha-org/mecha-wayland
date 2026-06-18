use taffy::prelude::*;
use ui::widgets::{Div, Text};
use ui::{RenderCommand, Widget, WidgetTree, compute_layout};
use utils::Color;

use crate::atlas;
use crate::surface::Surface;
use crate::widgets::clock::ClockText;
use crate::widgets::unlock_circle::UnlockCircle;

type PanelRoot = Div<(ClockText, UnlockCircle, Text)>;

pub struct LockUi {
    pub lock_surface_id: u32,
    pub surface: Surface,
    tree: WidgetTree,
    root: PanelRoot,
}

impl LockUi {
    pub fn new(wl_surface_id: u32, lock_surface_id: u32) -> Self {
        // Clock row
        let mut clock_text = ClockText::new(Style::default(), atlas::UI.id);
        clock_text.inner.font = Some(&atlas::UI_FONT_MONO_100);

        // Circle row
        let circle = UnlockCircle::new(Style {
            display: Display::Flex,
            size: Size {
                width: length(UnlockCircle::DIAMETER),
                height: length(UnlockCircle::DIAMETER),
            },
            margin: Rect {
                top: auto(),
                right: zero(),
                bottom: length(20.0_f32),
                left: zero(),
            },
            ..Default::default()
        });

        // Hint row
        let mut hint = Text::new(Style {
            margin: Rect {
                top: zero(),
                right: zero(),
                bottom: length(40.0_f32),
                left: zero(),
            },
            ..Default::default()
        });
        hint.text = "Swipe up to unlock".to_string();
        hint.atlas_id = Some(atlas::UI.id);
        hint.color = Color::rgb(0.6, 0.6, 0.6);
        hint.z = 0.5;
        hint.font = Some(&atlas::UI_FONT_MONO_16);

        // Root column
        let mut root = Div::new(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: Some(AlignItems::Center),
                size: Size {
                    width: percent(1.0_f32),
                    height: percent(1.0_f32),
                },
                padding: Rect {
                    top: length(110.0_f32),
                    right: zero(),
                    bottom: zero(),
                    left: zero(),
                },
                ..Default::default()
            },
            (clock_text, circle, hint),
        );
        let mut tree = WidgetTree::new();
        root.build_tree(&mut tree);

        Self {
            lock_surface_id,
            surface: Surface::new(wl_surface_id),
            tree,
            root,
        }
    }

    /// Update the circle's drag offset (call on every `TouchEvent::Drag` or pointer move).
    pub fn set_drag(&mut self, offset_y: f32, dragging: bool) {
        self.root.children.1.set_drag(offset_y, dragging);
    }

    /// Snap the circle back to its resting position.
    pub fn reset_drag(&mut self) {
        self.root.children.1.reset_drag();
    }

    /// Returns the widget node id of the unlock circle.
    pub fn circle_node_id(&self) -> u64 {
        self.root.children.1.node_id().into()
    }

    /// Update the displayed clock text.
    ///
    /// Returns `true` if the string changed and a redraw is required.
    pub fn update_clock(&mut self, h: u32, m: u32) -> bool {
        let (root, tree) = (&mut self.root, &mut self.tree);
        root.children.0.update(tree, h, m)
    }

    /// Re-run Taffy layout using the current surface dimensions.
    pub fn recompute_layout(&mut self) {
        let (w, h) = self.surface.size;
        compute_layout(
            &mut self.tree,
            self.root.node_id(),
            taffy::Size {
                width: taffy::AvailableSpace::Definite(w as f32),
                height: taffy::AvailableSpace::Definite(h as f32),
            },
        );
    }

    /// Collect render commands for the current frame.
    pub fn render_commands(&self) -> Vec<RenderCommand> {
        let layout = self.tree.layout(self.root.node_id()).unwrap();
        self.root
            .render_node(layout, &self.tree, ui::Point::new(0.0, 0.0))
    }
}
