use taffy::Style;
use ui::{Point, Render, RenderCommand};
use utils::{Color, Rect, Size};

#[ui::widget]
pub struct UnlockCircle {
    /// Current drag offset in pixels (negative = dragged upward).
    pub drag_offset_y: f32,
    /// Whether a drag is actively in progress.
    pub is_dragging: bool,
}

impl UnlockCircle {
    pub const RADIUS: f32 = 36.0;
    pub const DIAMETER: f32 = Self::RADIUS * 2.0;

    pub fn new(style: Style) -> Self {
        Self {
            node_id: taffy::NodeId::new(u64::MAX),
            style,
            drag_offset_y: 0.0,
            is_dragging: false,
        }
    }

    /// Apply a new drag position (offset must be ≤ 0 — upward only).
    pub fn set_drag(&mut self, offset_y: f32, dragging: bool) {
        self.drag_offset_y = offset_y.min(0.0);
        self.is_dragging = dragging;
    }

    /// Reset to resting position.
    pub fn reset_drag(&mut self) {
        self.drag_offset_y = 0.0;
        self.is_dragging = false;
    }
}

impl Render for UnlockCircle {
    fn render(&self, layout: &taffy::Layout, abs_pos: Point) -> Vec<RenderCommand> {
        let visual_y = abs_pos.y() + self.drag_offset_y;
        let size = Size::new(layout.size.width, layout.size.height);

        vec![
            RenderCommand::RegisterHitArea {
                id: self.node_id.into(),
                rect: Rect::new(
                    abs_pos.x(),
                    abs_pos.y(),
                    layout.size.width,
                    layout.size.height,
                ),
            },
            RenderCommand::DrawQuad {
                color: Color::from_rgb8(255, 255, 255),
                border_color: Color::from_rgb8(89, 89, 89),
                origin: Point::new(abs_pos.x(), visual_y),
                z: 0.2,
                size,
                border_radius: Self::RADIUS,
                border_thickness: 12.0,
            },
        ]
    }
}
