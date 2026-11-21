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
// The scroll crate's fling estimator, so a sheet's flick and a scroll
// view's fling read a finger the same way.
use layout::{Align, ComputedLayout, Justify, LayoutStyle, StyleContext, Val, auto, percent, px};
use scroll::VelocityTracker;
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
pub const CLOSE_FRACTION: f32 = 0.5;

/// The downward speed, in logical pixels per second, past which a release
/// is a flick that closes the sheet: Flutter's threshold for the same
/// gesture. The speed is the shared [`VelocityTracker`]'s estimate over
/// the last few moves, so one jittery sample does not decide it; a finger
/// that rests before lifting has none.
pub const FLING_VELOCITY: f32 = 700.0;

/// The user closed the sheet: for a prompt, a cancel. Emitted at the
/// sheet; its owner removes it, or calls `open`. Not emitted for `close`
/// from code.
///
/// Today the only way is dragging it down. The braces are deliberate: an
/// empty braced struct can be `#[non_exhaustive]`, so a field can be added
/// later (which gesture, say) without breaking anyone matching on it.
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

    /// Whether the user can close the sheet, by dragging it down on the
    /// grip or header. On by default; turn it off for a prompt that must be
    /// answered. The owner can still `close` it either way.
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

/// Where the surface sits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Position {
    /// At rest, pinned to the bottom of the parent.
    Rest,
    /// Slid down this many logical pixels by a drag.
    Dragged(f32),
    /// Pushed a whole parent height down: below the bottom edge however
    /// tall the surface is, or later becomes, and before it is ever laid
    /// out.
    Closed,
}

impl Position {
    /// The surface's top inset for this position.
    fn top(self) -> Val {
        match self {
            Self::Rest => px(0.0),
            Self::Dragged(d) => px(d),
            Self::Closed => percent(100.0),
        }
    }
}

/// A drag in progress, from the press that may start it. Not `PartialEq`:
/// the scroll crate's `VelocityTracker` it holds is not.
#[derive(Debug, Clone)]
pub(crate) struct Drag {
    contact: ContactId,
    /// The finger's y the slide is measured from: the press point until
    /// the slop is crossed, the crossing point after.
    origin: f32,
    /// The surface's height when the press landed: what the close fraction
    /// is of.
    height: f32,
    /// Past the slop: the sheet follows the finger and holds the contact.
    active: bool,
    /// The finger's y over time, from the press: the release speed.
    tracker: VelocityTracker,
}

/// Themed Mechanix Design bottom sheet.
#[derive(Debug, Clone)]
pub struct BottomSheet {
    pub(crate) size: SheetSize,
    pub(crate) handle_visible: bool,
    pub(crate) dismissible: bool,
    pub(crate) position: Position,
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
        self.drag.as_ref().is_some_and(|d| d.active)
    }

    /// Closed and waiting for its owner to remove it or open it again.
    #[inline]
    pub fn is_closed(&self) -> bool {
        self.position == Position::Closed
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

/// The surface at its size, moved down from where it rests by `position`.
/// A relative offset moves it without changing the size of anything.
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
                .style(surface_style(b.size, Position::Rest))
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
            position: Position::Rest,
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

/// Past the slop the sheet claims the contact and slides with the
/// finger, never above its resting place.
fn on_press_moved(ctx: &mut Context<'_, BottomSheet>, e: &PressMoved) {
    let y = e.position.y;
    let (claim, origin) = {
        let Some(drag) = ctx.me().drag.as_mut().filter(|d| d.contact == e.contact) else {
            return;
        };
        drag.tracker.add(e.time, f64::from(y));
        let mut claim = false;
        if !drag.active {
            // Only a move down starts a drag: up has nowhere to go, and
            // leaves the press to whatever is under it.
            if y - drag.origin <= DRAG_SLOP {
                return;
            }
            // Measure from the crossing point, so the sheet does not jump
            // by the slop.
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

/// The press is over: `time` is the release's timestamp, `None` for a
/// cancel, which never closes. A drag far or fast enough closes the
/// sheet; any other returns it to rest.
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
    let offset = match ctx.me().position {
        Position::Dragged(d) => d,
        _ => 0.0,
    };
    let velocity = drag.tracker.velocity(time) as f32;
    if should_close(offset, drag.height, velocity) {
        close(ctx);
        ctx.emit(SheetClosed {}, ctx.handle());
    } else {
        set_position(ctx, Position::Rest);
    }
}

/// A release `offset` px down a sheet `height` px tall, moving at
/// `velocity` px/s (positive downward), closes it.
fn should_close(offset: f32, height: f32, velocity: f32) -> bool {
    velocity > FLING_VELOCITY || offset > height * CLOSE_FRACTION
}

/// Push the surface below the bottom edge and stop taking drags.
fn close(ctx: &mut Context<'_, BottomSheet>) {
    ctx.me().drag = None;
    set_position(ctx, Position::Closed);
}

fn set_position(ctx: &mut Context<'_, BottomSheet>, position: Position) {
    ctx.me().position = position;
    apply_size(ctx);
}

/// Write the surface's layout style from the current size and position.
fn apply_size(ctx: &mut Context<'_, BottomSheet>) {
    let (size, position, surface) = {
        let me = ctx.me();
        (me.size, me.position, me.surface)
    };
    if let Some(mut c) = ctx.at(surface) {
        c.set_style(surface_style(size, position));
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
    //! Unit tests for what only this module can see. The widget's
    //! behavior is tested through its public API in
    //! `tests/bottom_sheet.rs`.
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
        assert!(should_close(20.0, 220.0, 1000.0), "flick down");
        assert!(!should_close(20.0, 220.0, FLING_VELOCITY), "not past it");
        assert!(!should_close(20.0, 220.0, -5000.0), "flick up");
    }

    #[test]
    fn closed_is_a_whole_parent_height_down() {
        assert_eq!(Position::Rest.top(), px(0.0));
        assert_eq!(Position::Dragged(42.0).top(), px(42.0));
        assert_eq!(Position::Closed.top(), percent(100.0));
    }
}
