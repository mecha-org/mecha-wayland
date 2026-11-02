//! `Button`: a pressable, focusable container — the native interactive
//! primitive. Embeds state transitions for hover, press, focus, and disabled.

use crate::state::WidgetState;
use app::{Build, Context, Handle, Spawner, Widget};
use geometry::{Color, Corners, Insets};
use interactivity::prelude::{Enter, Exit, Press, PressCancelled, Release};
use layout::{LayoutStyle, Val};
use paint::{Paint, PaintContext, Quad};

/// Visual overrides applied when a specific [`WidgetState`] is active.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ButtonStateStyle {
    pub background: Option<Color>,
    pub border_widths: Option<Insets<f32>>,
    pub border_color: Option<Color>,
}

impl ButtonStateStyle {
    #[inline]
    pub fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    #[inline]
    pub fn border(mut self, widths: Insets<f32>, color: Color) -> Self {
        self.border_widths = Some(widths);
        self.border_color = Some(color);
        self
    }

    #[inline]
    pub fn border_widths(mut self, widths: Insets<f32>) -> Self {
        self.border_widths = Some(widths);
        self
    }

    #[inline]
    pub fn border_color(mut self, color: Color) -> Self {
        self.border_color = Some(color);
        self
    }
}

/// The configurable properties of a native button.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ButtonProps {
    pub background: Color,
    pub radius: Corners<f32>,
    pub border_widths: Insets<f32>,
    pub border_color: Color,
    pub hover: ButtonStateStyle,
    pub pressed: ButtonStateStyle,
    pub focused: ButtonStateStyle,
    pub disabled: ButtonStateStyle,
    pub state: WidgetState,
}

impl Default for ButtonProps {
    fn default() -> Self {
        Self {
            background: Color::TRANSPARENT,
            radius: Corners::all(0.0),
            border_widths: Insets::all(0.0),
            border_color: Color::TRANSPARENT,
            hover: ButtonStateStyle::default(),
            pressed: ButtonStateStyle::default(),
            focused: ButtonStateStyle::default(),
            disabled: ButtonStateStyle::default(),
            state: WidgetState::Enabled,
        }
    }
}

impl ButtonProps {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    fn active_state_style(&self) -> ButtonStateStyle {
        match self.state {
            WidgetState::Enabled => ButtonStateStyle::default(),
            WidgetState::Hovered => self.hover,
            WidgetState::Pressed => self.pressed,
            WidgetState::Focused => self.focused,
            WidgetState::Disabled => self.disabled,
        }
    }

    pub fn active_quad(&self) -> Quad {
        let s = self.active_state_style();
        Quad {
            color: s.background.unwrap_or(self.background),
            radii: self.radius,
            border: s.border_widths.unwrap_or(self.border_widths),
            border_color: s.border_color.unwrap_or(self.border_color),
            is_opaque: true,
        }
    }

    #[inline]
    pub const fn is_disabled(&self) -> bool {
        self.state.is_disabled()
    }
}

pub struct Button {
    pub props: ButtonProps,
}

impl std::ops::Deref for Button {
    type Target = ButtonProps;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.props
    }
}

pub fn button() -> ButtonBuilder {
    ButtonBuilder::new()
}

pub struct ButtonBuilder {
    pub props: ButtonProps,
    pub style: LayoutStyle,
}

impl std::ops::Deref for ButtonBuilder {
    type Target = ButtonProps;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.props
    }
}

impl From<ButtonProps> for ButtonBuilder {
    fn from(props: ButtonProps) -> Self {
        Self {
            props,
            style: LayoutStyle::default().row().center(),
        }
    }
}

impl ButtonBuilder {
    pub fn new() -> Self {
        Self {
            props: ButtonProps::default(),
            style: LayoutStyle::default().row().center(),
        }
    }

    pub fn style(mut self, style: LayoutStyle) -> Self {
        self.style = style;
        self
    }

    pub fn width(mut self, width: Val) -> Self {
        self.style = self.style.width(width);
        self
    }

    pub fn height(mut self, height: Val) -> Self {
        self.style = self.style.height(height);
        self
    }

    pub fn background(mut self, color: Color) -> Self {
        self.props.background = color;
        self
    }

    pub fn radius(mut self, radii: Corners<f32>) -> Self {
        self.props.radius = radii;
        self
    }

    pub fn border(mut self, widths: Insets<f32>, color: Color) -> Self {
        self.props.border_widths = widths;
        self.props.border_color = color;
        self
    }

    pub fn border_widths(mut self, widths: Insets<f32>) -> Self {
        self.props.border_widths = widths;
        self
    }

    pub fn border_color(mut self, color: Color) -> Self {
        self.props.border_color = color;
        self
    }

    pub fn hover_style(mut self, style: ButtonStateStyle) -> Self {
        self.props.hover = style;
        self
    }

    pub fn pressed_style(mut self, style: ButtonStateStyle) -> Self {
        self.props.pressed = style;
        self
    }

    pub fn focused_style(mut self, style: ButtonStateStyle) -> Self {
        self.props.focused = style;
        self
    }

    pub fn disabled_style(mut self, style: ButtonStateStyle) -> Self {
        self.props.disabled = style;
        self
    }

    pub fn hover_background(mut self, color: Color) -> Self {
        self.props.hover.background = Some(color);
        self
    }

    pub fn hover_border(mut self, widths: Insets<f32>, color: Color) -> Self {
        self.props.hover.border_widths = Some(widths);
        self.props.hover.border_color = Some(color);
        self
    }

    pub fn pressed_background(mut self, color: Color) -> Self {
        self.props.pressed.background = Some(color);
        self
    }

    pub fn pressed_border(mut self, widths: Insets<f32>, color: Color) -> Self {
        self.props.pressed.border_widths = Some(widths);
        self.props.pressed.border_color = Some(color);
        self
    }

    pub fn focused_background(mut self, color: Color) -> Self {
        self.props.focused.background = Some(color);
        self
    }

    pub fn focused_border(mut self, widths: Insets<f32>, color: Color) -> Self {
        self.props.focused.border_widths = Some(widths);
        self.props.focused.border_color = Some(color);
        self
    }

    pub fn disabled_background(mut self, color: Color) -> Self {
        self.props.disabled.background = Some(color);
        self
    }

    pub fn disabled_border(mut self, widths: Insets<f32>, color: Color) -> Self {
        self.props.disabled.border_widths = Some(widths);
        self.props.disabled.border_color = Some(color);
        self
    }

    pub fn state(mut self, state: WidgetState) -> Self {
        self.props.state = state;
        self
    }

    pub fn disabled(self, disabled: bool) -> Self {
        self.state(if disabled {
            WidgetState::Disabled
        } else {
            WidgetState::Enabled
        })
    }

    pub fn focused(self, focused: bool) -> Self {
        self.state(if focused {
            WidgetState::Focused
        } else {
            WidgetState::Enabled
        })
    }
}

impl Default for ButtonBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Build for ButtonBuilder {
    type Widget = Button;
}

impl Widget for Button {
    type Builder = ButtonBuilder;

    fn build(b: ButtonBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        *s.component_mut::<LayoutStyle>(me).unwrap() = b.style;
        *s.component_mut::<Paint>(me).unwrap() = Paint::Quad(b.props.active_quad());

        s.on::<Enter>(me, |ctx: &mut Context<'_, Button>, _| {
            if ctx.me().state == WidgetState::Enabled {
                ctx.set_state(WidgetState::Hovered);
            }
        });
        s.on::<Exit>(me, |ctx: &mut Context<'_, Button>, _| {
            if matches!(ctx.me().state, WidgetState::Hovered | WidgetState::Pressed) {
                ctx.set_state(WidgetState::Enabled);
            }
        });
        s.on::<Press>(me, |ctx: &mut Context<'_, Button>, _| {
            if matches!(
                ctx.me().state,
                WidgetState::Enabled | WidgetState::Hovered | WidgetState::Focused
            ) {
                ctx.set_state(WidgetState::Pressed);
            }
        });
        s.on::<Release>(me, |ctx: &mut Context<'_, Button>, _| {
            if ctx.me().state == WidgetState::Pressed {
                ctx.set_state(WidgetState::Hovered);
            }
        });
        s.on::<PressCancelled>(me, |ctx: &mut Context<'_, Button>, _| {
            if ctx.me().state == WidgetState::Pressed {
                ctx.set_state(WidgetState::Enabled);
            }
        });

        Button { props: b.props }
    }
}

/// Post-spawn mutations for a [`Button`].
pub trait ButtonContext {
    fn set_background(&mut self, color: Color);
    fn set_radius(&mut self, radii: Corners<f32>);
    fn set_border(&mut self, widths: Insets<f32>, color: Color);
    fn set_border_widths(&mut self, widths: Insets<f32>);
    fn set_border_color(&mut self, color: Color);

    fn set_hover_style(&mut self, style: ButtonStateStyle);
    fn set_pressed_style(&mut self, style: ButtonStateStyle);
    fn set_focused_style(&mut self, style: ButtonStateStyle);
    fn set_disabled_style(&mut self, style: ButtonStateStyle);

    fn set_state(&mut self, state: WidgetState);
    fn set_disabled(&mut self, disabled: bool);
    fn set_focused(&mut self, focused: bool);
}

impl ButtonContext for Context<'_, Button> {
    fn set_background(&mut self, color: Color) {
        if self.me().props.background == color {
            return;
        }
        self.me().props.background = color;
        let active = self.me().active_quad();
        self.set_paint(Paint::Quad(active));
    }

    fn set_radius(&mut self, radii: Corners<f32>) {
        if self.me().props.radius == radii {
            return;
        }
        self.me().props.radius = radii;
        let active = self.me().active_quad();
        self.set_paint(Paint::Quad(active));
    }

    fn set_border(&mut self, widths: Insets<f32>, color: Color) {
        if self.me().props.border_widths == widths && self.me().props.border_color == color {
            return;
        }
        let me = self.me();
        me.props.border_widths = widths;
        me.props.border_color = color;
        let active = me.active_quad();
        self.set_paint(Paint::Quad(active));
    }

    fn set_border_widths(&mut self, widths: Insets<f32>) {
        if self.me().props.border_widths == widths {
            return;
        }
        self.me().props.border_widths = widths;
        let active = self.me().active_quad();
        self.set_paint(Paint::Quad(active));
    }

    fn set_border_color(&mut self, color: Color) {
        if self.me().props.border_color == color {
            return;
        }
        self.me().props.border_color = color;
        let active = self.me().active_quad();
        self.set_paint(Paint::Quad(active));
    }

    fn set_hover_style(&mut self, style: ButtonStateStyle) {
        if self.me().props.hover == style {
            return;
        }
        self.me().props.hover = style;
        if self.me().state == WidgetState::Hovered {
            let active = self.me().active_quad();
            self.set_paint(Paint::Quad(active));
        }
    }

    fn set_pressed_style(&mut self, style: ButtonStateStyle) {
        if self.me().props.pressed == style {
            return;
        }
        self.me().props.pressed = style;
        if self.me().state == WidgetState::Pressed {
            let active = self.me().active_quad();
            self.set_paint(Paint::Quad(active));
        }
    }

    fn set_focused_style(&mut self, style: ButtonStateStyle) {
        if self.me().props.focused == style {
            return;
        }
        self.me().props.focused = style;
        if self.me().state == WidgetState::Focused {
            let active = self.me().active_quad();
            self.set_paint(Paint::Quad(active));
        }
    }

    fn set_disabled_style(&mut self, style: ButtonStateStyle) {
        if self.me().props.disabled == style {
            return;
        }
        self.me().props.disabled = style;
        if self.me().state == WidgetState::Disabled {
            let active = self.me().active_quad();
            self.set_paint(Paint::Quad(active));
        }
    }

    fn set_state(&mut self, state: WidgetState) {
        if self.me().props.state == state {
            return;
        }
        self.me().props.state = state;
        let active = self.me().active_quad();
        self.set_paint(Paint::Quad(active));
    }

    fn set_disabled(&mut self, disabled: bool) {
        self.set_state(if disabled {
            WidgetState::Disabled
        } else {
            WidgetState::Enabled
        });
    }

    fn set_focused(&mut self, focused: bool) {
        self.set_state(if focused {
            WidgetState::Focused
        } else {
            WidgetState::Enabled
        });
    }
}

#[cfg(test)]
mod tests {
    use geometry::{Color, Corners, Insets};

    use super::*;

    const RED: Color = Color::rgb(1.0, 0.0, 0.0);
    const GREEN: Color = Color::rgb(0.0, 1.0, 0.0);
    const BLUE: Color = Color::rgb(0.0, 0.0, 1.0);

    #[test]
    fn builder_defaults() {
        let b = button();
        assert_eq!(b.background, Color::TRANSPARENT);
        assert_eq!(b.radius, Corners::all(0.0));
        assert_eq!(b.border_widths, Insets::all(0.0));
        assert_eq!(b.border_color, Color::TRANSPARENT);
        assert_eq!(b.hover, ButtonStateStyle::default());
        assert_eq!(b.pressed, ButtonStateStyle::default());
        assert_eq!(b.focused, ButtonStateStyle::default());
        assert_eq!(b.disabled, ButtonStateStyle::default());
        assert_eq!(b.state, WidgetState::Enabled);
        assert!(!b.is_disabled());
    }

    #[test]
    fn state_style_builder() {
        let style = ButtonStateStyle::default()
            .background(RED)
            .border_widths(Insets::all(2.0))
            .border_color(GREEN);
        assert_eq!(style.background, Some(RED));
        assert_eq!(style.border_widths, Some(Insets::all(2.0)));
        assert_eq!(style.border_color, Some(GREEN));

        let compound = ButtonStateStyle::default().border(Insets::all(1.0), BLUE);
        assert_eq!(compound.border_widths, Some(Insets::all(1.0)));
        assert_eq!(compound.border_color, Some(BLUE));
    }

    #[test]
    fn builder_verbs_set_the_right_fields() {
        let b = button()
            .background(RED)
            .radius(Corners::all(8.0))
            .border(Insets::all(1.0), GREEN)
            .hover_background(BLUE)
            .hover_border(Insets::all(2.0), Color::WHITE)
            .pressed_background(RED)
            .pressed_border(Insets::all(3.0), Color::BLACK)
            .focused_background(GREEN)
            .focused_border(Insets::all(4.0), BLUE)
            .disabled_background(Color::BLACK)
            .disabled_border(Insets::all(5.0), RED);

        assert_eq!(b.background, RED);
        assert_eq!(b.radius, Corners::all(8.0));
        assert_eq!(b.border_widths, Insets::all(1.0));
        assert_eq!(b.border_color, GREEN);

        assert_eq!(b.hover.background, Some(BLUE));
        assert_eq!(b.hover.border_widths, Some(Insets::all(2.0)));
        assert_eq!(b.hover.border_color, Some(Color::WHITE));

        assert_eq!(b.pressed.background, Some(RED));
        assert_eq!(b.pressed.border_widths, Some(Insets::all(3.0)));
        assert_eq!(b.pressed.border_color, Some(Color::BLACK));

        assert_eq!(b.focused.background, Some(GREEN));
        assert_eq!(b.focused.border_widths, Some(Insets::all(4.0)));
        assert_eq!(b.focused.border_color, Some(BLUE));

        assert_eq!(b.disabled.background, Some(Color::BLACK));
        assert_eq!(b.disabled.border_widths, Some(Insets::all(5.0)));
        assert_eq!(b.disabled.border_color, Some(RED));
    }

    #[test]
    fn whole_state_style_setters() {
        let hover = ButtonStateStyle::default().background(RED);
        let pressed = ButtonStateStyle::default().background(GREEN);
        let focused = ButtonStateStyle::default().background(BLUE);
        let disabled = ButtonStateStyle::default().border_color(Color::WHITE);

        let b = button()
            .hover_style(hover)
            .pressed_style(pressed)
            .focused_style(focused)
            .disabled_style(disabled);

        assert_eq!(b.hover, hover);
        assert_eq!(b.pressed, pressed);
        assert_eq!(b.focused, focused);
        assert_eq!(b.disabled, disabled);
    }

    #[test]
    fn disabled_and_state_verbs() {
        let b = button().disabled(true);
        assert_eq!(b.state, WidgetState::Disabled);
        assert!(b.is_disabled());

        let b = button().disabled(false);
        assert_eq!(b.state, WidgetState::Enabled);
        assert!(!b.is_disabled());

        let b = button().state(WidgetState::Focused);
        assert_eq!(b.state, WidgetState::Focused);
        assert!(!b.is_disabled());
    }

    #[test]
    fn style_verb_replaces_layout_style() {
        let style = LayoutStyle::default().column();
        let b = button().style(style.clone());
        assert_eq!(b.style, style);
    }

    #[test]
    fn active_quad_resolves_base_and_overrides() {
        let base = button()
            .background(Color::BLACK)
            .radius(Corners::all(4.0))
            .border(Insets::all(1.0), Color::WHITE)
            .hover_background(RED)
            .pressed_border(Insets::all(2.0), GREEN)
            .focused_style(
                ButtonStateStyle::default()
                    .background(BLUE)
                    .border(Insets::all(3.0), Color::BLACK),
            )
            .disabled_background(Color::TRANSPARENT);

        // Enabled uses base styles
        let q = base.props.active_quad();
        assert_eq!(q.color, Color::BLACK);
        assert_eq!(q.radii, Corners::all(4.0));
        assert_eq!(q.border, Insets::all(1.0));
        assert_eq!(q.border_color, Color::WHITE);

        // Hovered overrides background, falls back to base border & radius
        let mut props = base.props;
        props.state = WidgetState::Hovered;
        let q = props.active_quad();
        assert_eq!(q.color, RED);
        assert_eq!(q.radii, Corners::all(4.0));
        assert_eq!(q.border, Insets::all(1.0));
        assert_eq!(q.border_color, Color::WHITE);

        // Pressed overrides border, falls back to base background & radius
        let mut props = base.props;
        props.state = WidgetState::Pressed;
        let q = props.active_quad();
        assert_eq!(q.color, Color::BLACK);
        assert_eq!(q.radii, Corners::all(4.0));
        assert_eq!(q.border, Insets::all(2.0));
        assert_eq!(q.border_color, GREEN);

        // Focused overrides both background and border
        let mut props = base.props;
        props.state = WidgetState::Focused;
        let q = props.active_quad();
        assert_eq!(q.color, BLUE);
        assert_eq!(q.radii, Corners::all(4.0));
        assert_eq!(q.border, Insets::all(3.0));
        assert_eq!(q.border_color, Color::BLACK);

        // Disabled overrides background, falls back to base border
        let mut props = base.props;
        props.state = WidgetState::Disabled;
        let q = props.active_quad();
        assert_eq!(q.color, Color::TRANSPARENT);
        assert_eq!(q.radii, Corners::all(4.0));
        assert_eq!(q.border, Insets::all(1.0));
        assert_eq!(q.border_color, Color::WHITE);
    }

    #[test]
    fn button_builder_width_and_height() {
        use layout::px;
        let b = button().width(px(120.0)).height(px(48.0));
        assert_eq!(b.style.width, px(120.0));
        assert_eq!(b.style.height, px(48.0));
    }

    #[test]
    fn button_builder_focused_and_borders() {
        let red = Color::rgba(1.0, 0.0, 0.0, 1.0);
        let b = button()
            .focused(true)
            .border_widths(Insets::all(2.5))
            .border_color(red);
        assert_eq!(b.props.state, WidgetState::Focused);
        assert_eq!(b.props.border_widths, Insets::all(2.5));
        assert_eq!(b.props.border_color, red);
    }

    #[test]
    fn press_cancelled_reverts_a_pressed_button() {
        use app::App;
        use interactivity::prelude::{ContactId, PressCancelled};

        let mut app = App::new();
        app.add_module(layout::LayoutModule)
            .add_module(paint::PaintModule);
        app.insert_resource(atlas::Atlas::new());
        let btn: Handle<Button> = app.spawn(app.root(), button());
        app.emit(
            Press {
                contact: ContactId::Mouse,
                position: geometry::Point::ZERO,
                time: 0,
            },
            btn.id(),
        );
        app.flush();
        assert_eq!(
            app.widget::<Button>(btn).unwrap().state,
            WidgetState::Pressed
        );

        app.emit(
            PressCancelled {
                contact: ContactId::Mouse,
                position: geometry::Point::ZERO,
            },
            btn.id(),
        );
        app.flush();
        assert_eq!(
            app.widget::<Button>(btn).unwrap().state,
            WidgetState::Enabled
        );
    }
}
