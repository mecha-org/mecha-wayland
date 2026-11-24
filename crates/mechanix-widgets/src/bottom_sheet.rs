use crate::color::{ColorSource, IntoColorSource};
use crate::text::{Text, TextContextExt, text};
use app::{Build, Context, Event, Handle, Spawner, Widget};
use geometry::{Insets, Rect};
use interactivity::{Clicked, ContactId, ContextClaim, Press, PressCancelled, PressMoved, Release};
use layout::{Align, ComputedLayout, Justify, LayoutStyle, StyleContext, Val, auto, percent, px};
use paint::{Paint, Quad};
use scroll::VelocityTracker;
use theme::{ColorRole, Spacing, SpawnerThemeExt, TextVariant, ThemeReader};
use widgets::{Div, DivContext, TextOverflow, div};

/// The default look and gesture tuning of a bottom sheet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BottomSheetStyle {
    pub bg: ColorRole,
    pub handle_color: ColorRole,
    pub title_color: ColorRole,
    pub title_variant: TextVariant,

    pub handle_width: f32,
    pub handle_height: f32,
    pub grip_padding_top: f32,
    pub grip_padding_bottom: f32,
    pub header_min_height: f32,

    pub scrim: ColorRole,
    pub scrim_opacity: f32,

    /// How far down a press must move before it drags the sheet.
    pub drag_slop: f32,
    /// Share of its height a sheet must be dragged down to close.
    pub close_fraction: f32,
    /// Downward release speed (px/s) past which the sheet closes.
    pub fling_velocity: f32,
}

impl BottomSheetStyle {
    pub const fn standard() -> Self {
        Self {
            bg: ColorRole::SurfaceContainerLow,
            handle_color: ColorRole::Outline,
            title_color: ColorRole::OnSurface,
            title_variant: TextVariant::TitleLarge,

            handle_width: 32.0,
            handle_height: 4.0,
            grip_padding_top: Spacing::Space100.dp(),
            grip_padding_bottom: Spacing::Space50.dp(),
            header_min_height: 56.0,

            scrim: ColorRole::Scrim,
            scrim_opacity: 0.32,

            drag_slop: 10.0,
            close_fraction: 0.5,
            fling_velocity: 700.0,
        }
    }

    /// The handle with its space above and below.
    pub const fn grip_height(&self) -> f32 {
        self.grip_padding_top + self.handle_height + self.grip_padding_bottom
    }
}

impl Default for BottomSheetStyle {
    fn default() -> Self {
        Self::standard()
    }
}

/// The user closed the sheet. Not emitted for `close` from code.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SheetClosed {}

impl Event for SheetClosed {}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BottomSheetOverrides {
    pub background: Option<ColorSource>,
    pub handle_color: Option<ColorSource>,
    pub title_color: Option<ColorSource>,
}

impl BottomSheetOverrides {
    fn background(&self, style: &BottomSheetStyle) -> ColorSource {
        self.background.unwrap_or(ColorSource::Role(style.bg))
    }

    fn handle_color(&self, style: &BottomSheetStyle) -> ColorSource {
        self.handle_color
            .unwrap_or(ColorSource::Role(style.handle_color))
    }

    fn title_color(&self, style: &BottomSheetStyle) -> ColorSource {
        self.title_color
            .unwrap_or(ColorSource::Role(style.title_color))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SheetSize {
    pub width: Val,
    pub height: Val,
    pub min_height: Val,
    pub max_height: Val,
}

impl Default for SheetSize {
    fn default() -> Self {
        Self {
            width: percent(100.0),
            height: Val::Auto,
            min_height: Val::Auto,
            max_height: percent(100.0),
        }
    }
}

pub fn bottom_sheet() -> BottomSheetBuilder {
    BottomSheetBuilder::new()
}

#[derive(Debug, Clone, PartialEq)]
pub struct BottomSheetBuilder {
    pub(crate) title: String,
    pub(crate) handle: bool,
    pub(crate) dismissible: bool,
    pub(crate) scrim: bool,
    pub(crate) size: SheetSize,
    pub(crate) style: BottomSheetStyle,
    pub(crate) overrides: BottomSheetOverrides,
}

impl Default for BottomSheetBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl BottomSheetBuilder {
    pub fn new() -> Self {
        Self {
            title: String::new(),
            handle: true,
            dismissible: true,
            scrim: false,
            size: SheetSize::default(),
            style: BottomSheetStyle::standard(),
            overrides: BottomSheetOverrides::default(),
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn handle(mut self, visible: bool) -> Self {
        self.handle = visible;
        self
    }

    /// Whether the user can close the sheet by dragging or tapping the scrim.
    pub fn dismissible(mut self, dismissible: bool) -> Self {
        self.dismissible = dismissible;
        self
    }

    pub fn scrim(mut self, scrim: bool) -> Self {
        self.scrim = scrim;
        self
    }

    pub fn width(mut self, width: Val) -> Self {
        self.size.width = width;
        self
    }

    pub fn height(mut self, height: Val) -> Self {
        self.size.height = height;
        self
    }

    pub fn min_height(mut self, height: Val) -> Self {
        self.size.min_height = height;
        self
    }

    pub fn max_height(mut self, height: Val) -> Self {
        self.size.max_height = height;
        self
    }

    /// Replace the default look and gesture tuning. Per-color overrides
    /// (`background`, `handle_color`, `title_color`) still win over it.
    pub fn style(mut self, style: BottomSheetStyle) -> Self {
        self.style = style;
        self
    }

    pub fn background(mut self, color: impl IntoColorSource) -> Self {
        self.overrides.background = color.into_color_source();
        self
    }

    pub fn handle_color(mut self, color: impl IntoColorSource) -> Self {
        self.overrides.handle_color = color.into_color_source();
        self
    }

    pub fn title_color(mut self, color: impl IntoColorSource) -> Self {
        self.overrides.title_color = color.into_color_source();
        self
    }
}

impl Build for BottomSheetBuilder {
    type Widget = BottomSheet;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Position {
    Rest,
    Dragged(f32),
    /// A whole parent height down, so it is offscreen at any surface height.
    Closed,
}

impl Position {
    fn top(self) -> Val {
        match self {
            Self::Rest => px(0.0),
            Self::Dragged(d) => px(d),
            Self::Closed => percent(100.0),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Drag {
    contact: ContactId,
    origin: f32,
    height: f32,
    /// Past the slop: the sheet follows the finger and holds the contact.
    active: bool,
    tracker: VelocityTracker,
}

/// Themed Mechanix Design bottom sheet.
#[derive(Debug, Clone)]
pub struct BottomSheet {
    pub(crate) size: SheetSize,
    pub(crate) handle_visible: bool,
    pub(crate) dismissible: bool,
    pub(crate) scrim: bool,
    pub(crate) position: Position,
    pub(crate) drag: Option<Drag>,
    pub(crate) style: BottomSheetStyle,
    pub(crate) overrides: BottomSheetOverrides,
    pub(crate) surface: Handle<Div>,
    pub(crate) grip: Handle<Div>,
    pub(crate) handle: Handle<Div>,
    pub(crate) header: Handle<Div>,
    pub(crate) title: Handle<Text>,
    pub(crate) body: Handle<Div>,
    pub(crate) footer: Handle<Div>,
}

impl BottomSheet {
    /// Padded slot for the sheet's main content.
    #[inline]
    pub fn body(&self) -> Handle<Div> {
        self.body
    }

    /// Unpadded slot under the body for edge-to-edge content.
    #[inline]
    pub fn footer(&self) -> Handle<Div> {
        self.footer
    }

    #[inline]
    pub fn surface(&self) -> Handle<Div> {
        self.surface
    }

    #[inline]
    pub fn grip(&self) -> Handle<Div> {
        self.grip
    }

    #[inline]
    pub fn handle(&self) -> Handle<Div> {
        self.handle
    }

    #[inline]
    pub fn header(&self) -> Handle<Div> {
        self.header
    }

    #[inline]
    pub fn title(&self) -> Handle<Text> {
        self.title
    }

    #[inline]
    pub fn has_scrim(&self) -> bool {
        self.scrim
    }

    #[inline]
    pub fn size(&self) -> SheetSize {
        self.size
    }

    #[inline]
    pub fn is_handle_visible(&self) -> bool {
        self.handle_visible
    }

    #[inline]
    pub fn is_dismissible(&self) -> bool {
        self.dismissible
    }

    #[inline]
    pub fn is_dragging(&self) -> bool {
        self.drag.as_ref().is_some_and(|d| d.active)
    }

    #[inline]
    pub fn is_closed(&self) -> bool {
        self.position == Position::Closed
    }

    #[inline]
    pub fn style(&self) -> &BottomSheetStyle {
        &self.style
    }

    #[inline]
    pub fn overrides(&self) -> &BottomSheetOverrides {
        &self.overrides
    }
}

fn sheet_style() -> LayoutStyle {
    LayoutStyle::default()
        .column()
        .size(percent(100.0), percent(100.0))
        .justify(Justify::End)
        .align_items(Align::Center)
}

fn surface_style(size: SheetSize, position: Position) -> LayoutStyle {
    LayoutStyle::default()
        .column()
        .relative()
        .inset(Insets::new(position.top(), auto(), auto(), auto()))
        .width(size.width)
        .height(size.height)
        .min_height(size.min_height)
        .max_height(size.max_height)
        .shrink(0.0)
        .align_items(Align::Stretch)
}

fn grip_style(style: &BottomSheetStyle, visible: bool) -> LayoutStyle {
    let layout = LayoutStyle::default()
        .column()
        .width(percent(100.0))
        .shrink(0.0)
        .align_items(Align::Center)
        .padding(Insets::new(
            px(style.grip_padding_top),
            px(0.0),
            px(style.grip_padding_bottom),
            px(0.0),
        ));
    if visible { layout } else { layout.hidden() }
}

fn handle_style(style: &BottomSheetStyle) -> LayoutStyle {
    LayoutStyle::default()
        .size(px(style.handle_width), px(style.handle_height))
        .shrink(0.0)
}

fn header_style(style: &BottomSheetStyle) -> LayoutStyle {
    LayoutStyle::default()
        .row()
        .align_items(Align::Center)
        .min_height(px(style.header_min_height))
        .column_gap(px(Spacing::Space200.dp()))
        .padding(Insets::new(
            px(Spacing::Space100.dp()),
            px(Spacing::Space300.dp()),
            px(Spacing::Space100.dp()),
            px(Spacing::Space300.dp()),
        ))
}

fn title_style() -> LayoutStyle {
    LayoutStyle::default()
        .grow(1.0)
        .shrink(1.0)
        .min_width(px(0.0))
}

fn body_style() -> LayoutStyle {
    LayoutStyle::default()
        .column()
        .grow(1.0)
        .row_gap(px(Spacing::Space200.dp()))
        .padding(Insets::all(px(Spacing::Space300.dp())))
}

fn footer_style() -> LayoutStyle {
    LayoutStyle::default()
        .column()
        .shrink(0.0)
        .align_items(Align::Stretch)
}

impl Widget for BottomSheet {
    type Builder = BottomSheetBuilder;

    fn build(b: BottomSheetBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        let style = b.style;
        *s.component_mut::<LayoutStyle>(me).unwrap() = sheet_style();
        if b.scrim {
            *s.component_mut::<Paint>(me).unwrap() = Paint::Quad(Quad::new(scrim_color(&style, s)));
        }

        let surface = s.spawn(
            me,
            div()
                .style(surface_style(b.size, Position::Rest))
                .background(b.overrides.background(&style).resolve(s)),
        );
        let grip = s.spawn(surface, div().style(grip_style(&style, b.handle)));
        let handle = s.spawn(
            grip,
            div()
                .style(handle_style(&style))
                .background(b.overrides.handle_color(&style).resolve(s))
                .radius(style.handle_height / 2.0),
        );
        let header = s.spawn(surface, div().style(header_style(&style)));
        let title = s.spawn(
            header,
            text(b.title)
                .variant(style.title_variant)
                .color(b.overrides.title_color(&style))
                .max_lines(1)
                .overflow(TextOverflow::Ellipsis)
                .style(title_style()),
        );
        let body = s.spawn(surface, div().style(body_style()));
        let footer = s.spawn(surface, div().style(footer_style()));

        s.on_theme(me, sync_visuals);
        s.on::<Clicked>(me, on_scrim_click);
        for area in [grip, header] {
            s.on::<Press>(area, on_press);
            s.on::<PressMoved>(area, on_press_moved);
            s.on::<Release>(area, |ctx, e: &Release| {
                end_drag(ctx, e.contact, Some(e.time))
            });
            s.on::<PressCancelled>(area, |ctx, e: &PressCancelled| {
                end_drag(ctx, e.contact, None)
            });
        }

        BottomSheet {
            size: b.size,
            handle_visible: b.handle,
            dismissible: b.dismissible,
            scrim: b.scrim,
            position: Position::Rest,
            drag: None,
            style,
            overrides: b.overrides,
            surface,
            grip,
            handle,
            header,
            title,
            body,
            footer,
        }
    }
}

fn rect_of<W: Widget, V: Widget>(ctx: &mut Context<'_, W>, handle: Handle<V>) -> Rect {
    ctx.at(handle)
        .and_then(|c| c.component::<ComputedLayout>().map(|l| l.rect))
        .unwrap_or(Rect::ZERO)
}

fn on_press(ctx: &mut Context<'_, BottomSheet>, e: &Press) {
    let (dismissible, busy, closed, surface) = {
        let me = ctx.me();
        (
            me.dismissible,
            me.drag.is_some(),
            me.position == Position::Closed,
            me.surface,
        )
    };
    if !dismissible || busy || closed {
        return;
    }
    let height = rect_of(ctx, surface).height();
    let mut tracker = VelocityTracker::default();
    tracker.add(e.time, f64::from(e.position.y));
    ctx.me().drag = Some(Drag {
        contact: e.contact,
        origin: e.position.y,
        height,
        active: false,
        tracker,
    });
}

fn on_press_moved(ctx: &mut Context<'_, BottomSheet>, e: &PressMoved) {
    let y = e.position.y;
    let (claim, origin) = {
        let me = ctx.me();
        let slop = me.style.drag_slop;
        let Some(drag) = me.drag.as_mut().filter(|d| d.contact == e.contact) else {
            return;
        };
        drag.tracker.add(e.time, f64::from(y));
        let mut claim = false;
        if !drag.active {
            if y - drag.origin <= slop {
                return;
            }
            drag.active = true;
            drag.origin = y;
            claim = true;
        }
        (claim, drag.origin)
    };
    if claim {
        ctx.claim_contact(e.contact);
    }
    set_position(ctx, Position::Dragged((y - origin).max(0.0)));
}

/// `time` is `None` for a cancel, which never closes the sheet.
fn end_drag(ctx: &mut Context<'_, BottomSheet>, contact: ContactId, time: Option<u32>) {
    if !ctx.me().drag.as_ref().is_some_and(|d| d.contact == contact) {
        return;
    }
    let drag = ctx.me().drag.take().expect("checked above");
    if !drag.active {
        return;
    }
    let Some(time) = time else {
        set_position(ctx, Position::Rest);
        return;
    };
    let (offset, style) = {
        let me = ctx.me();
        let offset = match me.position {
            Position::Dragged(d) => d,
            _ => 0.0,
        };
        (offset, me.style)
    };
    let velocity = drag.tracker.velocity(time) as f32;
    if should_close(&style, offset, drag.height, velocity) {
        close(ctx);
        ctx.emit(SheetClosed {}, ctx.handle());
    } else {
        set_position(ctx, Position::Rest);
    }
}

fn should_close(style: &BottomSheetStyle, offset: f32, height: f32, velocity: f32) -> bool {
    velocity > style.fling_velocity || offset > height * style.close_fraction
}

fn close(ctx: &mut Context<'_, BottomSheet>) {
    ctx.me().drag = None;
    set_position(ctx, Position::Closed);
}

fn user_close(ctx: &mut Context<'_, BottomSheet>) {
    let me = ctx.me();
    if !me.dismissible || me.position == Position::Closed {
        return;
    }
    close(ctx);
    ctx.emit(SheetClosed {}, ctx.handle());
}

fn on_scrim_click(ctx: &mut Context<'_, BottomSheet>, e: &Clicked) {
    let (scrim, surface) = {
        let me = ctx.me();
        (me.scrim, me.surface)
    };
    if !scrim || rect_of(ctx, surface).contains(e.position) {
        return;
    }
    user_close(ctx);
}

fn scrim_color(style: &BottomSheetStyle, reader: &impl ThemeReader) -> geometry::Color {
    let c = reader.color(style.scrim);
    c.with_alpha(c.a * style.scrim_opacity)
}

fn sync_scrim(ctx: &mut Context<'_, BottomSheet>) {
    let (show, style) = {
        let me = ctx.me();
        (me.scrim && me.position != Position::Closed, me.style)
    };
    let paint = if show {
        Paint::Quad(Quad::new(scrim_color(&style, ctx)))
    } else {
        Paint::None
    };
    if let Some(mut p) = ctx.component_mut::<Paint>() {
        *p = paint;
    }
}

fn set_position(ctx: &mut Context<'_, BottomSheet>, position: Position) {
    ctx.me().position = position;
    apply_size(ctx);
    sync_scrim(ctx);
}

fn apply_size(ctx: &mut Context<'_, BottomSheet>) {
    let (size, position, surface) = {
        let me = ctx.me();
        (me.size, me.position, me.surface)
    };
    if let Some(mut c) = ctx.at(surface) {
        c.set_style(surface_style(size, position));
    }
}

fn sync_visuals(ctx: &mut Context<'_, BottomSheet>) {
    let (style, overrides, surface, handle) = {
        let me = ctx.me();
        (me.style, me.overrides, me.surface, me.handle)
    };
    let background = overrides.background(&style).resolve(ctx);
    let handle_color = overrides.handle_color(&style).resolve(ctx);

    if let Some(mut c) = ctx.at(surface) {
        c.set_background(background);
    }
    if let Some(mut c) = ctx.at(handle) {
        c.set_background(handle_color);
        c.set_radius(style.handle_height / 2.0);
    }
    sync_scrim(ctx);
}

/// Extension trait for mutating a spawned [`BottomSheet`]. Size setters are
/// `set_sheet_*` to avoid clashing with `StyleContext::set_width`/`set_height`.
pub trait BottomSheetContextExt {
    fn set_title(&mut self, title: impl Into<String>);
    fn set_handle_visible(&mut self, visible: bool);
    fn set_dismissible(&mut self, dismissible: bool);
    fn open(&mut self);
    /// Closes without emitting [`SheetClosed`], even if not dismissible.
    fn close(&mut self);

    fn set_scrim(&mut self, scrim: bool);

    fn set_sheet_width(&mut self, width: Val);
    fn set_sheet_height(&mut self, height: Val);
    fn set_sheet_min_height(&mut self, height: Val);
    fn set_sheet_max_height(&mut self, height: Val);

    fn set_background(&mut self, color: impl IntoColorSource);
    fn set_handle_color(&mut self, color: impl IntoColorSource);
    fn set_title_color(&mut self, color: impl IntoColorSource);

    fn sync_visuals(&mut self);
}

impl BottomSheetContextExt for Context<'_, BottomSheet> {
    fn set_title(&mut self, title: impl Into<String>) {
        let h = self.me().title;
        if let Some(mut c) = self.at(h) {
            c.set_text(title);
        }
    }

    fn set_handle_visible(&mut self, visible: bool) {
        if self.me().handle_visible == visible {
            return;
        }
        self.me().handle_visible = visible;
        let (grip, style) = {
            let me = self.me();
            (me.grip, me.style)
        };
        if let Some(mut c) = self.at(grip) {
            c.set_style(grip_style(&style, visible));
        }
    }

    fn set_dismissible(&mut self, dismissible: bool) {
        let me = self.me();
        me.dismissible = dismissible;
        if !dismissible && me.drag.take().is_some_and(|d| d.active) {
            set_position(self, Position::Rest);
        }
    }

    fn open(&mut self) {
        if self.me().position != Position::Closed {
            return;
        }
        set_position(self, Position::Rest);
    }

    fn close(&mut self) {
        if self.me().position == Position::Closed {
            return;
        }
        close(self);
    }

    fn set_scrim(&mut self, scrim: bool) {
        if self.me().scrim == scrim {
            return;
        }
        self.me().scrim = scrim;
        sync_scrim(self);
    }

    fn set_sheet_width(&mut self, width: Val) {
        if self.me().size.width == width {
            return;
        }
        self.me().size.width = width;
        apply_size(self);
    }

    fn set_sheet_height(&mut self, height: Val) {
        if self.me().size.height == height {
            return;
        }
        self.me().size.height = height;
        apply_size(self);
    }

    fn set_sheet_min_height(&mut self, height: Val) {
        if self.me().size.min_height == height {
            return;
        }
        self.me().size.min_height = height;
        apply_size(self);
    }

    fn set_sheet_max_height(&mut self, height: Val) {
        if self.me().size.max_height == height {
            return;
        }
        self.me().size.max_height = height;
        apply_size(self);
    }

    fn set_background(&mut self, color: impl IntoColorSource) {
        let color = color.into_color_source();
        if self.me().overrides.background == color {
            return;
        }
        self.me().overrides.background = color;
        sync_visuals(self);
    }

    fn set_handle_color(&mut self, color: impl IntoColorSource) {
        let color = color.into_color_source();
        if self.me().overrides.handle_color == color {
            return;
        }
        self.me().overrides.handle_color = color;
        sync_visuals(self);
    }

    fn set_title_color(&mut self, color: impl IntoColorSource) {
        let color = color.into_color_source();
        if self.me().overrides.title_color == color {
            return;
        }
        self.me().overrides.title_color = color;
        let (title, source) = {
            let me = self.me();
            (me.title, me.overrides.title_color(&me.style))
        };
        if let Some(mut c) = self.at(title) {
            c.set_color(source);
        }
    }

    fn sync_visuals(&mut self) {
        sync_visuals(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geometry::Color;

    #[test]
    fn builder_defaults() {
        let b = bottom_sheet();
        assert_eq!(b.title, "");
        assert!(b.handle);
        assert!(b.dismissible);
        assert_eq!(
            b.size,
            SheetSize {
                width: percent(100.0),
                height: auto(),
                min_height: auto(),
                max_height: percent(100.0),
            }
        );
        assert_eq!(b.style, BottomSheetStyle::standard());
        assert_eq!(b.overrides, BottomSheetOverrides::default());
    }

    #[test]
    fn builder_verbs_set_their_fields() {
        let b = bottom_sheet()
            .title("OfficeWifi 3")
            .handle(false)
            .dismissible(false)
            .width(px(320.0))
            .height(percent(50.0))
            .min_height(px(100.0))
            .max_height(percent(90.0))
            .background(Color::WHITE)
            .handle_color(ColorRole::Primary)
            .title_color(ColorRole::Error);
        assert_eq!(b.title, "OfficeWifi 3");
        assert!(!b.handle);
        assert!(!b.dismissible);
        assert_eq!(
            b.size,
            SheetSize {
                width: px(320.0),
                height: percent(50.0),
                min_height: px(100.0),
                max_height: percent(90.0),
            }
        );
        assert_eq!(
            b.overrides.background,
            Some(ColorSource::Exact(Color::WHITE))
        );
        assert_eq!(
            b.overrides.handle_color,
            Some(ColorSource::Role(ColorRole::Primary))
        );
        assert_eq!(
            b.overrides.title_color,
            Some(ColorSource::Role(ColorRole::Error))
        );
    }

    #[test]
    fn closing_needs_half_the_height_or_a_flick_down() {
        let st = BottomSheetStyle::standard();
        assert!(!should_close(&st, 100.0, 220.0, 0.0));
        assert!(!should_close(&st, 110.0, 220.0, 0.0), "exactly half stays");
        assert!(should_close(&st, 111.0, 220.0, 0.0));
        assert!(should_close(&st, 20.0, 220.0, 1000.0), "flick down");
        assert!(
            !should_close(&st, 20.0, 220.0, st.fling_velocity),
            "not past it"
        );
        assert!(!should_close(&st, 20.0, 220.0, -5000.0), "flick up");
    }

    #[test]
    fn custom_style_tunes_closing() {
        let st = BottomSheetStyle {
            close_fraction: 0.25,
            fling_velocity: 2000.0,
            ..BottomSheetStyle::standard()
        };
        assert!(should_close(&st, 56.0, 220.0, 0.0));
        assert!(!should_close(&st, 20.0, 220.0, 1000.0));
    }

    #[test]
    fn style_sets_defaults_and_overrides_still_win() {
        let st = BottomSheetStyle {
            bg: ColorRole::Primary,
            handle_color: ColorRole::Secondary,
            ..BottomSheetStyle::standard()
        };
        let b = bottom_sheet().style(st).handle_color(ColorRole::Error);
        assert_eq!(b.style, st);
        assert_eq!(
            b.overrides.background(&b.style),
            ColorSource::Role(ColorRole::Primary)
        );
        assert_eq!(
            b.overrides.handle_color(&b.style),
            ColorSource::Role(ColorRole::Error)
        );
    }

    #[test]
    fn closed_is_a_whole_parent_height_down() {
        assert_eq!(Position::Rest.top(), px(0.0));
        assert_eq!(Position::Dragged(42.0).top(), px(42.0));
        assert_eq!(Position::Closed.top(), percent(100.0));
    }
}
