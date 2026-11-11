use crate::color::{ColorSource, IntoColorSource};
use app::{Build, Context, Event, Handle, OnChanged, Spawner, Widget};
use geometry::{Color, Point, Rect};
use interactivity::prelude::{ContactId, ContextClaim, Press, PressCancelled, PressMoved, Release};
use layout::{Align, ComputedLayout, Justify, LayoutStyle, Val, px};
use theme::{ColorRole, SpawnerThemeExt, ThemeReader};
use widgets::{Div, DivContext, div};

/// Orientation of a [`Slider`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SliderOrientation {
    #[default]
    Horizontal,
    Vertical,
}

/// Standard slider sizes (variants) according to Mechanix specifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SliderSize {
    XSmall,
    Small,
    #[default]
    Medium,
    Large,
    XLarge,
}

/// Alias for [`SliderSize`] to match button variant terminology.
pub type SliderVariant = SliderSize;

impl SliderSize {
    #[inline]
    pub fn height(self) -> f32 {
        match self {
            Self::XSmall => 16.0,
            Self::Small => 24.0,
            Self::Medium => 40.0,
            Self::Large => 56.0,
            Self::XLarge => 96.0,
        }
    }

    #[inline]
    pub fn thickness(self) -> f32 {
        2.0
    }

    #[inline]
    pub fn width(self) -> f32 {
        self.thickness()
    }

    #[inline]
    pub fn spacing(self) -> f32 {
        4.0
    }

    #[inline]
    pub fn gap(self) -> f32 {
        self.spacing()
    }

    #[inline]
    pub fn radius(self) -> f32 {
        0.0
    }

    #[inline]
    pub fn tick_size(self, orientation: SliderOrientation) -> (f32, f32) {
        match orientation {
            SliderOrientation::Horizontal => (self.width(), self.height()),
            SliderOrientation::Vertical => (self.height(), self.width()),
        }
    }

    /// [`tick_size`](Self::tick_size) as `px` layout values.
    #[inline]
    pub fn tick_size_val(self, orientation: SliderOrientation) -> (Val, Val) {
        let (w, h) = self.tick_size(orientation);
        (px(w), px(h))
    }
}

/// Default track length.
pub const DEFAULT_LENGTH: Val = px(240.0);

/// How many ticks of `tick` px separated by `gap` px fit in `length` px.
/// `n` ticks need `n * tick + (n - 1) * gap`, so `n = (length + gap) / (tick + gap)`.
/// `0` when not even one tick fits.
fn tick_count(length: f32, tick: f32, gap: f32) -> usize {
    let gap = gap.max(0.0);
    let pitch = tick + gap;
    if !(length.is_finite() && pitch.is_finite()) || pitch <= 0.0 || length < tick {
        return 0;
    }
    // Tolerate float error so an exact fit isn't lost.
    (((length + gap) / pitch) + 1e-4).floor() as usize
}

/// Length covered by `n` ticks: `n * tick + (n - 1) * gap`.
fn tick_span(n: usize, tick: f32, gap: f32) -> f32 {
    if n == 0 {
        0.0
    } else {
        n as f32 * tick + (n - 1) as f32 * gap.max(0.0)
    }
}

/// Resolves a tick dimension to px. `percent` is of `parent` (the slider's
/// content box on that axis); `auto` falls back to `default`.
fn resolve_val(v: Val, parent: f32, default: f32) -> f32 {
    let r = match v {
        Val::Px(p) => p,
        Val::Percent(p) => parent * p / 100.0,
        Val::Auto => default,
    };
    if r.is_finite() { r.max(0.0) } else { 0.0 }
}

/// Size and spacing of the tick divs. Width and height are `px` or
/// `percent` of the slider's content box; `auto` uses the size variant.
#[derive(Debug, Clone, Copy, PartialEq)]
struct TickSpec {
    width: Val,
    height: Val,
    gap: f32,
    radius: f32,
    size: SliderSize,
}

impl TickSpec {
    /// Number of ticks that fit in `length` px along the main axis.
    fn count_in(self, length: f32, orientation: SliderOrientation) -> usize {
        tick_count(length, self.extent(orientation, length), self.gap)
    }

    /// Main-axis size of one tick in px, for a track `length` px long.
    ///
    /// Snapped to whole pixels: layout rounds each tick's position, so a
    /// fractional size (e.g. 1% of 384 px = 3.84 px) would land each tick
    /// on the pixel grid differently and draw some 3 px and some 4 px wide.
    /// A visible tick never snaps below 1 px.
    fn extent(self, orientation: SliderOrientation, length: f32) -> f32 {
        let (main, default) = match orientation {
            SliderOrientation::Horizontal => (self.width, self.size.width()),
            SliderOrientation::Vertical => (self.height, self.size.width()),
        };
        let r = resolve_val(main, length, default);
        if r > 0.0 { r.round().max(1.0) } else { 0.0 }
    }

    /// Cross-axis size of one tick. Left to layout so `percent` tracks the
    /// slider's cross size; `auto` uses the size variant.
    fn cross(self, orientation: SliderOrientation) -> Val {
        let cross = match orientation {
            SliderOrientation::Horizontal => self.height,
            SliderOrientation::Vertical => self.width,
        };
        match cross {
            Val::Auto => px(self.size.height()),
            v => v,
        }
    }
}

/// Emitted when a press or drag changes the value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderChange {
    pub value: f32,
}

impl Event for SliderChange {}

pub fn slider() -> SliderBuilder {
    SliderBuilder::new()
}

/// Builder for a [`Slider`].
#[derive(Debug, Clone)]
pub struct SliderBuilder {
    pub size: SliderSize,
    pub value: f32,
    pub min: f32,
    pub max: f32,
    /// Track length along the main axis: width when horizontal, height when vertical.
    /// `px` or `percent` (of the parent); `auto` leaves the axis to [`style`](Self::style).
    pub length: Val,
    /// Overrides below are `None` until set; unset ones come from
    /// [`size`](Self::size) and [`orientation`](Self::orientation) at build
    /// time, so the builder calls can go in any order.
    pub tick_gap: Option<f32>,
    pub tick_radius: Option<f32>,
    /// On-screen tick width: `px`, or `percent` of the slider's content width.
    pub tick_width: Option<Val>,
    /// On-screen tick height: `px`, or `percent` of the slider's content height.
    pub tick_height: Option<Val>,
    pub active_color: Option<ColorSource>,
    pub inactive_color: Option<ColorSource>,
    pub style: Option<LayoutStyle>,
    pub orientation: SliderOrientation,
}

impl Default for SliderBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SliderBuilder {
    pub fn new() -> Self {
        Self {
            size: SliderSize::Medium,
            value: 0.0,
            min: 0.0,
            max: 1.0,
            length: DEFAULT_LENGTH,
            tick_gap: None,
            tick_radius: None,
            tick_width: None,
            tick_height: None,
            active_color: None,
            inactive_color: None,
            style: None,
            orientation: SliderOrientation::Horizontal,
        }
    }

    /// Sets the size variant: the default tick size, gap and radius. Tick
    /// values set explicitly win, whichever order the calls are made in.
    pub fn size(mut self, size: SliderSize) -> Self {
        self.size = size;
        self
    }

    /// Alias for [`size`](Self::size).
    pub fn variant(self, variant: SliderSize) -> Self {
        self.size(variant)
    }

    /// Lays the track out left to right. The default tick size is thin
    /// along the track and the variant height across it; an explicit
    /// [`tick_width`](Self::tick_width) / [`tick_height`](Self::tick_height)
    /// is kept as given.
    pub fn horizontal(self) -> Self {
        self.orientation(SliderOrientation::Horizontal)
    }

    /// Lays the track out bottom to top. See [`horizontal`](Self::horizontal).
    pub fn vertical(self) -> Self {
        self.orientation(SliderOrientation::Vertical)
    }

    pub fn orientation(mut self, orientation: SliderOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    pub fn value(mut self, value: f32) -> Self {
        self.value = value;
        self
    }

    pub fn min(mut self, min: f32) -> Self {
        self.min = min;
        self
    }

    pub fn max(mut self, max: f32) -> Self {
        self.max = max;
        self
    }

    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.min = min;
        self.max = max;
        self
    }

    /// Sets the track length along the main axis (width when horizontal,
    /// height when vertical), e.g. `px(240.0)` or `percent(80.0)`. As many
    /// ticks as fit are laid out, recounted whenever the laid-out length
    /// changes; space too small for another tick is left empty at the end.
    pub fn length(mut self, length: Val) -> Self {
        self.length = length;
        self
    }

    /// The tick settings in effect: each override, else the size variant's
    /// default for the orientation.
    fn tick_spec(&self) -> TickSpec {
        let (w, h) = self.size.tick_size_val(self.orientation);
        TickSpec {
            width: self.tick_width.unwrap_or(w),
            height: self.tick_height.unwrap_or(h),
            gap: self.tick_gap.unwrap_or(self.size.gap()),
            radius: self.tick_radius.unwrap_or(self.size.radius()),
            size: self.size,
        }
    }

    /// Number of ticks known before layout: `Some` for a `px` length,
    /// `None` when it depends on layout (`percent` / `auto`).
    pub fn tick_count(&self) -> Option<usize> {
        match self.length {
            Val::Px(length) => Some(self.tick_spec().count_in(length, self.orientation)),
            _ => None,
        }
    }

    pub fn tick_gap(mut self, gap: f32) -> Self {
        self.tick_gap = Some(gap);
        self
    }

    pub fn tick_radius(mut self, radius: f32) -> Self {
        self.tick_radius = Some(radius);
        self
    }

    /// Tick width, e.g. `px(2.0)` or `percent(1.0)` (of the slider's content
    /// width). Re-resolved whenever the slider's layout changes.
    pub fn tick_width(mut self, width: Val) -> Self {
        self.tick_width = Some(width);
        self
    }

    /// Tick height, e.g. `px(40.0)` or `percent(100.0)` (of the slider's
    /// content height). Re-resolved whenever the slider's layout changes.
    ///
    /// A cross-axis `percent` needs the slider to have a definite cross size
    /// (set it via [`style`](Self::style)); otherwise it resolves to `0`.
    pub fn tick_height(mut self, height: Val) -> Self {
        self.tick_height = Some(height);
        self
    }

    /// Sets [`tick_width`](Self::tick_width) and [`tick_height`](Self::tick_height).
    pub fn tick_size(mut self, width: Val, height: Val) -> Self {
        self.tick_width = Some(width);
        self.tick_height = Some(height);
        self
    }

    pub fn active_color(mut self, color: impl IntoColorSource) -> Self {
        self.active_color = color.into_color_source();
        self
    }

    pub fn inactive_color(mut self, color: impl IntoColorSource) -> Self {
        self.inactive_color = color.into_color_source();
        self
    }

    pub fn style(mut self, style: LayoutStyle) -> Self {
        self.style = Some(style);
        self
    }
}

/// A spawned slider. Its state is read through the getters here and
/// changed through [`SliderContextExt`], which keeps the ticks in sync.
pub struct Slider {
    size: SliderSize,
    value: f32,
    min: f32,
    max: f32,
    /// Derived from the laid-out track length; see [`tick_count`](Self::tick_count).
    tick_count: usize,
    active_color: Option<ColorSource>,
    inactive_color: Option<ColorSource>,
    orientation: SliderOrientation,
    tick: TickSpec,
    /// Main-axis size of one tick in px, resolved against the laid-out track.
    extent: f32,
    pub(crate) ticks: Vec<Handle<Div>>,
    /// The contact currently dragging the slider, if any.
    pub(crate) dragging: Option<ContactId>,
}

impl Slider {
    /// Whether a contact is currently dragging the slider.
    #[inline]
    pub fn is_dragging(&self) -> bool {
        self.dragging.is_some()
    }

    /// Size variant of the slider.
    #[inline]
    pub fn size(&self) -> SliderSize {
        self.size
    }

    /// Current value, within `[min, max]`.
    #[inline]
    pub fn value(&self) -> f32 {
        self.value
    }

    #[inline]
    pub fn min(&self) -> f32 {
        self.min
    }

    #[inline]
    pub fn max(&self) -> f32 {
        self.max
    }

    #[inline]
    pub fn orientation(&self) -> SliderOrientation {
        self.orientation
    }

    /// Number of ticks, derived from the laid-out track length. `0` before
    /// the first layout of a `percent` / `auto` length, or when not even
    /// one tick fits.
    #[inline]
    pub fn tick_count(&self) -> usize {
        self.tick_count
    }

    /// The active-color override, `None` for the theme default.
    #[inline]
    pub fn active_color(&self) -> Option<ColorSource> {
        self.active_color
    }

    /// The inactive-color override, `None` for the theme default.
    #[inline]
    pub fn inactive_color(&self) -> Option<ColorSource> {
        self.inactive_color
    }

    /// Position of `value` within `[min, max]`, in `0.0..=1.0`.
    #[inline]
    pub fn fraction(&self) -> f32 {
        self.fraction_for(self.value)
    }

    fn fraction_for(&self, value: f32) -> f32 {
        let range = self.max - self.min;
        if range > 0.0 && range.is_finite() && value.is_finite() {
            ((value - self.min) / range).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Number of lit ticks for the current value, in `0..=tick_count`.
    ///
    /// The value's position along the ticks is `fraction * span`; a tick is
    /// lit once that position reaches its centre, so the lit ticks follow
    /// the pointer exactly. `min` lights none, `max` lights all.
    #[inline]
    pub fn lit_ticks(&self) -> usize {
        let n = self.tick_count;
        let extent = self.extent;
        let pitch = extent + self.tick.gap.max(0.0);
        if n == 0 || pitch <= 0.0 {
            return 0;
        }
        let pos = self.fraction() * tick_span(n, extent, self.tick.gap);
        let past_first_centre = pos + 1e-3 - extent / 2.0;
        if past_first_centre < 0.0 {
            return 0;
        }
        ((past_first_centre / pitch).floor() as usize + 1).min(n)
    }

    /// Fraction (`0.0..=1.0`) of the range under `pos`, measured over the
    /// ticks themselves (not the empty remainder at the end), from the left
    /// when horizontal or the bottom when vertical. `None` when there are no
    /// ticks to measure against.
    fn fraction_at(&self, content: Rect, pos: Point) -> Option<f32> {
        let span = tick_span(self.tick_count, self.extent, self.tick.gap);
        if span <= 0.0 || !span.is_finite() {
            return None;
        }
        let offset = match self.orientation {
            SliderOrientation::Horizontal => pos.x - content.x(),
            SliderOrientation::Vertical => content.bottom() - pos.y,
        };
        Some((offset / span).clamp(0.0, 1.0))
    }

    /// Clamps to `[min, max]` without panicking when `min > max`.
    #[inline]
    fn clamp(&self, value: f32) -> f32 {
        if value.is_finite() {
            value.max(self.min).min(self.max)
        } else {
            self.min.min(self.max)
        }
    }

    /// Value at `fraction` (`0.0..=1.0`) of the range, keeping the endpoints exact.
    fn value_at_fraction(&self, fraction: f32) -> f32 {
        if fraction <= 0.0 {
            self.min
        } else if fraction >= 1.0 {
            self.max
        } else {
            self.min + (self.max - self.min) * fraction
        }
    }

    /// Sets the value (clamped); returns whether it changed.
    fn set(&mut self, value: f32) -> bool {
        let value = self.clamp(value);
        if self.value == value {
            return false;
        }
        self.value = value;
        true
    }
}

/// Resolves `(active, inactive)` tick colors, falling back to theme defaults.
fn tick_colors(
    active: Option<ColorSource>,
    inactive: Option<ColorSource>,
    reader: &impl ThemeReader,
) -> (Color, Color) {
    let resolve =
        |c: Option<ColorSource>, role: ColorRole| c.unwrap_or(role.into()).resolve(reader);
    (
        resolve(active, ColorRole::OnSecondaryFixed),
        resolve(inactive, ColorRole::SecondaryFixedDim),
    )
}

impl Build for SliderBuilder {
    type Widget = Slider;
}

impl Widget for Slider {
    type Builder = SliderBuilder;

    fn build(b: SliderBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        let tick_count = b.tick_count().unwrap_or(0);
        let tick = b.tick_spec();
        // Known up front only for a `px` length; otherwise set by `fit_ticks`.
        let extent = match b.length {
            Val::Px(length) => tick.extent(b.orientation, length),
            _ => 0.0,
        };
        // Ticks pack from the start (left / bottom); leftover space stays at the end.
        let mut style = b
            .style
            .unwrap_or_default()
            .justify(Justify::Start)
            .align_items(Align::Center);
        match b.orientation {
            SliderOrientation::Horizontal => {
                style = style.row().column_gap(px(tick.gap)).min_width(px(0.0));
                if b.length != Val::Auto {
                    style = style.width(b.length);
                }
            }
            SliderOrientation::Vertical => {
                style = style
                    .column_reverse()
                    .row_gap(px(tick.gap))
                    .min_height(px(0.0));
                if b.length != Val::Auto {
                    style = style.height(b.length);
                }
            }
        }
        *s.component_mut::<LayoutStyle>(me).unwrap() = style;

        let mut slider = Slider {
            size: b.size,
            value: b.value,
            min: b.min,
            max: b.max,
            tick_count,
            active_color: b.active_color,
            inactive_color: b.inactive_color,
            orientation: b.orientation,
            tick,
            extent,
            ticks: Vec::new(),
            dragging: None,
        };
        slider.value = slider.clamp(slider.value);

        let (active, inactive) = tick_colors(b.active_color, b.inactive_color, s);
        let lit = slider.lit_ticks();
        slider.ticks = (0..slider.tick_count)
            .map(|i| {
                let color = if i < lit { active } else { inactive };
                s.spawn(me, tick_div(tick, b.orientation, extent, color))
            })
            .collect();

        s.on::<OnChanged<ComputedLayout>>(me, |ctx, _| fit_ticks(ctx));
        s.on::<Press>(me, on_press);
        s.on::<PressMoved>(me, on_press_moved);
        s.on::<Release>(me, |ctx, e: &Release| end_drag(ctx, e.contact));
        s.on::<PressCancelled>(me, |ctx, e: &PressCancelled| end_drag(ctx, e.contact));
        s.on_theme(me, sync_ticks);

        slider
    }
}

/// Layout of one tick: the main axis in resolved px (so the count always
/// fits the track exactly), the cross axis left to layout.
fn tick_style(tick: TickSpec, orientation: SliderOrientation, extent: f32) -> LayoutStyle {
    let (w, h) = match orientation {
        SliderOrientation::Horizontal => (px(extent), tick.cross(orientation)),
        SliderOrientation::Vertical => (tick.cross(orientation), px(extent)),
    };
    LayoutStyle::default().width(w).height(h).shrink(0.0)
}

fn tick_div(
    tick: TickSpec,
    orientation: SliderOrientation,
    extent: f32,
    color: Color,
) -> widgets::DivBuilder {
    div()
        .style(tick_style(tick, orientation, extent))
        .radius(tick.radius)
        .background(color)
}

/// Recounts the ticks for the laid-out track length, spawning or removing
/// tick divs to match. Leftover space too small for a tick stays empty.
fn fit_ticks(ctx: &mut Context<'_, Slider>) {
    let Some(content) = ctx.component::<ComputedLayout>().map(|l| l.content()) else {
        return;
    };
    let (tick, orientation) = {
        let me = ctx.me();
        (me.tick, me.orientation)
    };
    let length = match orientation {
        SliderOrientation::Horizontal => content.width(),
        SliderOrientation::Vertical => content.height(),
    };
    let extent = tick.extent(orientation, length);
    let want = tick_count(length, extent, tick.gap);

    // A `percent` tick resizes with the track: restyle the existing ticks.
    let resized = ctx.me().extent != extent;
    if resized {
        ctx.me().extent = extent;
        let style = tick_style(tick, orientation, extent);
        let ticks = ctx.me().ticks.clone();
        for t in ticks {
            if let Some(mut c) = ctx.at(t)
                && let Some(mut s) = c.component_mut::<LayoutStyle>()
            {
                *s = style.clone();
            }
        }
    }

    let have = ctx.me().ticks.len();
    if want == have {
        if resized {
            // Same count, but the lit ticks depend on the tick extent.
            sync_ticks(ctx);
        }
        return;
    }

    if want < have {
        let extra = ctx.me().ticks.split_off(want);
        for t in extra {
            ctx.remove(t);
        }
    } else {
        let me = ctx.handle();
        // Colored by `sync_ticks` below.
        let placeholder = Color::TRANSPARENT;
        for _ in have..want {
            let t = ctx.spawn(me, tick_div(tick, orientation, extent, placeholder));
            ctx.me().ticks.push(t);
        }
    }
    ctx.me().tick_count = want;
    sync_ticks(ctx);
}

/// Starts a drag: the slider claims the contact so an enclosing scroll view
/// can't take it over, then jumps to the pressed position.
fn on_press(ctx: &mut Context<'_, Slider>, e: &Press) {
    if ctx.me().dragging.is_some() {
        // Another contact already drives the slider.
        return;
    }
    ctx.me().dragging = Some(e.contact);
    ctx.claim_contact(e.contact);
    set_from_pointer(ctx, e.position);
}

/// Follows the dragging contact, even outside the slider's bounds.
fn on_press_moved(ctx: &mut Context<'_, Slider>, e: &PressMoved) {
    if ctx.me().dragging == Some(e.contact) {
        set_from_pointer(ctx, e.position);
    }
}

fn end_drag(ctx: &mut Context<'_, Slider>, contact: ContactId) {
    let me = ctx.me();
    if me.dragging == Some(contact) {
        me.dragging = None;
    }
}

/// Sets the value from the pointer's position along the track and emits on change.
fn set_from_pointer(ctx: &mut Context<'_, Slider>, pos: Point) {
    let range = ctx.me().max - ctx.me().min;
    if !range.is_finite() || range <= 0.0 {
        return;
    }
    let Some(content) = ctx.component::<ComputedLayout>().map(|l| l.content()) else {
        return;
    };
    let me = ctx.me();
    let Some(fraction) = me.fraction_at(content, pos) else {
        return;
    };
    let value = me.value_at_fraction(fraction);
    if !me.set(value) {
        return;
    }
    let value = me.value;

    sync_ticks(ctx);
    let handle = ctx.handle();
    ctx.emit(SliderChange { value }, handle);
}

fn sync_ticks(ctx: &mut Context<'_, Slider>) {
    let (lit, count) = {
        let me = ctx.me();
        (me.lit_ticks(), me.ticks.len())
    };
    let (active, inactive) = {
        let me = ctx.me();
        tick_colors(me.active_color, me.inactive_color, ctx)
    };

    for i in 0..count {
        let tick = ctx.me().ticks[i];
        if let Some(mut c) = ctx.at(tick) {
            c.set_background(if i < lit { active } else { inactive });
        }
    }
}

/// Extension trait for reading and updating a spawned [`Slider`].
pub trait SliderContextExt {
    fn size(&mut self) -> SliderSize;
    fn value(&mut self) -> f32;
    fn fraction(&mut self) -> f32;
    fn lit_ticks(&mut self) -> usize;
    fn orientation(&mut self) -> SliderOrientation;
    /// Clamps to `[min, max]` without emitting an event.
    fn set_value(&mut self, value: f32);
    fn set_range(&mut self, min: f32, max: f32);
    fn set_active_color(&mut self, color: impl IntoColorSource);
    fn set_inactive_color(&mut self, color: impl IntoColorSource);
    fn sync_visuals(&mut self);
}

impl SliderContextExt for Context<'_, Slider> {
    fn size(&mut self) -> SliderSize {
        self.me().size
    }

    fn value(&mut self) -> f32 {
        self.me().value
    }

    fn fraction(&mut self) -> f32 {
        self.me().fraction()
    }

    fn lit_ticks(&mut self) -> usize {
        self.me().lit_ticks()
    }

    fn orientation(&mut self) -> SliderOrientation {
        self.me().orientation
    }

    fn set_value(&mut self, value: f32) {
        if self.me().set(value) {
            sync_ticks(self);
        }
    }

    fn set_range(&mut self, min: f32, max: f32) {
        let me = self.me();
        me.min = min;
        me.max = max;
        me.value = me.clamp(me.value);
        sync_ticks(self);
    }

    fn set_active_color(&mut self, color: impl IntoColorSource) {
        self.me().active_color = color.into_color_source();
        sync_ticks(self);
    }

    fn set_inactive_color(&mut self, color: impl IntoColorSource) {
        self.me().inactive_color = color.into_color_source();
        sync_ticks(self);
    }

    fn sync_visuals(&mut self) {
        sync_ticks(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slider_at(value: f32, min: f32, max: f32, tick_count: usize) -> Slider {
        Slider {
            size: SliderSize::Medium,
            value,
            min,
            max,
            tick_count,
            active_color: None,
            inactive_color: None,
            orientation: SliderOrientation::Horizontal,
            tick: slider().tick_spec(),
            extent: 2.0,
            ticks: vec![],
            dragging: None,
        }
    }

    #[test]
    fn slider_fraction_and_lit_ticks() {
        let mut s = slider_at(20.0, 0.0, 100.0, 50);
        assert_eq!(s.fraction(), 0.2);
        assert_eq!(s.lit_ticks(), 10);

        s.value = 50.0;
        assert_eq!(s.fraction(), 0.5);
        assert_eq!(s.lit_ticks(), 25);

        s.value = 100.0;
        assert_eq!(s.fraction(), 1.0);
        assert_eq!(s.lit_ticks(), 50);
    }

    #[test]
    fn tick_count_fills_length_and_leaves_remainder_empty() {
        // 2 px ticks, 4 px gap: n ticks need 6n - 4 px.
        assert_eq!(tick_count(236.0, 2.0, 4.0), 40); // exact fit
        assert_eq!(tick_count(240.0, 2.0, 4.0), 40); // 4 px left empty
        assert_eq!(tick_count(241.0, 2.0, 4.0), 40); // 5 px: still no room
        assert_eq!(tick_count(242.0, 2.0, 4.0), 41); // room for one more
        assert_eq!(tick_count(2.0, 2.0, 4.0), 1); // exactly one tick
        assert_eq!(tick_count(1.0, 2.0, 4.0), 0); // not even one fits
        assert_eq!(tick_count(0.0, 2.0, 4.0), 0);
        assert_eq!(tick_count(f32::NAN, 2.0, 4.0), 0);
        assert_eq!(tick_count(100.0, 0.0, 0.0), 0); // zero pitch: no infinite ticks

        assert_eq!(tick_span(0, 2.0, 4.0), 0.0);
        assert_eq!(tick_span(1, 2.0, 4.0), 2.0);
        assert_eq!(tick_span(40, 2.0, 4.0), 236.0);

        // Vertical uses the tick height as the main-axis extent.
        let v = slider().vertical().length(px(100.0));
        assert_eq!(v.tick_count(), Some(17));
    }

    #[test]
    fn pointer_maps_over_the_ticks_not_the_empty_remainder() {
        // 40 ticks of 2 px + 4 px gaps span 236 px of a 240 px track.
        let mut s = slider_at(0.0, 0.0, 100.0, 40);
        let track = Rect::new(10.0, 0.0, 240.0, 40.0);

        assert_eq!(s.fraction_at(track, Point::new(10.0, 0.0)), Some(0.0));
        assert_eq!(s.fraction_at(track, Point::new(128.0, 0.0)), Some(0.5));
        // The far edge of the last tick is already the maximum...
        assert_eq!(s.fraction_at(track, Point::new(246.0, 0.0)), Some(1.0));
        // ...and so is the empty remainder past it.
        assert_eq!(s.fraction_at(track, Point::new(249.0, 0.0)), Some(1.0));
        assert_eq!(s.fraction_at(track, Point::new(0.0, 0.0)), Some(0.0));

        // Pressing the middle of tick `i` lights ticks `0..=i`.
        for i in [0usize, 7, 39] {
            let center = 10.0 + i as f32 * 6.0 + 1.0;
            let f = s.fraction_at(track, Point::new(center, 0.0)).unwrap();
            s.value = s.value_at_fraction(f);
            assert_eq!(s.lit_ticks(), i + 1, "tick {i}");
        }

        // Vertical measures up from the bottom.
        s.orientation = SliderOrientation::Vertical;
        s.tick = slider().vertical().tick_spec();
        let track = Rect::new(0.0, 0.0, 40.0, 240.0);
        assert_eq!(s.fraction_at(track, Point::new(0.0, 240.0)), Some(0.0));
        assert_eq!(s.fraction_at(track, Point::new(0.0, 122.0)), Some(0.5));
        assert_eq!(s.fraction_at(track, Point::new(0.0, 4.0)), Some(1.0));

        // No ticks yet (before the first layout): nothing to map against.
        s.tick_count = 0;
        assert_eq!(s.fraction_at(track, Point::new(0.0, 100.0)), None);
    }

    #[test]
    fn slider_tolerates_inverted_range() {
        let s = slider_at(0.0, 10.0, 0.0, 1);
        assert_eq!(s.clamp(5.0), 0.0);
        assert_eq!(s.fraction(), 0.0);
    }

    #[test]
    fn slider_value_is_continuous() {
        let mut s = slider_at(0.0, 0.0, 100.0, 4);
        // No snapping: values between ticks are kept as-is.
        assert!(s.set(38.0));
        assert_eq!(s.value, 38.0);
        assert!(!s.set(38.0));
        assert_eq!(s.lit_ticks(), 2); // 1.52 ticks rounds to 2

        assert!(s.set(-10.0));
        assert_eq!(s.value, 0.0);
        assert!(s.set(200.0));
        assert_eq!(s.value, 100.0);
        assert!(s.set(f32::NAN));
        assert_eq!(s.value, 0.0);

        assert_eq!(s.value_at_fraction(0.0), 0.0);
        assert_eq!(s.value_at_fraction(0.37), 37.0);
        assert_eq!(s.value_at_fraction(1.0), 100.0);
    }

    #[test]
    fn slider_size_properties() {
        // XSmall: — × 16 px
        assert_eq!(SliderSize::XSmall.height(), 16.0);
        assert_eq!(SliderSize::XSmall.width(), 2.0);
        assert_eq!(SliderSize::XSmall.thickness(), 2.0);
        assert_eq!(SliderSize::XSmall.gap(), 4.0);
        assert_eq!(SliderSize::XSmall.spacing(), 4.0);
        assert_eq!(SliderSize::XSmall.radius(), 0.0);

        // Small: — × 24 px
        assert_eq!(SliderSize::Small.height(), 24.0);
        assert_eq!(SliderSize::Small.width(), 2.0);
        assert_eq!(SliderSize::Small.thickness(), 2.0);
        assert_eq!(SliderSize::Small.gap(), 4.0);
        assert_eq!(SliderSize::Small.spacing(), 4.0);
        assert_eq!(SliderSize::Small.radius(), 0.0);

        // Medium (default): — × 40 px
        assert_eq!(SliderSize::Medium.height(), 40.0);
        assert_eq!(SliderSize::Medium.width(), 2.0);
        assert_eq!(SliderSize::Medium.thickness(), 2.0);
        assert_eq!(SliderSize::Medium.gap(), 4.0);
        assert_eq!(SliderSize::Medium.spacing(), 4.0);
        assert_eq!(SliderSize::Medium.radius(), 0.0);

        // Large: — × 56 px
        assert_eq!(SliderSize::Large.height(), 56.0);
        assert_eq!(SliderSize::Large.width(), 2.0);
        assert_eq!(SliderSize::Large.thickness(), 2.0);
        assert_eq!(SliderSize::Large.gap(), 4.0);
        assert_eq!(SliderSize::Large.spacing(), 4.0);
        assert_eq!(SliderSize::Large.radius(), 0.0);

        // XLarge: — × 96 px
        assert_eq!(SliderSize::XLarge.height(), 96.0);
        assert_eq!(SliderSize::XLarge.width(), 2.0);
        assert_eq!(SliderSize::XLarge.thickness(), 2.0);
        assert_eq!(SliderSize::XLarge.gap(), 4.0);
        assert_eq!(SliderSize::XLarge.spacing(), 4.0);
        assert_eq!(SliderSize::XLarge.radius(), 0.0);
    }

    #[test]
    fn slider_builder() {
        let b = slider().range(0.0, 100.0).value(45.0).length(px(236.0));

        assert_eq!(b.min, 0.0);
        assert_eq!(b.max, 100.0);
        assert_eq!(b.value, 45.0);
        assert_eq!(b.length, px(236.0));
        assert_eq!(b.tick_count(), Some(40));
        // Percent lengths are counted after layout.
        assert_eq!(slider().length(layout::percent(50.0)).tick_count(), None);
        assert_eq!(b.size, SliderSize::Medium);
        assert_eq!(b.tick_spec().width, px(2.0));
        assert_eq!(b.tick_spec().height, px(40.0));
        assert_eq!(b.tick_spec().gap, 4.0);
        assert_eq!(b.orientation, SliderOrientation::Horizontal);

        let bv = slider().vertical();
        assert_eq!(bv.orientation, SliderOrientation::Vertical);
        assert_eq!(bv.tick_spec().width, px(40.0));
        assert_eq!(bv.tick_spec().height, px(2.0));

        let bh = slider().vertical().horizontal();
        assert_eq!(bh.orientation, SliderOrientation::Horizontal);
        assert_eq!(bh.tick_spec().width, px(2.0));
        assert_eq!(bh.tick_spec().height, px(40.0));

        let custom = slider().tick_size(px(4.0), px(32.0));
        assert_eq!(custom.tick_spec().width, px(4.0));
        assert_eq!(custom.tick_spec().height, px(32.0));

        // Percent tick width resolves against a px length up front:
        // 1% of 300 = 3 px ticks, 4 px gap -> (300 + 4) / 7 = 43 ticks.
        let pct = slider().length(px(300.0)).tick_width(layout::percent(1.0));
        assert_eq!(pct.tick_count(), Some(43));

        // Fractional tick sizes snap to whole pixels so every tick draws
        // the same width: 1% of 384 = 3.84 -> 4 px; 0.1% of 300 = 0.3 -> 1 px.
        let spec = slider().tick_width(layout::percent(1.0)).tick_spec();
        assert_eq!(spec.extent(SliderOrientation::Horizontal, 384.0), 4.0);
        let spec = slider().tick_width(layout::percent(0.1)).tick_spec();
        assert_eq!(spec.extent(SliderOrientation::Horizontal, 300.0), 1.0);
        let spec = slider().tick_width(px(0.0)).tick_spec();
        assert_eq!(spec.extent(SliderOrientation::Horizontal, 300.0), 0.0);
    }

    #[test]
    fn slider_size_and_variants() {
        let xs = slider().size(SliderSize::XSmall);
        assert_eq!(xs.size, SliderSize::XSmall);
        assert_eq!(xs.tick_spec().height, px(16.0));
        assert_eq!(xs.tick_spec().width, px(2.0));
        assert_eq!(xs.tick_spec().gap, 4.0);

        let sm = slider().size(SliderSize::Small);
        assert_eq!(sm.size, SliderSize::Small);
        assert_eq!(sm.tick_spec().height, px(24.0));
        assert_eq!(sm.tick_spec().width, px(2.0));
        assert_eq!(sm.tick_spec().gap, 4.0);

        let md = slider().size(SliderSize::Medium);
        assert_eq!(md.size, SliderSize::Medium);
        assert_eq!(md.tick_spec().height, px(40.0));
        assert_eq!(md.tick_spec().width, px(2.0));
        assert_eq!(md.tick_spec().gap, 4.0);

        let lg = slider().size(SliderSize::Large);
        assert_eq!(lg.size, SliderSize::Large);
        assert_eq!(lg.tick_spec().height, px(56.0));
        assert_eq!(lg.tick_spec().width, px(2.0));
        assert_eq!(lg.tick_spec().gap, 4.0);

        let xl = slider().size(SliderSize::XLarge);
        assert_eq!(xl.size, SliderSize::XLarge);
        assert_eq!(xl.tick_spec().height, px(96.0));
        assert_eq!(xl.tick_spec().width, px(2.0));
        assert_eq!(xl.tick_spec().gap, 4.0);

        // variant method alias
        let v = slider().variant(SliderVariant::Large);
        assert_eq!(v.size, SliderSize::Large);

        // vertical orientation with specific size: swap width and height
        let v_lg = slider().size(SliderSize::Large).vertical();
        assert_eq!(v_lg.orientation, SliderOrientation::Vertical);
        assert_eq!(v_lg.tick_spec().width, px(56.0));
        assert_eq!(v_lg.tick_spec().height, px(2.0));

        // setting size after setting vertical orientation: swap width and height
        let v_lg2 = slider().vertical().size(SliderSize::Large);
        assert_eq!(v_lg2.orientation, SliderOrientation::Vertical);
        assert_eq!(v_lg2.tick_spec().width, px(56.0));
        assert_eq!(v_lg2.tick_spec().height, px(2.0));
    }

    #[test]
    fn builder_overrides_are_order_independent() {
        // An explicit tick size survives a later `size` or orientation change.
        let a = slider()
            .tick_width(px(4.0))
            .tick_gap(8.0)
            .size(SliderSize::Large)
            .vertical();
        let b = slider()
            .vertical()
            .size(SliderSize::Large)
            .tick_gap(8.0)
            .tick_width(px(4.0));
        assert_eq!(a.tick_spec(), b.tick_spec());
        // Width as given; height is the vertical default (thin along the track).
        assert_eq!((a.tick_spec().width, a.tick_spec().height), (px(4.0), px(2.0)));
        assert_eq!(a.tick_spec().gap, 8.0);
        assert_eq!(b.tick_spec().gap, 8.0);

        let r = slider().tick_radius(3.0).size(SliderSize::XSmall);
        assert_eq!(r.tick_spec().radius, 3.0);
        assert_eq!((r.tick_spec().width, r.tick_spec().height), (px(2.0), px(16.0)));

        // Flipping orientation twice is a no-op for overrides.
        let f = slider().tick_height(px(30.0)).vertical().horizontal();
        assert_eq!((f.tick_spec().width, f.tick_spec().height), (px(2.0), px(30.0)));
    }

    #[test]
    fn spawned_slider_exposes_state_through_getters() {
        use app::App;
        use theme::MechanixTheme;

        let mut app = App::new();
        app.add_module(layout::LayoutModule)
            .add_module(paint::PaintModule)
            .add_module(MechanixTheme::dark());
        let s = app.spawn(
            app.root(),
            slider()
                .range(10.0, 20.0)
                .value(25.0)
                .length(px(236.0))
                .vertical(),
        );
        let me = app.widget::<Slider>(s).unwrap();
        assert_eq!(me.min(), 10.0);
        assert_eq!(me.max(), 20.0);
        assert_eq!(me.value(), 20.0, "clamped at build");
        assert_eq!(me.orientation(), SliderOrientation::Vertical);
        assert_eq!(me.size(), SliderSize::Medium);
        assert_eq!(me.active_color(), None);
        assert_eq!(me.inactive_color(), None);
        // Vertical medium ticks are 2 px tall: 40 fit in 236 px.
        assert_eq!(me.tick_count(), 40);
        assert_eq!(me.lit_ticks(), 40);
    }

    #[test]
    fn percent_length_recounts_ticks_after_layout_and_on_resize() {
        use app::App;
        use layout::{LayoutModule, LayoutRoot, percent};
        use paint::PaintModule;
        use theme::MechanixTheme;

        let mut app = App::new();
        app.add_module(LayoutModule)
            .add_module(PaintModule)
            .add_module(MechanixTheme::dark());
        app.insert_resource(atlas::Atlas::new());

        let root = app
            .spawn_with(
                app.root(),
                div().style(LayoutStyle::default().size(px(200.0), px(100.0))),
                (LayoutRoot(true),),
            )
            .id();
        let s = app.spawn(root, slider().length(percent(50.0)).value(0.5));
        assert_eq!(app.widget::<Slider>(s).unwrap().ticks.len(), 0);

        let settle = |app: &mut App| {
            for _ in 0..4 {
                app.tick();
            }
        };
        let ticks_fit = |app: &App, expected: usize| {
            let me = app.widget::<Slider>(s).unwrap();
            assert_eq!(me.ticks.len(), expected);
            assert_eq!(me.tick_count, expected);
            let track = app.component::<ComputedLayout>(s).unwrap().content();
            let last = app
                .component::<ComputedLayout>(*me.ticks.last().unwrap())
                .unwrap();
            assert!(
                last.rect.right() <= track.right(),
                "last tick inside the track"
            );
            assert!(
                track.right() - last.rect.right() < 6.0,
                "remainder smaller than a tick + gap"
            );
        };

        // 50% of 200 = 100 px: 2 px ticks, 4 px gap -> 17 ticks using 17*2 + 16*4 = 98 px.
        settle(&mut app);
        ticks_fit(&app, 17);

        // Growing the parent adds ticks: 50% of 400 = 200 px -> 34 ticks.
        app.component_mut::<LayoutStyle>(root).unwrap().width = px(400.0);
        settle(&mut app);
        ticks_fit(&app, 34);

        // Shrinking removes them again.
        app.component_mut::<LayoutStyle>(root).unwrap().width = px(120.0);
        settle(&mut app);
        ticks_fit(&app, 10);
    }

    #[test]
    fn percent_tick_size_follows_layout() {
        use app::App;
        use layout::{LayoutModule, LayoutRoot, percent};
        use paint::PaintModule;
        use theme::MechanixTheme;

        let mut app = App::new();
        app.add_module(LayoutModule)
            .add_module(PaintModule)
            .add_module(MechanixTheme::dark());
        app.insert_resource(atlas::Atlas::new());

        let root = app
            .spawn_with(
                app.root(),
                div().style(LayoutStyle::default().size(px(200.0), px(100.0))),
                (LayoutRoot(true),),
            )
            .id();
        let s = app.spawn(
            root,
            slider()
                .length(percent(100.0))
                .tick_width(percent(2.0))
                .tick_height(percent(50.0))
                .style(LayoutStyle::default().height(px(80.0))),
        );
        let settle = |app: &mut App| {
            for _ in 0..4 {
                app.tick();
            }
        };
        let check = |app: &App, count: usize, w: f32, h: f32| {
            let me = app.widget::<Slider>(s).unwrap();
            assert_eq!(me.tick_count, count);
            assert_eq!(me.ticks.len(), count);
            let r = app.component::<ComputedLayout>(me.ticks[0]).unwrap().rect;
            assert!((r.width() - w).abs() < 0.01, "width {} != {w}", r.width());
            assert!(
                (r.height() - h).abs() < 0.01,
                "height {} != {h}",
                r.height()
            );
            let track = app.component::<ComputedLayout>(s).unwrap().content();
            let last = app
                .component::<ComputedLayout>(*me.ticks.last().unwrap())
                .unwrap();
            assert!(last.rect.right() <= track.right() + 0.01);
        };

        // 2% of 200 = 4 px ticks, 4 px gap: (200 + 4) / 8 = 25 ticks; 50% of 80 = 40 px tall.
        settle(&mut app);
        check(&app, 25, 4.0, 40.0);

        // 2% of 400 = 8 px ticks: (400 + 4) / 12 = 33 ticks.
        app.component_mut::<LayoutStyle>(root).unwrap().width = px(400.0);
        settle(&mut app);
        check(&app, 33, 8.0, 40.0);
    }

    #[test]
    fn slider_default_colors() {
        use app::App;
        use theme::{MechanixTheme, ThemeReader};

        let mut app = App::new();
        app.add_module(MechanixTheme::dark());

        let (active, inactive) = tick_colors(None, None, &app);
        assert_eq!(active, app.color(ColorRole::OnSecondaryFixed));
        assert_eq!(inactive, app.color(ColorRole::SecondaryFixedDim));
    }
}
