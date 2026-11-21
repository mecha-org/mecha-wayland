//! `BottomSheet`: a themed surface that rises from the bottom of its
//! parent and holds content the caller supplies.
//!
//! The sheet is only a frame. It draws a drag handle and a header with a
//! title, and leaves two empty slots for the caller to fill:
//!
//! ```text
//! BottomSheet            fills its parent; pins the surface to the bottom
//! └─ surface             themed background, rounded top corners
//!    ├─ grip             full-width drag area around the handle
//!    │  └─ handle        the drag-handle pill
//!    ├─ header           a row with the title; also a drag area
//!    ├─ body             caller content, padded       ← BottomSheet::body
//!    └─ footer           caller content, edge to edge ← BottomSheet::footer
//! ```
//!
//! Content goes in by spawning into those slots. From a parent's build:
//!
//! ```ignore
//! let sheet = s.spawn(me, bottom_sheet().title("OfficeWifi 3"));
//! let body = s.widget::<BottomSheet>(sheet).unwrap().body();
//! s.spawn(body, text("Enter the password"));
//! ```
//!
//! # Size
//!
//! Width, height and the height bounds are layout [`Val`]s: `px(..)`,
//! `percent(..)` of the parent, or `auto()`. The defaults are a full-width
//! sheet as tall as its content, never taller than the parent.
//!
//! # Drag to dismiss
//!
//! A press on the grip or the header that moves down more than
//! [`DRAG_SLOP`] drags the sheet: the surface slides down with the finger,
//! keeping its size, and never rises above where it rests. Once a drag
//! starts the sheet claims the contact, so whatever sits in the header (a
//! button, say) gets `PressCancelled` instead of a click.
//!
//! On release the sheet closes if it was dragged down more than half its
//! height or flicked down faster than [`FLING_VELOCITY`]; otherwise it
//! returns to rest.
//!
//! # Open and close
//!
//! A sheet is open once spawned. When the user closes it (by dragging it
//! down) it emits [`SheetClosed`]: for a prompt, a cancel. The owner can
//! also `close` and `open` it from code; those emit nothing, since the
//! owner already knows (it closes the sheet itself once a password is
//! accepted, say). A closed sheet stays below the bottom edge and takes
//! no drags. It does not remove itself: the owner does that, or calls
//! `open` to bring it back.
//!
//! The sheet knows nothing about Wayland surfaces: it fills whatever node
//! it is spawned under, a window or a layer surface alike.

use crate::color::{ColorSource, IntoColorSource};
use crate::text::{Text, TextContextExt, text};
use app::{Build, Context, Event, Handle, Spawner, Widget};
use geometry::{Corners, Insets, Rect};
use interactivity::{ContactId, ContextClaim, Press, PressCancelled, PressMoved, Release};
use layout::{Align, ComputedLayout, Justify, LayoutStyle, StyleContext, Val, auto, percent, px};
use theme::{ColorRole, Shape, Spacing, SpawnerThemeExt, TextVariant};
use widgets::{Div, DivContext, TextOverflow, div};

/// The drag handle's size, in logical pixels.
pub const HANDLE_WIDTH: f32 = 32.0;
pub const HANDLE_HEIGHT: f32 = 4.0;

/// The grip's height: the handle with its space above and below.
pub const GRIP_HEIGHT: f32 = Spacing::Space100.dp() + HANDLE_HEIGHT + Spacing::Space50.dp();

/// The header row's minimum height, in logical pixels.
pub const HEADER_MIN_HEIGHT: f32 = 56.0;

/// How far down, in logical pixels, a press must move before it drags the
/// sheet. Matches the scroll crate's default slop, so a sheet and a
/// scroll view feel the same under a finger.
pub const DRAG_SLOP: f32 = 10.0;

/// The share of its height a sheet must be dragged down to close on a
/// slow release.
pub const DISMISS_FRACTION: f32 = 0.5;

/// The downward speed, in logical pixels per millisecond, past which a
/// release is a flick that closes the sheet: 700 px/s, Flutter's threshold for
/// the same gesture.
pub const FLING_VELOCITY: f32 = 0.7;

/// A finger still this long, in milliseconds, before it lifts has no
/// speed left to flick with.
const FLING_STALE_MS: u32 = 100;

/// The user closed the sheet: for a prompt, a cancel. Emitted at the
/// sheet; its owner removes it, or calls `open`. Not emitted for `close`
/// from code.
///
/// Today the only way is dragging it down. Non-exhaustive, so a field can
/// be added later (which gesture, say) without breaking anyone.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SheetClosed {}

impl Event for SheetClosed {}

/// Colors set by the caller instead of the theme defaults. `None` keeps
/// the default role.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BottomSheetOverrides {
    pub background: Option<ColorSource>,
    pub handle_color: Option<ColorSource>,
    pub title_color: Option<ColorSource>,
}

impl BottomSheetOverrides {
    /// The surface color: `SurfaceContainerHighest` unless overridden.
    fn background(&self) -> ColorSource {
        self.background
            .unwrap_or(ColorSource::Role(ColorRole::SurfaceContainerHighest))
    }

    /// The drag handle's color: `OnSurface` unless overridden.
    fn handle_color(&self) -> ColorSource {
        self.handle_color
            .unwrap_or(ColorSource::Role(ColorRole::OnSurface))
    }

    /// The title's color: `OnSurface` unless overridden.
    fn title_color(&self) -> ColorSource {
        self.title_color
            .unwrap_or(ColorSource::Role(ColorRole::OnSurface))
    }
}

/// The surface's size as the caller set it.
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
    pub(crate) size: SheetSize,
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
            size: SheetSize::default(),
            overrides: BottomSheetOverrides::default(),
        }
    }

    /// The header's title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Show the drag handle. On by default. Hidden, the header is still a
    /// drag area while the sheet is dismissible.
    pub fn handle(mut self, visible: bool) -> Self {
        self.handle = visible;
        self
    }

    /// Let a drag down on the grip or header close the sheet. On by
    /// default; turn it off for a prompt that must be answered. The owner
    /// can still `close` it either way.
    pub fn dismissible(mut self, dismissible: bool) -> Self {
        self.dismissible = dismissible;
        self
    }

    /// The surface's width. `percent(100.0)` by default; a narrower sheet
    /// is centered.
    pub fn width(mut self, width: Val) -> Self {
        self.size.width = width;
        self
    }

    /// The surface's height. `auto()` by default: as tall as the content.
    pub fn height(mut self, height: Val) -> Self {
        self.size.height = height;
        self
    }

    /// The smallest height. `auto()` by default.
    pub fn min_height(mut self, height: Val) -> Self {
        self.size.min_height = height;
        self
    }

    /// The largest height. `percent(100.0)` of the parent by default.
    pub fn max_height(mut self, height: Val) -> Self {
        self.size.max_height = height;
        self
    }

    /// The surface color, in place of `SurfaceContainerHighest`.
    pub fn background(mut self, color: impl IntoColorSource) -> Self {
        self.overrides.background = color.into_color_source();
        self
    }

    /// The drag handle's color, in place of `OnSurface`.
    pub fn handle_color(mut self, color: impl IntoColorSource) -> Self {
        self.overrides.handle_color = color.into_color_source();
        self
    }

    /// The title's color, in place of `OnSurface`.
    pub fn title_color(mut self, color: impl IntoColorSource) -> Self {
        self.overrides.title_color = color.into_color_source();
        self
    }
}

impl Build for BottomSheetBuilder {
    type Widget = BottomSheet;
}

/// A drag in progress, from the press that may start it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Drag {
    contact: ContactId,
    /// The finger's y the slide is measured from: the press point until
    /// the slop is crossed, the crossing point after.
    origin: f32,
    /// The surface's height when the press landed: how far down it goes
    /// when closed, and what the close fraction is of.
    height: f32,
    /// Past the slop: the sheet follows the finger and holds the contact.
    active: bool,
    /// The finger's last y and timestamp, and its speed between the last
    /// two moves (logical px per ms, positive downward).
    last_y: f32,
    last_time: u32,
    velocity: f32,
}

/// Themed Mechanix Design bottom sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct BottomSheet {
    pub(crate) size: SheetSize,
    pub(crate) handle_visible: bool,
    pub(crate) dismissible: bool,
    /// How far below its resting place the surface sits, in logical
    /// pixels: the drag's slide, or the full height once closed.
    pub(crate) offset: f32,
    pub(crate) closed: bool,
    pub(crate) drag: Option<Drag>,
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
    /// The slot for the sheet's main content: a padded column. Spawn into
    /// it.
    #[inline]
    pub fn body(&self) -> Handle<Div> {
        self.body
    }

    /// The slot under the body, with no padding, for content that runs
    /// edge to edge at the bottom of the sheet (a keypad, an action row).
    /// Empty, it takes no space.
    #[inline]
    pub fn footer(&self) -> Handle<Div> {
        self.footer
    }

    /// The themed surface that holds the grip, header and slots.
    #[inline]
    pub fn surface(&self) -> Handle<Div> {
        self.surface
    }

    /// The full-width drag area around the handle.
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

    /// A drag is sliding the sheet right now.
    #[inline]
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some_and(|d| d.active)
    }

    /// Closed and waiting for its owner to remove it or open it again.
    #[inline]
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// How far below its resting place the surface sits, in logical
    /// pixels.
    #[inline]
    pub fn offset(&self) -> f32 {
        self.offset
    }

    #[inline]
    pub fn overrides(&self) -> &BottomSheetOverrides {
        &self.overrides
    }
}

/// The sheet node itself: fills the parent, pushes the surface to the
/// bottom and centers a narrower one.
fn sheet_style() -> LayoutStyle {
    LayoutStyle::default()
        .column()
        .size(percent(100.0), percent(100.0))
        .justify(Justify::End)
        .align_items(Align::Center)
}

/// The surface at its size, moved `offset` down from where it rests. A
/// relative offset moves it without laying anything out again.
fn surface_style(size: SheetSize, offset: f32) -> LayoutStyle {
    LayoutStyle::default()
        .column()
        .relative()
        .inset(Insets::new(px(offset), auto(), auto(), auto()))
        .width(size.width)
        .height(size.height)
        .min_height(size.min_height)
        .max_height(size.max_height)
        .shrink(0.0)
        .align_items(Align::Stretch)
}

/// The surface's top corners are rounded, the bottom ones sit on the edge
/// of the parent.
fn surface_radii() -> Corners<f32> {
    let r = Shape::ExtraLarge.resolve_radius_dp(0.0);
    Corners::new(r, r, 0.0, 0.0)
}

fn grip_style(visible: bool) -> LayoutStyle {
    let style = LayoutStyle::default()
        .column()
        .width(percent(100.0))
        .shrink(0.0)
        .align_items(Align::Center)
        .padding(Insets::new(
            px(Spacing::Space100.dp()),
            px(0.0),
            px(Spacing::Space50.dp()),
            px(0.0),
        ));
    if visible { style } else { style.hidden() }
}

fn handle_style() -> LayoutStyle {
    LayoutStyle::default()
        .size(px(HANDLE_WIDTH), px(HANDLE_HEIGHT))
        .shrink(0.0)
}

fn header_style() -> LayoutStyle {
    LayoutStyle::default()
        .row()
        .align_items(Align::Center)
        .min_height(px(HEADER_MIN_HEIGHT))
        .column_gap(px(Spacing::Space200.dp()))
        .padding(Insets::new(
            px(Spacing::Space100.dp()),
            px(Spacing::Space300.dp()),
            px(Spacing::Space100.dp()),
            px(Spacing::Space300.dp()),
        ))
}

fn title_style() -> LayoutStyle {
    // A zero min width lets a long title shrink and ellipsize instead of
    // pushing the row wider than the sheet.
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
        *s.component_mut::<LayoutStyle>(me).unwrap() = sheet_style();

        let surface = s.spawn(
            me,
            div()
                .style(surface_style(b.size, 0.0))
                .background(b.overrides.background().resolve(s))
                .radii(surface_radii()),
        );
        let grip = s.spawn(surface, div().style(grip_style(b.handle)));
        let handle = s.spawn(
            grip,
            div()
                .style(handle_style())
                .background(b.overrides.handle_color().resolve(s))
                .radius(HANDLE_HEIGHT / 2.0),
        );
        let header = s.spawn(surface, div().style(header_style()));
        let title = s.spawn(
            header,
            text(b.title)
                .variant(TextVariant::HeadlineSmall)
                .color(b.overrides.title_color())
                .max_lines(1)
                .overflow(TextOverflow::Ellipsis)
                .style(title_style()),
        );
        let body = s.spawn(surface, div().style(body_style()));
        let footer = s.spawn(surface, div().style(footer_style()));

        s.on_theme(me, sync_visuals);
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
            offset: 0.0,
            closed: false,
            drag: None,
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

/// The last laid-out rect of `handle`, or zero before the first layout.
fn rect_of<W: Widget, V: Widget>(ctx: &mut Context<'_, W>, handle: Handle<V>) -> Rect {
    ctx.at(handle)
        .and_then(|c| c.component::<ComputedLayout>().map(|l| l.rect))
        .unwrap_or(Rect::ZERO)
}

/// A press on the grip or header: note where it landed. Nothing moves,
/// and nothing is claimed, until it crosses the slop downward.
fn on_press(ctx: &mut Context<'_, BottomSheet>, e: &Press) {
    let (dismissible, busy, closed, surface) = {
        let me = ctx.me();
        (me.dismissible, me.drag.is_some(), me.closed, me.surface)
    };
    if !dismissible || busy || closed {
        return;
    }
    let height = rect_of(ctx, surface).height();
    ctx.me().drag = Some(Drag {
        contact: e.contact,
        origin: e.position.y,
        height,
        active: false,
        last_y: e.position.y,
        last_time: e.time,
        velocity: 0.0,
    });
}

/// Past the slop the sheet claims the contact and slides with the
/// finger, never above its resting place.
fn on_press_moved(ctx: &mut Context<'_, BottomSheet>, e: &PressMoved) {
    let Some(mut drag) = ctx.me().drag.filter(|d| d.contact == e.contact) else {
        return;
    };
    let y = e.position.y;
    let dt = e.time.wrapping_sub(drag.last_time);
    if dt > 0 {
        drag.velocity = (y - drag.last_y) / dt as f32;
    }
    drag.last_y = y;
    drag.last_time = e.time;
    if !drag.active {
        // Only a move down starts a drag: up has nowhere to go, and
        // leaves the press to whatever is under it.
        if y - drag.origin <= DRAG_SLOP {
            ctx.me().drag = Some(drag);
            return;
        }
        // Measure from the crossing point, so the sheet does not jump by
        // the slop.
        drag.active = true;
        drag.origin = y;
        ctx.claim_contact(e.contact);
    }
    ctx.me().drag = Some(drag);
    set_offset(ctx, (y - drag.origin).max(0.0));
}

/// The press is over: `time` is the release's timestamp, `None` for a
/// cancel, which never closes. A drag far or fast enough closes the
/// sheet; any other returns it to rest.
fn end_drag(ctx: &mut Context<'_, BottomSheet>, contact: ContactId, time: Option<u32>) {
    let Some(drag) = ctx.me().drag.filter(|d| d.contact == contact) else {
        return;
    };
    ctx.me().drag = None;
    if !drag.active {
        return;
    }
    let Some(time) = time else {
        set_offset(ctx, 0.0);
        return;
    };
    let velocity = if time.wrapping_sub(drag.last_time) <= FLING_STALE_MS {
        drag.velocity
    } else {
        0.0
    };
    if should_close(ctx.me().offset, drag.height, velocity) {
        close(ctx, drag.height);
        ctx.emit(SheetClosed {}, ctx.handle());
    } else {
        set_offset(ctx, 0.0);
    }
}

/// A release `offset` px down a sheet `height` px tall, moving at
/// `velocity` (positive downward), closes it.
fn should_close(offset: f32, height: f32, velocity: f32) -> bool {
    velocity > FLING_VELOCITY || offset > height * DISMISS_FRACTION
}

/// Push the surface `height` px down, off the bottom edge, and stop
/// taking drags.
fn close(ctx: &mut Context<'_, BottomSheet>, height: f32) {
    let me = ctx.me();
    me.closed = true;
    me.drag = None;
    set_offset(ctx, height);
}

fn set_offset(ctx: &mut Context<'_, BottomSheet>, offset: f32) {
    ctx.me().offset = offset;
    apply_size(ctx);
}

/// Write the surface's layout style from the current size and offset.
fn apply_size(ctx: &mut Context<'_, BottomSheet>) {
    let (size, offset, surface) = {
        let me = ctx.me();
        (me.size, me.offset, me.surface)
    };
    if let Some(mut c) = ctx.at(surface) {
        c.set_style(surface_style(size, offset));
    }
}

/// Repaint the surface and handle from the theme and overrides. The title
/// is a themed `Text` and follows the theme by itself.
fn sync_visuals(ctx: &mut Context<'_, BottomSheet>) {
    let (overrides, surface, handle) = {
        let me = ctx.me();
        (me.overrides, me.surface, me.handle)
    };
    let background = overrides.background().resolve(ctx);
    let handle_color = overrides.handle_color().resolve(ctx);

    if let Some(mut c) = ctx.at(surface) {
        // Background first: a `Div` builds on its current quad, and only a
        // visible one keeps the radii.
        c.set_background(background);
        c.set_radii(surface_radii());
    }
    if let Some(mut c) = ctx.at(handle) {
        c.set_background(handle_color);
        c.set_radius(HANDLE_HEIGHT / 2.0);
    }
}

/// Extension trait for mutating a spawned [`BottomSheet`].
///
/// The size setters are named `set_sheet_*`, apart from `StyleContext`'s
/// `set_width`/`set_height`, which every `Context` has.
pub trait BottomSheetContextExt {
    fn set_title(&mut self, title: impl Into<String>);
    fn set_handle_visible(&mut self, visible: bool);
    /// Turning it off returns a sheet being dragged to rest.
    fn set_dismissible(&mut self, dismissible: bool);
    /// Bring a closed sheet back to rest, taking drags again. Emits
    /// nothing; nothing happens to an open sheet.
    fn open(&mut self);
    /// Close the sheet from code, as a drag would, but without emitting
    /// [`SheetClosed`]: the caller already knows. Ends a drag in progress.
    /// Nothing happens to a closed sheet. Works whether or not the sheet is
    /// dismissible.
    fn close(&mut self);

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
        let grip = self.me().grip;
        if let Some(mut c) = self.at(grip) {
            c.set_style(grip_style(visible));
        }
    }

    fn set_dismissible(&mut self, dismissible: bool) {
        let me = self.me();
        me.dismissible = dismissible;
        if !dismissible && me.drag.take().is_some_and(|d| d.active) {
            set_offset(self, 0.0);
        }
    }

    fn open(&mut self) {
        let me = self.me();
        if !me.closed {
            return;
        }
        me.closed = false;
        set_offset(self, 0.0);
    }

    fn close(&mut self) {
        if self.me().closed {
            return;
        }
        let surface = self.me().surface;
        let height = rect_of(self, surface).height();
        close(self, height);
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
            (me.title, me.overrides.title_color())
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
    use app::{App, NodeId};
    use geometry::{Color, Point};
    use interactivity::{ContactInput, ContactPhase, InteractivityModule};
    use paint::{Paint, Quad};
    use theme::{AppThemeExt, MechanixTheme, ThemeReader};
    use window::{WindowModule, window};

    const W: f32 = 400.0;
    const H: f32 = 800.0;

    /// An app that lays out, paints and takes contacts, with a font, and
    /// a `W` x `H` window to hold a sheet.
    fn sheet_app() -> (App, NodeId) {
        let mut app = App::new();
        app.add_module(layout::LayoutModule)
            .add_module(paint::PaintModule)
            .add_module(WindowModule)
            .add_module(InteractivityModule)
            .add_module(MechanixTheme::light());
        let mut atlas = atlas::Atlas::new();
        let font = atlas
            .add_font(include_bytes!(
                "../../../crates/atlas/tests/fixtures/Inter-Regular.ttf"
            ))
            .unwrap();
        app.insert_resource(atlas);
        app.insert_resource(crate::font::FontBook::new(font));
        let win = app
            .spawn(
                app.root(),
                window().layout(LayoutStyle::default().size(px(W), px(H))),
            )
            .id();
        (app, win)
    }

    fn settle(app: &mut App) {
        for _ in 0..4 {
            app.tick();
        }
    }

    fn rect(app: &App, id: impl Into<NodeId>) -> Rect {
        app.component::<ComputedLayout>(id).unwrap().rect
    }

    fn sheet(app: &App, h: Handle<BottomSheet>) -> &BottomSheet {
        app.widget::<BottomSheet>(h).unwrap()
    }

    fn surface_rect(app: &App, h: Handle<BottomSheet>) -> Rect {
        rect(app, sheet(app, h).surface())
    }

    fn surface_height(app: &App, h: Handle<BottomSheet>) -> f32 {
        surface_rect(app, h).height()
    }

    fn quad(app: &App, id: impl Into<NodeId>) -> Quad {
        match app.component::<Paint>(id).unwrap() {
            Paint::Quad(q) => *q,
            other => panic!("expected a quad, got {other:?}"),
        }
    }

    fn title_string(app: &App, h: Handle<BottomSheet>) -> String {
        let text = app.widget::<Text>(sheet(app, h).title()).unwrap().native();
        app.widget::<widgets::Text>(text).unwrap().string.clone()
    }

    /// A fixed-size box, standing in for caller content.
    fn block(height: f32) -> widgets::DivBuilder {
        div().style(LayoutStyle::default().size(px(100.0), px(height)))
    }

    // ── builder ─────────────────────────────────────────────────────────

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
        assert!(!should_close(100.0, 220.0, 0.0));
        assert!(!should_close(110.0, 220.0, 0.0), "exactly half stays");
        assert!(should_close(111.0, 220.0, 0.0));
        assert!(should_close(20.0, 220.0, 1.0), "flick down");
        assert!(!should_close(20.0, 220.0, FLING_VELOCITY), "not past it");
        assert!(!should_close(20.0, 220.0, -5.0), "flick up");
    }

    // ── layout ──────────────────────────────────────────────────────────

    #[test]
    fn surface_holds_grip_header_body_footer_in_order() {
        let (mut app, win) = sheet_app();
        let h = app.spawn(win, bottom_sheet().title("Title"));
        let s = sheet(&app, h).clone();
        let children = app.children(s.surface()).unwrap().to_vec();
        assert_eq!(
            children,
            vec![
                s.grip().id(),
                s.header().id(),
                s.body().id(),
                s.footer().id()
            ]
        );
        assert_eq!(app.parent(s.handle()), Some(s.grip().id()));
        assert_eq!(app.parent(s.title()), Some(s.header().id()));
        assert_eq!(title_string(&app, h), "Title");
    }

    #[test]
    fn auto_height_sits_at_the_bottom_and_fits_its_content() {
        let (mut app, win) = sheet_app();
        let h = app.spawn(win, bottom_sheet().title("Title"));
        let body = sheet(&app, h).body();
        settle(&mut app);
        let empty = surface_rect(&app, h);
        assert_eq!(empty.width(), W);
        assert_eq!(empty.bottom(), H, "pinned to the bottom");
        assert!(empty.height() < H, "fits, not full height");

        app.spawn(body, block(120.0));
        settle(&mut app);
        let filled = surface_rect(&app, h);
        assert_eq!(filled.height(), empty.height() + 120.0);
        assert_eq!(filled.bottom(), H);
    }

    #[test]
    fn auto_height_stops_at_the_max_height() {
        let (mut app, win) = sheet_app();
        let h = app.spawn(win, bottom_sheet());
        let body = sheet(&app, h).body();
        app.spawn(body, block(H * 2.0));
        settle(&mut app);
        assert_eq!(surface_height(&app, h), H);

        let (mut app, win) = sheet_app();
        let h = app.spawn(win, bottom_sheet().max_height(percent(75.0)));
        let body = sheet(&app, h).body();
        app.spawn(body, block(H * 2.0));
        settle(&mut app);
        assert_eq!(surface_height(&app, h), H * 0.75);
    }

    #[test]
    fn height_takes_percent_and_px() {
        for (height, expected) in [
            (percent(50.0), H / 2.0),
            (percent(100.0), H),
            (px(300.0), 300.0),
        ] {
            let (mut app, win) = sheet_app();
            let h = app.spawn(win, bottom_sheet().height(height));
            settle(&mut app);
            let r = surface_rect(&app, h);
            assert_eq!((r.height(), r.bottom()), (expected, H), "{height:?}");
        }
    }

    #[test]
    fn a_narrower_sheet_is_centered() {
        for (width, expected) in [(percent(50.0), W / 2.0), (px(320.0), 320.0)] {
            let (mut app, win) = sheet_app();
            let h = app.spawn(win, bottom_sheet().width(width));
            settle(&mut app);
            let r = surface_rect(&app, h);
            assert_eq!(r.width(), expected, "{width:?}");
            assert_eq!(r.x(), (W - expected) / 2.0, "{width:?}");
        }
    }

    #[test]
    fn footer_stays_at_the_bottom_of_a_tall_sheet() {
        let (mut app, win) = sheet_app();
        let h = app.spawn(win, bottom_sheet().height(percent(100.0)));
        let (body, footer) = (sheet(&app, h).body(), sheet(&app, h).footer());
        app.spawn(body, block(40.0));
        app.spawn(footer, block(200.0));
        settle(&mut app);
        let f = rect(&app, footer);
        assert_eq!((f.height(), f.bottom()), (200.0, H));
        assert_eq!(f.width(), W, "edge to edge");
    }

    #[test]
    fn empty_footer_takes_no_space() {
        let (mut app, win) = sheet_app();
        let h = app.spawn(win, bottom_sheet());
        settle(&mut app);
        assert_eq!(rect(&app, sheet(&app, h).footer()).height(), 0.0);
    }

    #[test]
    fn handle_is_a_centered_pill_and_can_be_hidden() {
        let (mut app, win) = sheet_app();
        let h = app.spawn(win, bottom_sheet());
        settle(&mut app);
        let pill = rect(&app, sheet(&app, h).handle());
        assert_eq!((pill.width(), pill.height()), (HANDLE_WIDTH, HANDLE_HEIGHT));
        assert_eq!(pill.x() + pill.width() / 2.0, W / 2.0);
        let grip = rect(&app, sheet(&app, h).grip());
        assert_eq!((grip.width(), grip.height()), (W, GRIP_HEIGHT));
        let with_handle = surface_height(&app, h);

        let (mut app, win) = sheet_app();
        let h = app.spawn(win, bottom_sheet().handle(false));
        settle(&mut app);
        assert_eq!(with_handle - surface_height(&app, h), GRIP_HEIGHT);
    }

    // ── colors ──────────────────────────────────────────────────────────

    #[test]
    fn surface_takes_theme_colors_and_rounded_top_corners() {
        let (mut app, win) = sheet_app();
        let h = app.spawn(win, bottom_sheet());
        let s = sheet(&app, h).clone();
        let surface = quad(&app, s.surface());
        assert_eq!(surface.color, app.color(ColorRole::SurfaceContainerHighest));
        assert_eq!(surface.radii, Corners::new(28.0, 28.0, 0.0, 0.0));
        assert_eq!(
            quad(&app, s.handle()).color,
            app.color(ColorRole::OnSurface)
        );
    }

    #[test]
    fn theme_change_repaints_the_surface() {
        let (mut app, win) = sheet_app();
        let h = app.spawn(win, bottom_sheet());
        let surface = sheet(&app, h).surface();
        let light = quad(&app, surface).color;
        app.set_theme(MechanixTheme::dark());
        settle(&mut app);
        let q = quad(&app, surface);
        assert_ne!(q.color, light);
        assert_eq!(q.color, app.color(ColorRole::SurfaceContainerHighest));
        assert_eq!(q.radii, Corners::new(28.0, 28.0, 0.0, 0.0));
    }

    #[test]
    fn overrides_beat_the_theme() {
        let (mut app, win) = sheet_app();
        let h = app.spawn(
            win,
            bottom_sheet()
                .background(Color::BLACK)
                .handle_color(ColorRole::Primary),
        );
        let s = sheet(&app, h).clone();
        assert_eq!(quad(&app, s.surface()).color, Color::BLACK);
        assert_eq!(quad(&app, s.handle()).color, app.color(ColorRole::Primary));
    }

    // ── context setters ─────────────────────────────────────────────────

    struct Poke;
    impl app::Event for Poke {}

    type Action = Box<dyn FnMut(&mut Context<'_, Controller>)>;

    /// Runs its closure on a `Poke`, so a test can reach a `Context`.
    struct Controller;
    struct ControllerBuilder(Action);
    impl Build for ControllerBuilder {
        type Widget = Controller;
    }
    impl Widget for Controller {
        type Builder = ControllerBuilder;
        fn build(b: ControllerBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
            let mut action = b.0;
            s.on::<Poke>(me, move |ctx, _| action(ctx));
            Controller
        }
    }

    /// Run `f` on the sheet's `Context`, then let layout settle.
    fn with_sheet(
        app: &mut App,
        h: Handle<BottomSheet>,
        mut f: impl FnMut(&mut Context<'_, BottomSheet>) + 'static,
    ) {
        let controller = app.spawn(
            app.root(),
            ControllerBuilder(Box::new(move |ctx: &mut Context<'_, Controller>| {
                f(&mut ctx.at(h).unwrap());
            })),
        );
        app.emit(Poke, controller.id());
        app.flush();
        settle(app);
        app.remove(controller);
    }

    #[test]
    fn set_title_changes_the_header() {
        let (mut app, win) = sheet_app();
        let h = app.spawn(win, bottom_sheet().title("Before"));
        with_sheet(&mut app, h, |c| c.set_title("After"));
        assert_eq!(title_string(&app, h), "After");
    }

    #[test]
    fn size_setters_resize_the_surface() {
        let (mut app, win) = sheet_app();
        let h = app.spawn(win, bottom_sheet());
        settle(&mut app);
        with_sheet(&mut app, h, |c| {
            c.set_sheet_height(percent(100.0));
            c.set_sheet_width(percent(50.0));
        });
        assert_eq!(sheet(&app, h).size().height, percent(100.0));
        let r = surface_rect(&app, h);
        assert_eq!((r.width(), r.height()), (W / 2.0, H));

        with_sheet(&mut app, h, |c| c.set_sheet_max_height(px(500.0)));
        assert_eq!(surface_height(&app, h), 500.0);
        with_sheet(&mut app, h, |c| {
            c.set_sheet_height(px(50.0));
            c.set_sheet_min_height(px(200.0));
        });
        assert_eq!(surface_height(&app, h), 200.0);
    }

    #[test]
    fn set_handle_visible_toggles_the_grip() {
        let (mut app, win) = sheet_app();
        let h = app.spawn(win, bottom_sheet());
        settle(&mut app);
        let shown = surface_height(&app, h);
        with_sheet(&mut app, h, |c| c.set_handle_visible(false));
        assert!(!sheet(&app, h).is_handle_visible());
        assert_eq!(shown - surface_height(&app, h), GRIP_HEIGHT);
        with_sheet(&mut app, h, |c| c.set_handle_visible(true));
        assert_eq!(surface_height(&app, h), shown);
    }

    #[test]
    fn color_setters_repaint_and_none_restores_the_theme() {
        let (mut app, win) = sheet_app();
        let h = app.spawn(win, bottom_sheet());
        let s = sheet(&app, h).clone();
        with_sheet(&mut app, h, |c| {
            c.set_background(Color::BLACK);
            c.set_handle_color(ColorRole::Primary);
            c.set_title_color(ColorRole::Error);
        });
        assert_eq!(quad(&app, s.surface()).color, Color::BLACK);
        assert_eq!(quad(&app, s.surface()).radii, surface_radii());
        assert_eq!(quad(&app, s.handle()).color, app.color(ColorRole::Primary));
        let native = app.widget::<Text>(s.title()).unwrap().native();
        assert_eq!(
            app.widget::<widgets::Text>(native).unwrap().color,
            app.color(ColorRole::Error)
        );

        with_sheet(&mut app, h, |c| c.set_background(None::<ColorSource>));
        assert_eq!(
            quad(&app, s.surface()).color,
            app.color(ColorRole::SurfaceContainerHighest)
        );
    }

    // ── a parent filling the slots ──────────────────────────────────────

    /// Spawns a sheet and fills its body during its own build; records
    /// what the sheet reports.
    struct Host {
        sheet: Handle<BottomSheet>,
        closed: usize,
        title_cancelled: bool,
    }
    struct HostBuilder(BottomSheetBuilder);
    impl Build for HostBuilder {
        type Widget = Host;
    }
    impl Widget for Host {
        type Builder = HostBuilder;
        fn build(b: HostBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
            *s.component_mut::<LayoutStyle>(me).unwrap() =
                LayoutStyle::default().size(percent(100.0), percent(100.0));
            let sheet = s.spawn(me, b.0);
            let (body, title) = {
                let w = s.widget::<BottomSheet>(sheet).unwrap();
                (w.body(), w.title())
            };
            s.spawn(body, block(100.0));
            s.on::<SheetClosed>(sheet, |ctx, _| ctx.me().closed += 1);
            s.on::<PressCancelled>(title, |ctx, _| ctx.me().title_cancelled = true);
            Host {
                sheet,
                closed: 0,
                title_cancelled: false,
            }
        }
    }

    #[test]
    fn a_parent_fills_the_body_during_its_build() {
        let (mut app, win) = sheet_app();
        let host = app.spawn(win, HostBuilder(bottom_sheet()));
        let sheet_h = app.widget::<Host>(host).unwrap().sheet;
        let body = sheet(&app, sheet_h).body();
        assert_eq!(app.children(body).map(<[_]>::len), Some(1));
    }

    // ── drag to dismiss ─────────────────────────────────────────────────

    const FINGER: ContactId = ContactId::Touch(0);

    fn contact(app: &mut App, win: NodeId, phase: ContactPhase, x: f32, y: f32, time: u32) {
        app.signal(ContactInput {
            window: win,
            contact: FINGER,
            phase,
            position: Point::new(x, y),
            time,
        });
        settle(app);
    }

    /// A finger down at `x` and the first `(y, time)`, then through the
    /// rest, still down.
    fn press_and_move(app: &mut App, win: NodeId, x: f32, points: &[(f32, u32)]) {
        let (y0, t0) = points[0];
        contact(app, win, ContactPhase::Moved, x, y0, t0);
        contact(app, win, ContactPhase::Pressed, x, y0, t0);
        for &(y, t) in &points[1..] {
            contact(app, win, ContactPhase::Moved, x, y, t);
        }
    }

    /// [`press_and_move`], lifting at the last point.
    fn gesture(app: &mut App, win: NodeId, x: f32, points: &[(f32, u32)]) {
        press_and_move(app, win, x, points);
        let (y, t) = *points.last().unwrap();
        contact(app, win, ContactPhase::Released, x, y, t);
    }

    /// The `(y, time)` points from `from` through each y in `path`, `step`
    /// ms apart.
    fn timed(from: Point, path: &[f32], step: u32) -> Vec<(f32, u32)> {
        let mut points = vec![(from.y, 1_000)];
        for (i, &y) in path.iter().enumerate() {
            points.push((y, 1_000 + step * (i as u32 + 1)));
        }
        points
    }

    /// A slow drag: a second between moves, far too slow to flick.
    fn drag(app: &mut App, win: NodeId, from: Point, path: &[f32]) {
        gesture(app, win, from.x, &timed(from, path, 1_000));
    }

    /// A fast drag: 10 ms between moves.
    fn flick(app: &mut App, win: NodeId, from: Point, path: &[f32]) {
        gesture(app, win, from.x, &timed(from, path, 10));
    }

    /// The middle of `id`'s rect.
    fn centre(app: &App, id: impl Into<NodeId>) -> Point {
        let r = rect(app, id);
        Point::new(r.x() + r.width() / 2.0, r.y() + r.height() / 2.0)
    }

    fn grip_centre(app: &App, h: Handle<BottomSheet>) -> Point {
        centre(app, sheet(app, h).grip())
    }

    /// How many `SheetClosed` the host has seen.
    fn closed(app: &App, host: Handle<Host>) -> usize {
        app.widget::<Host>(host).unwrap().closed
    }

    /// A hosted sheet, laid out.
    fn hosted(b: BottomSheetBuilder) -> (App, NodeId, Handle<Host>, Handle<BottomSheet>) {
        let (mut app, win) = sheet_app();
        let host = app.spawn(win, HostBuilder(b));
        let h = app.widget::<Host>(host).unwrap().sheet;
        settle(&mut app);
        (app, win, host, h)
    }

    // The hosted sheet is its grip, header and a padded 100 px body: 220
    // px tall, so a slow drag must pass 110 px to dismiss it.

    #[test]
    fn the_sheet_slides_with_the_finger_without_resizing() {
        let (mut app, win, _, h) = hosted(bottom_sheet());
        let rest = surface_rect(&app, h);
        let at = grip_centre(&app, h);
        // Crossing the slop (at +20) moves nothing; the next 60 px do.
        press_and_move(
            &mut app,
            win,
            at.x,
            &timed(at, &[at.y + 20.0, at.y + 80.0], 1_000),
        );
        let moved = surface_rect(&app, h);
        assert!(sheet(&app, h).is_dragging());
        assert_eq!(moved.y(), rest.y() + 60.0);
        assert_eq!(moved.height(), rest.height(), "slides, not shrinks");
    }

    #[test]
    fn a_short_drag_down_returns_to_rest() {
        let (mut app, win, host, h) = hosted(bottom_sheet());
        let rest = surface_rect(&app, h);
        let at = grip_centre(&app, h);
        drag(&mut app, win, at, &[at.y + 20.0, at.y + 120.0]);
        assert_eq!(surface_rect(&app, h), rest);
        assert!(!sheet(&app, h).is_closed());
        assert!(!sheet(&app, h).is_dragging());
        assert_eq!(closed(&app, host), 0);
    }

    #[test]
    fn a_drag_past_half_the_height_closes() {
        let (mut app, win, host, h) = hosted(bottom_sheet());
        let at = grip_centre(&app, h);
        drag(&mut app, win, at, &[at.y + 20.0, at.y + 150.0]);
        assert!(sheet(&app, h).is_closed());
        assert_eq!(closed(&app, host), 1);
        assert!(surface_rect(&app, h).y() >= H, "pushed off the bottom edge");
    }

    #[test]
    fn a_flick_down_closes() {
        let (mut app, win, host, h) = hosted(bottom_sheet());
        let at = grip_centre(&app, h);
        flick(&mut app, win, at, &[at.y + 20.0, at.y + 50.0]);
        assert_eq!(closed(&app, host), 1);
    }

    #[test]
    fn dragging_the_header_closes_too() {
        let (mut app, win, host, h) = hosted(bottom_sheet());
        let at = centre(&app, sheet(&app, h).header());
        drag(&mut app, win, at, &[at.y + 20.0, at.y + 150.0]);
        assert_eq!(closed(&app, host), 1);
    }

    #[test]
    fn a_pause_before_lifting_is_not_a_flick() {
        let (mut app, win, host, h) = hosted(bottom_sheet());
        let at = grip_centre(&app, h);
        let down = at.y + 50.0;
        gesture(
            &mut app,
            win,
            at.x,
            &[
                (at.y, 1_000),
                (at.y + 20.0, 1_010),
                (down, 1_020),
                (down, 1_520),
            ],
        );
        assert_eq!(closed(&app, host), 0);
        assert_eq!(sheet(&app, h).offset(), 0.0);
    }

    #[test]
    fn a_cancelled_drag_returns_to_rest() {
        let (mut app, win, host, h) = hosted(bottom_sheet());
        let rest = surface_rect(&app, h);
        let at = grip_centre(&app, h);
        let points = timed(at, &[at.y + 20.0, at.y + 200.0], 10);
        press_and_move(&mut app, win, at.x, &points);
        contact(
            &mut app,
            win,
            ContactPhase::Cancelled,
            at.x,
            at.y + 200.0,
            1_030,
        );
        assert_eq!(surface_rect(&app, h), rest);
        assert_eq!(closed(&app, host), 0);
    }

    #[test]
    fn dragging_up_moves_nothing_and_takes_nothing() {
        let (mut app, win, host, h) = hosted(bottom_sheet().title("Title"));
        let rest = surface_rect(&app, h);
        let at = centre(&app, sheet(&app, h).title());
        drag(&mut app, win, at, &[at.y - 20.0, at.y - 120.0]);
        assert_eq!(surface_rect(&app, h), rest);
        assert!(!app.widget::<Host>(host).unwrap().title_cancelled);
    }

    #[test]
    fn the_sheet_never_rises_above_rest() {
        let (mut app, win, _, h) = hosted(bottom_sheet());
        let rest = surface_rect(&app, h);
        let at = grip_centre(&app, h);
        press_and_move(
            &mut app,
            win,
            at.x,
            &timed(at, &[at.y + 20.0, at.y - 100.0], 1_000),
        );
        assert_eq!(surface_rect(&app, h), rest);
    }

    #[test]
    fn a_move_within_the_slop_changes_nothing() {
        let (mut app, win, host, h) = hosted(bottom_sheet());
        let rest = surface_rect(&app, h);
        let at = grip_centre(&app, h);
        drag(&mut app, win, at, &[at.y + DRAG_SLOP]);
        assert_eq!(surface_rect(&app, h), rest);
        assert_eq!(closed(&app, host), 0);
    }

    #[test]
    fn a_drag_in_the_body_changes_nothing() {
        let (mut app, win, host, h) = hosted(bottom_sheet());
        let rest = surface_rect(&app, h);
        let at = centre(&app, sheet(&app, h).body());
        flick(&mut app, win, at, &[at.y + 20.0, at.y + 150.0]);
        assert_eq!(surface_rect(&app, h), rest);
        assert_eq!(closed(&app, host), 0);
    }

    #[test]
    fn a_sheet_that_is_not_dismissible_does_not_move() {
        let (mut app, win, host, h) = hosted(bottom_sheet().dismissible(false));
        let rest = surface_rect(&app, h);
        let at = grip_centre(&app, h);
        flick(&mut app, win, at, &[at.y + 20.0, at.y + 150.0]);
        assert_eq!(surface_rect(&app, h), rest);
        assert_eq!(closed(&app, host), 0);
    }

    #[test]
    fn a_drag_takes_the_press_from_the_header_content() {
        let (mut app, win, host, h) = hosted(bottom_sheet().title("Title"));
        let at = centre(&app, sheet(&app, h).title());
        drag(&mut app, win, at, &[at.y + 20.0, at.y + 60.0]);
        assert!(app.widget::<Host>(host).unwrap().title_cancelled);
    }

    #[test]
    fn a_tap_on_the_header_content_is_left_alone() {
        let (mut app, win, host, h) = hosted(bottom_sheet().title("Title"));
        let at = centre(&app, sheet(&app, h).title());
        drag(&mut app, win, at, &[at.y]);
        assert!(!app.widget::<Host>(host).unwrap().title_cancelled);
    }

    #[test]
    fn a_closed_sheet_takes_no_drags_until_opened() {
        let (mut app, win, host, h) = hosted(bottom_sheet());
        let rest = surface_rect(&app, h);
        let at = grip_centre(&app, h);
        drag(&mut app, win, at, &[at.y + 20.0, at.y + 150.0]);
        assert!(sheet(&app, h).is_closed());

        // Where the grip used to be: nothing there takes the drag now.
        flick(&mut app, win, at, &[at.y + 20.0, at.y + 150.0]);
        assert_eq!(closed(&app, host), 1);

        with_sheet(&mut app, h, |c| c.open());
        assert!(!sheet(&app, h).is_closed());
        assert_eq!(surface_rect(&app, h), rest);
        drag(&mut app, win, at, &[at.y + 20.0, at.y + 150.0]);
        assert_eq!(closed(&app, host), 2, "draggable again");
    }

    #[test]
    fn close_and_open_from_code_move_the_sheet_silently() {
        let (mut app, win, host, h) = hosted(bottom_sheet());
        let rest = surface_rect(&app, h);
        with_sheet(&mut app, h, |c| c.close());
        assert!(sheet(&app, h).is_closed());
        assert!(surface_rect(&app, h).y() >= H, "pushed off the bottom edge");
        assert_eq!(closed(&app, host), 0, "the owner already knows");

        // Closed: the grip's old spot takes no drag.
        let at = Point::new(rest.x() + rest.width() / 2.0, rest.y() + GRIP_HEIGHT / 2.0);
        flick(&mut app, win, at, &[at.y + 20.0, at.y + 150.0]);
        assert_eq!(closed(&app, host), 0);

        with_sheet(&mut app, h, |c| c.open());
        assert!(!sheet(&app, h).is_closed());
        assert_eq!(surface_rect(&app, h), rest);
        assert_eq!(closed(&app, host), 0);
    }

    #[test]
    fn open_and_close_twice_are_harmless() {
        let (mut app, _, _, h) = hosted(bottom_sheet());
        let rest = surface_rect(&app, h);
        with_sheet(&mut app, h, |c| c.open());
        assert_eq!(surface_rect(&app, h), rest, "already open");
        with_sheet(&mut app, h, |c| {
            c.close();
            c.close();
        });
        assert!(sheet(&app, h).is_closed());
        with_sheet(&mut app, h, |c| {
            c.open();
            c.open();
        });
        assert_eq!(surface_rect(&app, h), rest);
    }

    #[test]
    fn close_works_on_a_sheet_that_is_not_dismissible() {
        let (mut app, _, _, h) = hosted(bottom_sheet().dismissible(false));
        with_sheet(&mut app, h, |c| c.close());
        assert!(sheet(&app, h).is_closed());
    }

    #[test]
    fn close_mid_drag_ends_the_drag_without_an_event() {
        let (mut app, win, host, h) = hosted(bottom_sheet());
        let at = grip_centre(&app, h);
        press_and_move(
            &mut app,
            win,
            at.x,
            &timed(at, &[at.y + 20.0, at.y + 80.0], 1_000),
        );
        with_sheet(&mut app, h, |c| c.close());
        assert!(!sheet(&app, h).is_dragging());
        // The finger lifting afterwards changes nothing more.
        contact(
            &mut app,
            win,
            ContactPhase::Released,
            at.x,
            at.y + 80.0,
            9_000,
        );
        assert!(sheet(&app, h).is_closed());
        assert_eq!(closed(&app, host), 0);
    }

    #[test]
    fn turning_dismissible_off_mid_drag_returns_to_rest() {
        let (mut app, win, _, h) = hosted(bottom_sheet());
        let rest = surface_rect(&app, h);
        let at = grip_centre(&app, h);
        press_and_move(
            &mut app,
            win,
            at.x,
            &timed(at, &[at.y + 20.0, at.y + 80.0], 1_000),
        );
        with_sheet(&mut app, h, |c| c.set_dismissible(false));
        assert_eq!(surface_rect(&app, h), rest);
        assert!(!sheet(&app, h).is_dragging());
    }
}
