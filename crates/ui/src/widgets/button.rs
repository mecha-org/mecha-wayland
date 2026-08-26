use taffy::prelude::*;
use taffy::{Layout, Style};
use utils::{Color, Point, Rect, Size as USize};

use crate::{Damage, OnChange, Render, RenderCommand, WidgetList};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BackgroundColor(pub Color);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Padding {
    pub left: LengthPercentage,
    pub right: LengthPercentage,
    pub top: LengthPercentage,
    pub bottom: LengthPercentage,
}

impl Padding {
    pub fn zero() -> Self {
        Self {
            left: zero(),
            right: zero(),
            top: zero(),
            bottom: zero(),
        }
    }

    pub fn all(val: f32) -> Self {
        Self {
            left: length(val),
            right: length(val),
            top: length(val),
            bottom: length(val),
        }
    }

    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            left: length(horizontal),
            right: length(horizontal),
            top: length(vertical),
            bottom: length(vertical),
        }
    }

    pub fn only(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left: length(left),
            right: length(right),
            top: length(top),
            bottom: length(bottom),
        }
    }
}

impl From<Padding> for taffy::Rect<LengthPercentage> {
    fn from(p: Padding) -> Self {
        taffy::Rect {
            left: p.left,
            right: p.right,
            top: p.top,
            bottom: p.bottom,
        }
    }
}

#[crate::widget]
pub struct Button<T: WidgetList> {
    pub background: BackgroundColor,
    pub border_color: Color,
    pub border_radius: f32,
    pub border_thickness: f32,
    pub disabled: bool,
    #[widget(child)]
    pub children: T,
}

impl<T: WidgetList> Button<T> {
    /// Creates a new Button.
    pub fn new(children: T) -> Self {
        let style = Style {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            justify_content: Some(JustifyContent::Center),
            align_items: Some(AlignItems::Center),
            size: taffy::Size {
                width: auto(),
                height: auto(),
            },
            padding: Padding::symmetric(24.0, 10.0).into(),
            gap: taffy::Size {
                width: length(8.0_f32),
                height: zero(),
            },
            ..Style::default()
        };

        Self {
            node_id: taffy::NodeId::new(u64::MAX),
            style,
            bounds: Rect::ZERO,
            pending_damage: Damage::None,
            is_opaque: false,
            background: BackgroundColor(Color::TRANSPARENT),
            border_color: Color::TRANSPARENT,
            border_radius: 4.0,
            border_thickness: 0.0,
            disabled: false,
            children,
        }
    }

    /// Filled Button (solid container background)
    pub fn filled(bg_color: Color, children: T) -> Self {
        Self::new(children).background(bg_color).border_radius(0.0)
    }

    /// Outlined Button (transparent background, stroke border)
    pub fn outlined(border_color: Color, children: T) -> Self {
        Self::new(children)
            .border_color(border_color)
            .border_thickness(1.5)
            .border_radius(0.0)
    }

    /// Sets the height of the button.
    pub fn height(mut self, height: f32) -> Self {
        self.style.size.height = length(height);
        self
    }

    /// Sets the width of the button.
    pub fn width(mut self, width: f32) -> Self {
        self.style.size.width = length(width);
        self
    }

    /// Sets the padding of the button.
    pub fn padding(mut self, padding: impl Into<taffy::Rect<LengthPercentage>>) -> Self {
        self.style.padding = padding.into();
        self
    }

    /// Sets the background color of the button.
    pub fn background(mut self, background: Color) -> Self {
        self.background = BackgroundColor(background);
        self
    }

    /// Sets the border color of the button.
    pub fn border_color(mut self, border_color: Color) -> Self {
        self.border_color = border_color;
        self
    }

    /// Sets the border radius of the button.
    pub fn border_radius(mut self, border_radius: f32) -> Self {
        self.border_radius = border_radius;
        self
    }

    /// Sets the border thickness of the button.
    pub fn border_thickness(mut self, border_thickness: f32) -> Self {
        self.border_thickness = border_thickness;
        self
    }

    /// Sets whether the button is disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

// OnChange impls
impl<T: WidgetList> OnChange<BackgroundColor> for Button<T> {
    fn damage(&self, _new: &BackgroundColor) -> Damage {
        Damage::paint(self.bounds)
    }
    fn change(&mut self, new: BackgroundColor) {
        self.background = new;
    }
}

impl<T: WidgetList> OnChange<Color> for Button<T> {
    fn damage(&self, _new: &Color) -> Damage {
        Damage::paint(self.bounds)
    }
    fn change(&mut self, new: Color) {
        self.border_color = new;
    }
}

/// Newtype for toggling the disabled state via `set(Disabled(bool))`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Disabled(pub bool);

impl<T: WidgetList> OnChange<Disabled> for Button<T> {
    fn damage(&self, _new: &Disabled) -> Damage {
        // Disabled state change may alter visual appearance but not layout.
        Damage::paint(self.bounds)
    }
    fn change(&mut self, new: Disabled) {
        self.disabled = new.0;
    }
}

// Render

impl<T: WidgetList> Render for Button<T> {
    fn render(&self, layout: &Layout, abs_pos: Point) -> Vec<RenderCommand> {
        let (color, border_color, border_thickness) = if self.disabled {
            (
                Color::from_rgb8(55, 58, 66),
                Color::from_rgb8(75, 80, 90),
                if self.border_thickness > 0.0 {
                    self.border_thickness
                } else {
                    1.0
                },
            )
        } else {
            (self.background.0, self.border_color, self.border_thickness)
        };

        // `z`, `background`, and `is_opaque` are stamped by the render walk.
        vec![RenderCommand::DrawQuad {
            color,
            border_color,
            origin: abs_pos,
            z: 0.0,
            size: USize::new(layout.size.width, layout.size.height),
            border_radius: self.border_radius,
            border_thickness,
            background: Color::TRANSPARENT,
            is_opaque: self.is_opaque,
        }]
    }

    fn fill(&self) -> Color {
        if self.disabled {
            Color::from_rgb8(55, 58, 66)
        } else {
            self.background.0
        }
    }
}
