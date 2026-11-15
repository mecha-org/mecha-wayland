use crate::color::{ColorSource, IntoColorSource};
use crate::text::{Text, TextContextExt, text};
use app::{Build, Context, Event, Handle, OnChanged, Spawner, Widget};
use geometry::{Color, Insets, Point, Rect};
use interactivity::prelude::{ContactId, ContextClaim, Press, PressCancelled, PressMoved, Release};
use layout::{Align, ComputedLayout, Justify, LayoutStyle, Val, auto, percent, px};
use paint::{Paint, PaintContext, Quad};
use theme::{ColorRole, SpawnerThemeExt, TextVariant, ThemeReader};
use widgets::{Div, div};

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

impl SliderSize {
    /// Height of one tick, in px (its width on a vertical slider). Not the
    /// slider's own height: the slider reserves [`thumb_height`](Self::thumb_height).
    #[inline]
    pub fn tick_height(self) -> f32 {
        match self {
            Self::XSmall => 16.0,
            Self::Small => 24.0,
            Self::Medium => 40.0,
            Self::Large => 56.0,
            Self::XLarge => 96.0,
        }
    }

    /// Width of one tick, in px (its height on a vertical slider). Not the
    /// slider's own width: that is the track length, set with
    /// [`SliderBuilder::length`].
    #[inline]
    pub fn tick_width(self) -> f32 {
        2.0
    }

    #[inline]
    pub fn gap(self) -> f32 {
        4.0
    }

    #[inline]
    pub fn radius(self) -> f32 {
        0.0
    }

    #[inline]
    pub fn tick_size(self, orientation: SliderOrientation) -> (f32, f32) {
        match orientation {
            SliderOrientation::Horizontal => (self.tick_width(), self.tick_height()),
            SliderOrientation::Vertical => (self.tick_height(), self.tick_width()),
        }
    }

    /// [`tick_size`](Self::tick_size) as `px` layout values.
    #[inline]
    pub fn tick_size_val(self, orientation: SliderOrientation) -> (Val, Val) {
        let (w, h) = self.tick_size(orientation);
        (px(w), px(h))
    }

    /// Length of the thumb across the track (its height when horizontal).
    #[inline]
    pub fn thumb_height(self) -> f32 {
        match self {
            Self::XSmall => 32.0,
            Self::Small => 36.0,
            Self::Medium => 52.0,
            Self::Large => 68.0,
            Self::XLarge => 108.0,
        }
    }

    /// Space between the thumb's end and the value label.
    #[inline]
    pub fn label_gap(self) -> f32 {
        4.0
    }

    /// Typography of the value label.
    #[inline]
    pub fn label_variant(self) -> TextVariant {
        TextVariant::LabelLarge
    }
}

/// Formats a value for the label: up to two decimals, trailing zeros
/// dropped, so `50.0` reads `50` and `0.25` reads `0.25`.
pub fn default_value_format(value: f32) -> String {
    let s = format!("{value:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// Default track length.
pub const DEFAULT_LENGTH: Val = px(240.0);

/// How many slots (divs) of `tick` px separated by `gap` px fit in
/// `length` px: the slider's [`tick_count`](Slider::tick_count).
/// `n` ticks need `n * tick + (n - 1) * gap`, so `n = (length + gap) / (tick + gap)`.
/// `0` when not even one tick fits.
fn fit_count(length: f32, tick: f32, gap: f32) -> usize {
    let gap = gap.max(0.0);
    let pitch = tick + gap;
    if !(length.is_finite() && pitch.is_finite()) || pitch <= 0.0 || length < tick {
        return 0;
    }
    // Tolerate float error so an exact fit isn't lost.
    (((length + gap) / pitch) + 1e-4).floor() as usize
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
/// `percent` of the slider's content box; `auto` uses the size.
#[derive(Debug, Clone, Copy, PartialEq)]
struct TickSpec {
    width: Val,
    height: Val,
    gap: f32,
    radius: f32,
    size: SliderSize,
    /// Cross-axis length of the thumb, in px.
    thumb_height: f32,
}

impl TickSpec {
    /// Number of ticks that fit in `length` px along the main axis. While
    /// pressed, the thumb is drawn in place of one of them.
    fn slots_in(self, length: f32, orientation: SliderOrientation) -> usize {
        fit_count(length, self.extent(orientation, length), self.gap)
    }

    /// Main-axis size of one tick in px, for a track `length` px long.
    ///
    /// Snapped to whole pixels: layout rounds each tick's position, so a
    /// fractional size (e.g. 1% of 384 px = 3.84 px) would land each tick
    /// on the pixel grid differently and draw some 3 px and some 4 px wide.
    /// A visible tick never snaps below 1 px.
    fn extent(self, orientation: SliderOrientation, length: f32) -> f32 {
        let (main, default) = match orientation {
            SliderOrientation::Horizontal => (self.width, self.size.tick_width()),
            SliderOrientation::Vertical => (self.height, self.size.tick_width()),
        };
        let r = resolve_val(main, length, default);
        if r > 0.0 { r.round().max(1.0) } else { 0.0 }
    }

    /// Cross-axis size of one tick. Left to layout so `percent` tracks the
    /// slider's cross size; `auto` uses the size.
    fn cross(self, orientation: SliderOrientation) -> Val {
        let cross = match orientation {
            SliderOrientation::Horizontal => self.height,
            SliderOrientation::Vertical => self.width,
        };
        match cross {
            Val::Auto => px(self.size.tick_height()),
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
    /// The thumb marking the value; `None` for the theme's primary color.
    pub thumb_color: Option<ColorSource>,
    /// Cross-axis length of the thumb in px; `None` for [`SliderSize::thumb_height`].
    pub thumb_height: Option<f32>,
    /// Whether the value label shows beside the thumb while pressed
    /// (above it when horizontal, left of it when vertical).
    pub show_value: bool,
    /// Turns the value into the label's text.
    pub value_format: fn(f32) -> String,
    /// Space between the thumb's end and the label; `None` for [`SliderSize::label_gap`].
    pub label_gap: Option<f32>,
    /// Typography of the label; `None` for [`SliderSize::label_variant`].
    pub label_variant: Option<TextVariant>,
    /// Color of the label; `None` for the theme's on-surface color.
    pub label_color: Option<ColorSource>,
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
            thumb_color: None,
            thumb_height: None,
            show_value: true,
            value_format: default_value_format,
            label_gap: None,
            label_variant: None,
            label_color: None,
            style: None,
            orientation: SliderOrientation::Horizontal,
        }
    }

    /// Sets the size: the default tick size, gap and radius. Tick
    /// values set explicitly win, whichever order the calls are made in.
    pub fn size(mut self, size: SliderSize) -> Self {
        self.size = size;
        self
    }

    /// Lays the track out left to right. The default tick size is thin
    /// along the track and the size's tick height across it; an explicit
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

    /// The tick settings in effect: each override, else the size's
    /// default for the orientation.
    fn tick_spec(&self) -> TickSpec {
        let (w, h) = self.size.tick_size_val(self.orientation);
        TickSpec {
            width: self.tick_width.unwrap_or(w),
            height: self.tick_height.unwrap_or(h),
            gap: self.tick_gap.unwrap_or(self.size.gap()),
            radius: self.tick_radius.unwrap_or(self.size.radius()),
            size: self.size,
            thumb_height: self
                .thumb_height
                .unwrap_or(self.size.thumb_height())
                .max(0.0),
        }
    }

    /// Number of ticks drawn, known before layout: `Some` for a `px`
    /// length, `None` when it depends on layout (`percent` / `auto`).
    pub fn tick_count(&self) -> Option<usize> {
        match self.length {
            Val::Px(length) => Some(self.tick_spec().slots_in(length, self.orientation)),
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

    pub fn thumb_color(mut self, color: impl IntoColorSource) -> Self {
        self.thumb_color = color.into_color_source();
        self
    }

    /// Cross-axis length of the thumb in px (its height when horizontal).
    /// Defaults to [`SliderSize::thumb_height`].
    pub fn thumb_height(mut self, height: f32) -> Self {
        self.thumb_height = Some(height);
        self
    }

    /// Whether the value label shows beside the thumb while pressed (it
    /// does by default).
    ///
    /// # Leave room for the label
    ///
    /// The label is absolutely positioned: it takes up no space in the
    /// layout, so nothing is reserved for it. It sits
    /// [`label_gap`](Self::label_gap) past the thumb's end, which is
    /// outside the slider's box, so it can overlap whatever is next to the
    /// slider unless you leave room:
    ///
    /// - **Horizontal:** the label is above the slider. Leave at least
    ///   `label_gap` + the label's line height above it: 4 + 20 = 24 px
    ///   with the defaults ([`TextVariant::LabelLarge`] is 20 px tall).
    /// - **Vertical:** the label is to the left. Leave `label_gap` + the
    ///   width of the widest text [`value_format`](Self::value_format) can
    ///   produce.
    ///
    /// A parent `gap`, a margin or padding all work. For example, a column
    /// with a label above the slider:
    ///
    /// ```ignore
    /// div().style(LayoutStyle::default().column().gap(px(28.0)))
    /// ```
    ///
    /// Turn the label off with `show_value(false)` if there's no room.
    pub fn show_value(mut self, show: bool) -> Self {
        self.show_value = show;
        self
    }

    /// How the label prints the value, e.g. `|v| format!("{:.0}%", v * 100.0)`.
    pub fn value_format(mut self, format: fn(f32) -> String) -> Self {
        self.value_format = format;
        self
    }

    /// Space between the thumb's end and the label in px. Defaults to
    /// [`SliderSize::label_gap`].
    pub fn label_gap(mut self, gap: f32) -> Self {
        self.label_gap = Some(gap);
        self
    }

    /// Typography of the label. Defaults to [`SliderSize::label_variant`].
    pub fn label_variant(mut self, variant: TextVariant) -> Self {
        self.label_variant = Some(variant);
        self
    }

    /// Color of the label. Defaults to the theme's on-surface color.
    pub fn label_color(mut self, color: impl IntoColorSource) -> Self {
        self.label_color = color.into_color_source();
        self
    }

    /// The label settings in effect: each override, else the size's default.
    fn label_spec(&self) -> LabelSpec {
        LabelSpec {
            show: self.show_value,
            format: self.value_format,
            gap: self.label_gap.unwrap_or(self.size.label_gap()),
            variant: self.label_variant.unwrap_or(self.size.label_variant()),
            color: self.label_color,
        }
    }

    pub fn style(mut self, style: LayoutStyle) -> Self {
        self.style = Some(style);
        self
    }
}

/// How the value label looks.
#[derive(Debug, Clone, Copy)]
struct LabelSpec {
    show: bool,
    format: fn(f32) -> String,
    gap: f32,
    variant: TextVariant,
    /// `None` for the theme's on-surface color.
    color: Option<ColorSource>,
}

/// A spawned slider. Its state is read through the getters here and
/// changed through [`SliderContextExt`], which keeps the ticks in sync.
///
/// While pressed, a value label appears outside the slider's box (above it,
/// or to the left when vertical) and takes no layout space; leave room for
/// it. See [`SliderBuilder::show_value`].
pub struct Slider {
    size: SliderSize,
    value: f32,
    min: f32,
    max: f32,
    /// Number of ticks, derived from the laid-out track length; see
    /// [`tick_count`](Self::tick_count).
    tick_count: usize,
    active_color: Option<ColorSource>,
    inactive_color: Option<ColorSource>,
    thumb_color: Option<ColorSource>,
    orientation: SliderOrientation,
    tick: TickSpec,
    /// Main-axis size of one tick in px, resolved against the laid-out track.
    extent: f32,
    /// The `tick_count` tick divs (empty when no tick fits). While pressed,
    /// slot [`thumb_index`](Slider::thumb_index) is drawn as the thumb.
    pub(crate) ticks: Vec<Handle<Div>>,
    /// The slot currently styled as the thumb, if any.
    pub(crate) thumb: Option<usize>,
    /// The contact currently dragging the slider, if any.
    pub(crate) dragging: Option<ContactId>,
    label_spec: LabelSpec,
    /// The value label while shown: its positioning box and the text.
    pub(crate) label: Option<(Handle<Div>, Handle<Text>)>,
}

impl Slider {
    /// Whether a contact is currently dragging the slider.
    #[inline]
    pub fn is_dragging(&self) -> bool {
        self.dragging.is_some()
    }

    /// Size of the slider.
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

    /// Number of ticks drawn, derived from the laid-out track length. While
    /// pressed, one of them is drawn as the thumb. `0` before the first
    /// layout of a `percent` / `auto` length, or when not even one fits.
    #[inline]
    pub fn tick_count(&self) -> usize {
        self.tick_count
    }

    /// Number of places the thumb can move to past the first:
    /// `tick_count - 1`, or `0` with no ticks.
    #[inline]
    fn steps(&self) -> usize {
        self.tick_count.saturating_sub(1)
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

    /// The thumb-color override, `None` for the theme default.
    #[inline]
    pub fn thumb_color(&self) -> Option<ColorSource> {
        self.thumb_color
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

    /// The tick the thumb sits on for the current value, in
    /// `0..tick_count`: `min` puts it on the first tick, `max` on the last.
    /// The thumb is only drawn while pressed (see [`is_thumb_shown`](Self::is_thumb_shown));
    /// otherwise this slot is drawn as a plain tick.
    #[inline]
    pub fn thumb_index(&self) -> usize {
        (self.fraction() * self.steps() as f32).round() as usize
    }

    /// Whether the thumb is drawn: only while the slider is pressed or dragged.
    #[inline]
    pub fn is_thumb_shown(&self) -> bool {
        self.thumb.is_some()
    }

    /// Number of ticks drawn lit while the slider is at rest, in
    /// `0..=tick_count`: the ticks before the thumb's, plus the thumb's
    /// own unless the value is `min`, so the lit run ends where the thumb
    /// would appear. (While pressed the thumb's slot draws as the thumb.)
    #[inline]
    pub fn lit_ticks(&self) -> usize {
        self.thumb_index() + usize::from(self.fraction() > 0.0)
    }

    /// Whether slot `i` is lit while the thumb is hidden; see [`lit_ticks`](Self::lit_ticks).
    fn slot_lit(&self, i: usize) -> bool {
        i < self.lit_ticks()
    }

    /// Fraction (`0.0..=1.0`) of the range under `pos`, from the left when
    /// horizontal or the bottom when vertical. `None` when there are no
    /// ticks to slide over.
    ///
    /// With the thumb in slot `k`, its centre sits at
    /// `extent / 2 + k * pitch` (the thumb takes exactly one slot), so
    /// mapping the pointer through that line keeps the thumb under it.
    fn fraction_at(&self, content: Rect, pos: Point) -> Option<f32> {
        let pitch = self.extent + self.tick.gap.max(0.0);
        let travel = self.steps() as f32 * pitch;
        if travel <= 0.0 || !travel.is_finite() {
            return None;
        }
        let offset = match self.orientation {
            SliderOrientation::Horizontal => pos.x - content.x(),
            SliderOrientation::Vertical => content.bottom() - pos.y,
        };
        let first = self.extent / 2.0;
        Some(((offset - first) / travel).clamp(0.0, 1.0))
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

    /// The value as the label prints it.
    #[inline]
    pub fn value_text(&self) -> String {
        (self.label_spec.format)(self.value)
    }

    /// Whether the value label is currently shown.
    #[inline]
    pub fn is_label_shown(&self) -> bool {
        self.label.is_some()
    }

    /// Distance along the track from the content box's start (left, or
    /// bottom when vertical) to the centre of the thumb in slot `k`.
    fn thumb_center(&self, k: usize) -> f32 {
        self.extent / 2.0 + k as f32 * (self.extent + self.tick.gap.max(0.0))
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

fn thumb_color(thumb: Option<ColorSource>, reader: &impl ThemeReader) -> Color {
    thumb.unwrap_or(ColorRole::Primary.into()).resolve(reader)
}

impl Build for SliderBuilder {
    type Widget = Slider;
}

impl Widget for Slider {
    type Builder = SliderBuilder;

    fn build(b: SliderBuilder, me: Handle<Self>, s: &mut Spawner<'_, Self>) -> Self {
        let tick = b.tick_spec();
        let label_spec = b.label_spec();
        // Both known up front only for a `px` length; otherwise set by `fit_ticks`.
        let slots = match b.length {
            Val::Px(length) => tick.slots_in(length, b.orientation),
            _ => 0,
        };
        let extent = match b.length {
            Val::Px(length) => tick.extent(b.orientation, length),
            _ => 0.0,
        };
        // Ticks pack from the start (left / bottom); leftover space stays at
        // the end. `Justify` is in container terms, not flex-direction terms,
        // so a vertical (`column_reverse`) track packs at `End`, the bottom.
        let pack = match b.orientation {
            SliderOrientation::Horizontal => Justify::Start,
            SliderOrientation::Vertical => Justify::End,
        };
        let mut style = b
            .style
            .unwrap_or_default()
            .justify(pack)
            .align_items(Align::Center);
        // The thumb is taller than the ticks across the track; reserve its
        // room up front so pressing doesn't grow the slider and shift the
        // layout around it. A cross-axis minimum set in `style` is kept.
        let thumb_room = px(tick.thumb_height);
        match b.orientation {
            SliderOrientation::Horizontal => {
                style = style.row().column_gap(px(tick.gap)).min_width(px(0.0));
                if style.min_height == Val::Auto {
                    style = style.min_height(thumb_room);
                }
                if b.length != Val::Auto {
                    style = style.width(b.length);
                }
            }
            SliderOrientation::Vertical => {
                style = style
                    .column_reverse()
                    .row_gap(px(tick.gap))
                    .min_height(px(0.0));
                if style.min_width == Val::Auto {
                    style = style.min_width(thumb_room);
                }
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
            tick_count: slots,
            active_color: b.active_color,
            inactive_color: b.inactive_color,
            thumb_color: b.thumb_color,
            orientation: b.orientation,
            tick,
            extent,
            ticks: Vec::new(),
            thumb: None,
            dragging: None,
            label_spec,
            label: None,
        };
        slider.value = slider.clamp(slider.value);

        // No thumb yet: it only shows while the slider is pressed.
        let (active, inactive) = tick_colors(b.active_color, b.inactive_color, s);
        slider.ticks = (0..slots)
            .map(|i| {
                let color = if slider.slot_lit(i) { active } else { inactive };
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

/// Layout of the thumb slot: exactly a tick's place along the track (so
/// nothing around it moves), the thumb height across it, centred so it
/// reaches past the ticks on both sides.
fn thumb_style(tick: TickSpec, orientation: SliderOrientation, extent: f32) -> LayoutStyle {
    let cross = px(tick.thumb_height);
    let style = tick_style(tick, orientation, extent);
    match orientation {
        SliderOrientation::Horizontal => style.height(cross),
        SliderOrientation::Vertical => style.width(cross),
    }
}

/// Layout of the box holding the value label: absolutely placed so it never
/// moves the ticks or resizes the slider. Zero-sized along the track at the
/// thumb's centre (`center`, px from the padding box's start edge) so the
/// text centres on the thumb, with its near edge at the slider's cross-axis
/// centre and padded by half the thumb plus `gap`, so the text sits `gap`
/// past the thumb's end: above it when horizontal, left of it when vertical.
fn label_box_style(
    orientation: SliderOrientation,
    center: f32,
    thumb_height: f32,
    gap: f32,
) -> LayoutStyle {
    let (zero, reach) = (px(0.0), px(thumb_height / 2.0 + gap));
    let style = LayoutStyle::default().absolute().align_items(Align::Center);
    match orientation {
        SliderOrientation::Horizontal => style
            .column()
            .width(zero)
            .padding(Insets::new(zero, zero, reach, zero))
            .inset(Insets::new(auto(), auto(), percent(50.0), px(center))),
        SliderOrientation::Vertical => style
            .row()
            .height(zero)
            .padding(Insets::new(zero, reach, zero, zero))
            .inset(Insets::new(px(center), percent(50.0), auto(), auto())),
    }
}

/// Shows the value label beside the thumb in slot `thumb` (or hides it,
/// for `None`), keeping its text and position in line with the value.
fn sync_label(ctx: &mut Context<'_, Slider>, thumb: Option<usize>) {
    let spec = ctx.me().label_spec;
    let layout = ctx.component::<ComputedLayout>().copied();
    let (Some(k), true, Some(layout)) = (thumb, spec.show, layout) else {
        if let Some((label_box, _)) = ctx.me().label.take() {
            ctx.remove(label_box);
        }
        return;
    };

    // Insets are measured from the padding box.
    let orientation = ctx.me().orientation;
    let thumb_height = ctx.me().tick.thumb_height;
    let along = ctx.me().thumb_center(k);
    let content = layout.content();
    let center = match orientation {
        SliderOrientation::Horizontal => {
            content.x() - (layout.rect.x() + layout.border.left) + along
        }
        SliderOrientation::Vertical => {
            content.bottom() - (layout.rect.y() + layout.border.top) - along
        }
    };
    let style = label_box_style(orientation, center, thumb_height, spec.gap);
    let value = ctx.me().value_text();

    match ctx.me().label {
        Some((label_box, text_node)) => {
            restyle(ctx, label_box, style);
            if let Some(mut t) = ctx.at(text_node) {
                t.set_text(value);
            }
        }
        None => {
            let me = ctx.handle();
            let label_box = ctx.spawn(me, div().style(style));
            let text_node = ctx.spawn(
                label_box,
                text(value)
                    .variant(spec.variant)
                    .color(spec.color.unwrap_or(ColorRole::OnSurface.into()))
                    .style(LayoutStyle::default().shrink(0.0)),
            );
            ctx.me().label = Some((label_box, text_node));
        }
    }
}

/// Writes `style` to `node` unless it already has it, so an unchanged
/// style doesn't trigger a relayout.
fn restyle(ctx: &mut Context<'_, Slider>, node: Handle<Div>, style: LayoutStyle) {
    if let Some(mut c) = ctx.at(node)
        && let Some(mut s) = c.component_mut::<LayoutStyle>()
        && *s != style
    {
        *s = style;
    }
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
    let want = fit_count(length, extent, tick.gap);

    // A `percent` tick resizes with the track: restyle the existing slots.
    // That drops the thumb's look too; `sync_ticks` puts it back.
    let resized = ctx.me().extent != extent;
    if resized {
        ctx.me().extent = extent;
        ctx.me().thumb = None;
        let style = tick_style(tick, orientation, extent);
        let ticks = ctx.me().ticks.clone();
        for t in ticks {
            restyle(ctx, t, style.clone());
        }
    }

    let have = ctx.me().ticks.len();
    if want == have {
        if resized {
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
/// can't take it over, then jumps to the pressed position and shows the thumb.
fn on_press(ctx: &mut Context<'_, Slider>, e: &Press) {
    if ctx.me().dragging.is_some() {
        // Another contact already drives the slider.
        return;
    }
    ctx.me().dragging = Some(e.contact);
    ctx.claim_contact(e.contact);
    if !set_from_pointer(ctx, e.position) {
        // Same value: the ticks are right, but the thumb must appear.
        sync_ticks(ctx);
    }
}

/// Follows the dragging contact, even outside the slider's bounds.
fn on_press_moved(ctx: &mut Context<'_, Slider>, e: &PressMoved) {
    if ctx.me().dragging == Some(e.contact) {
        set_from_pointer(ctx, e.position);
    }
}

/// Ends the drag and hides the thumb.
fn end_drag(ctx: &mut Context<'_, Slider>, contact: ContactId) {
    if ctx.me().dragging == Some(contact) {
        ctx.me().dragging = None;
        sync_ticks(ctx);
    }
}

/// Sets the value from the pointer's position along the track; on a
/// change, syncs the ticks and emits [`SliderChange`]. Returns whether it changed.
fn set_from_pointer(ctx: &mut Context<'_, Slider>, pos: Point) -> bool {
    let range = ctx.me().max - ctx.me().min;
    if !range.is_finite() || range <= 0.0 {
        return false;
    }
    let Some(content) = ctx.component::<ComputedLayout>().map(|l| l.content()) else {
        return false;
    };
    let me = ctx.me();
    let Some(fraction) = me.fraction_at(content, pos) else {
        return false;
    };
    let value = me.value_at_fraction(fraction);
    if !me.set(value) {
        return false;
    }
    let value = me.value;

    sync_ticks(ctx);
    let handle = ctx.handle();
    ctx.emit(SliderChange { value }, handle);
    true
}

/// Brings the slots in line with the value. While pressed, the thumb's slot
/// gets the thumb look, its two neighbours are hidden, the ones before it
/// get the active color and the rest the inactive one; otherwise every slot
/// is a plain tick.
fn sync_ticks(ctx: &mut Context<'_, Slider>) {
    let (thumb, count, tick, orientation, extent, pressed) = {
        let me = ctx.me();
        (
            me.thumb_index(),
            me.ticks.len(),
            me.tick,
            me.orientation,
            me.extent,
            me.dragging.is_some(),
        )
    };
    let (active, inactive, thumb_color) = {
        let me = ctx.me();
        let (a, i, t) = (me.active_color, me.inactive_color, me.thumb_color);
        let (a, i) = tick_colors(a, i, ctx);
        (a, i, thumb_color(t, ctx))
    };

    // Move the thumb's look: restore the slot it leaves, restyle the new one.
    let want = (pressed && count > 0).then_some(thumb);
    let shown = ctx.me().thumb;
    if shown != want {
        if let Some(old) = shown
            && let Some(&t) = ctx.me().ticks.get(old)
        {
            restyle(ctx, t, tick_style(tick, orientation, extent));
        }
        if let Some(new) = want
            && let Some(&t) = ctx.me().ticks.get(new)
        {
            restyle(ctx, t, thumb_style(tick, orientation, extent));
        }
        ctx.me().thumb = want;
    }
    sync_label(ctx, want);

    // While pressed, the ticks right beside the thumb are hidden (kept in
    // place, drawn transparent) to give it room without moving anything.
    let beside = |i: usize| want.is_some_and(|k| i + 1 == k || i == k + 1);
    for i in 0..count {
        let slot = ctx.me().ticks[i];
        let color = if want == Some(i) {
            thumb_color
        } else if beside(i) {
            Color::TRANSPARENT
        } else if ctx.me().slot_lit(i) {
            active
        } else {
            inactive
        };
        paint_slot(ctx, slot, color, tick.radius);
    }
}

/// Paints a slot as a `color` quad with the tick radius. The whole quad is
/// written (not just the colour, as `set_background` would): a transparent
/// quad is stored as `Paint::None`, which forgets the radius, so a hidden
/// neighbour would otherwise come back square. `set_paint` skips the write
/// when the paint is unchanged, so a drag only touches the slots it moves.
fn paint_slot(ctx: &mut Context<'_, Slider>, slot: Handle<Div>, color: Color, radius: f32) {
    let quad = Quad::new(color).radius(radius);
    let paint = if quad.is_invisible() {
        Paint::None
    } else {
        Paint::Quad(quad)
    };
    if let Some(mut c) = ctx.at(slot) {
        c.set_paint(paint);
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
    fn set_thumb_color(&mut self, color: impl IntoColorSource);
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

    fn set_thumb_color(&mut self, color: impl IntoColorSource) {
        self.me().thumb_color = color.into_color_source();
        sync_ticks(self);
    }

    fn sync_visuals(&mut self) {
        sync_ticks(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gives `app` an atlas with a font, for the value label's text.
    fn add_font(app: &mut app::App) {
        let mut atlas = atlas::Atlas::new();
        let font = atlas
            .add_font(include_bytes!(
                "../../../crates/atlas/tests/fixtures/Inter-Regular.ttf"
            ))
            .unwrap();
        app.insert_resource(atlas);
        app.insert_resource(crate::font::FontBook::new(font));
    }

    /// An app that lays out, paints and takes presses, with a font, and a
    /// 400 x 300 root that keeps its children at their natural size.
    fn pressable_app() -> (app::App, app::NodeId) {
        let mut app = app::App::new();
        app.add_module(layout::LayoutModule)
            .add_module(paint::PaintModule)
            .add_module(interactivity::InteractivityModule)
            .add_module(theme::MechanixTheme::dark());
        add_font(&mut app);
        let root = app
            .spawn_with(
                app.root(),
                div().style(
                    LayoutStyle::default()
                        .size(px(400.0), px(300.0))
                        .align_items(Align::Start)
                        .padding_all(px(60.0)),
                ),
                (layout::LayoutRoot(true),),
            )
            .id();
        (app, root)
    }

    fn settle(app: &mut app::App) {
        for _ in 0..4 {
            app.tick();
        }
    }

    fn press(app: &mut app::App, s: Handle<Slider>, at: Point) {
        app.emit(
            Press {
                contact: ContactId::Mouse,
                position: at,
                time: 0,
            },
            s.id(),
        );
        settle(app);
    }

    fn slider_at(value: f32, min: f32, max: f32, tick_count: usize) -> Slider {
        Slider {
            size: SliderSize::Medium,
            value,
            min,
            max,
            tick_count,
            active_color: None,
            inactive_color: None,
            thumb_color: None,
            orientation: SliderOrientation::Horizontal,
            tick: slider().tick_spec(),
            extent: 2.0,
            ticks: vec![],
            thumb: None,
            dragging: None,
            label_spec: slider().label_spec(),
            label: None,
        }
    }

    #[test]
    fn slider_fraction_and_lit_ticks() {
        let mut s = slider_at(20.0, 0.0, 100.0, 51);
        assert_eq!(s.fraction(), 0.2);
        // The thumb's own slot is lit too once the value is past `min`.
        assert_eq!(s.lit_ticks(), 11);

        s.value = 50.0;
        assert_eq!(s.fraction(), 0.5);
        assert_eq!(s.lit_ticks(), 26);

        s.value = 100.0;
        assert_eq!(s.fraction(), 1.0);
        assert_eq!(s.lit_ticks(), 51, "all 51 ticks");

        s.value = 0.0;
        assert_eq!(s.lit_ticks(), 0, "nothing lit at min");
    }

    #[test]
    fn tick_count_fills_length_and_leaves_remainder_empty() {
        // 2 px ticks, 4 px gap: n ticks need 6n - 4 px.
        assert_eq!(fit_count(236.0, 2.0, 4.0), 40); // exact fit
        assert_eq!(fit_count(240.0, 2.0, 4.0), 40); // 4 px left empty
        assert_eq!(fit_count(241.0, 2.0, 4.0), 40); // 5 px: still no room
        assert_eq!(fit_count(242.0, 2.0, 4.0), 41); // room for one more
        assert_eq!(fit_count(2.0, 2.0, 4.0), 1); // exactly one tick
        assert_eq!(fit_count(1.0, 2.0, 4.0), 0); // not even one fits
        assert_eq!(fit_count(0.0, 2.0, 4.0), 0);
        assert_eq!(fit_count(f32::NAN, 2.0, 4.0), 0);
        assert_eq!(fit_count(100.0, 0.0, 0.0), 0); // zero pitch: no infinite ticks

        // Vertical uses the tick height as the main-axis extent:
        // 100 px -> 17 ticks.
        let v = slider().vertical().length(px(100.0));
        assert_eq!(v.tick_count(), Some(17));
    }

    #[test]
    fn pointer_keeps_the_thumb_under_it() {
        // 41 ticks: 2 px wide, 4 px gaps. The thumb takes exactly
        // one tick's place, so in slot `k` its centre is at `x + 1 + 6k`.
        let mut s = slider_at(0.0, 0.0, 100.0, 41);
        let track = Rect::new(10.0, 0.0, 260.0, 40.0);
        let at = |s: &Slider, x: f32| s.fraction_at(track, Point::new(x, 0.0));

        assert_eq!(at(&s, 11.0), Some(0.0));
        assert_eq!(at(&s, 11.0 + 120.0), Some(0.5));
        assert_eq!(at(&s, 11.0 + 240.0), Some(1.0));
        // Past either end clamps.
        assert_eq!(at(&s, 0.0), Some(0.0));
        assert_eq!(at(&s, 300.0), Some(1.0));

        // Pressing where slot `k` would put its thumb there.
        for k in [0usize, 7, 20, 40] {
            let f = at(&s, 11.0 + 6.0 * k as f32).unwrap();
            s.value = s.value_at_fraction(f);
            assert_eq!(s.thumb_index(), k, "slot {k}");
            let lit = k + usize::from(k > 0);
            assert_eq!(
                s.lit_ticks(),
                lit,
                "lit up to and including the thumb's slot"
            );
        }

        // Vertical measures up from the bottom.
        s.orientation = SliderOrientation::Vertical;
        s.tick = slider().vertical().tick_spec();
        let track = Rect::new(0.0, 0.0, 40.0, 260.0);
        assert_eq!(s.fraction_at(track, Point::new(0.0, 259.0)), Some(0.0));
        assert_eq!(s.fraction_at(track, Point::new(0.0, 139.0)), Some(0.5));
        assert_eq!(s.fraction_at(track, Point::new(0.0, 19.0)), Some(1.0));

        // No ticks yet (before the first layout): nothing to slide over.
        s.tick_count = 0;
        assert_eq!(s.fraction_at(track, Point::new(0.0, 100.0)), None);
    }

    #[test]
    fn slider_tolerates_inverted_range() {
        let s = slider_at(0.0, 10.0, 0.0, 2);
        assert_eq!(s.clamp(5.0), 0.0);
        assert_eq!(s.fraction(), 0.0);
    }

    #[test]
    fn slider_value_is_continuous() {
        let mut s = slider_at(0.0, 0.0, 100.0, 5);
        // No snapping: values between ticks are kept as-is.
        assert!(s.set(38.0));
        assert_eq!(s.value, 38.0);
        assert!(!s.set(38.0));
        assert_eq!(s.thumb_index(), 2); // 1.52 ticks rounds to 2
        assert_eq!(s.lit_ticks(), 3); // ...and that slot is lit too

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
        assert_eq!(SliderSize::XSmall.tick_height(), 16.0);
        assert_eq!(SliderSize::XSmall.tick_width(), 2.0);
        assert_eq!(SliderSize::XSmall.gap(), 4.0);
        assert_eq!(SliderSize::XSmall.radius(), 0.0);

        // Small: — × 24 px
        assert_eq!(SliderSize::Small.tick_height(), 24.0);
        assert_eq!(SliderSize::Small.tick_width(), 2.0);
        assert_eq!(SliderSize::Small.gap(), 4.0);
        assert_eq!(SliderSize::Small.radius(), 0.0);

        // Medium (default): — × 40 px
        assert_eq!(SliderSize::Medium.tick_height(), 40.0);
        assert_eq!(SliderSize::Medium.tick_width(), 2.0);
        assert_eq!(SliderSize::Medium.gap(), 4.0);
        assert_eq!(SliderSize::Medium.radius(), 0.0);

        // Large: — × 56 px
        assert_eq!(SliderSize::Large.tick_height(), 56.0);
        assert_eq!(SliderSize::Large.tick_width(), 2.0);
        assert_eq!(SliderSize::Large.gap(), 4.0);
        assert_eq!(SliderSize::Large.radius(), 0.0);

        // XLarge: — × 96 px
        assert_eq!(SliderSize::XLarge.tick_height(), 96.0);
        assert_eq!(SliderSize::XLarge.tick_width(), 2.0);
        assert_eq!(SliderSize::XLarge.gap(), 4.0);
        assert_eq!(SliderSize::XLarge.radius(), 0.0);

        // Thumb heights per size.
        assert_eq!(SliderSize::XSmall.thumb_height(), 32.0);
        assert_eq!(SliderSize::Small.thumb_height(), 36.0);
        assert_eq!(SliderSize::Medium.thumb_height(), 52.0);
        assert_eq!(SliderSize::Large.thumb_height(), 68.0);
        assert_eq!(SliderSize::XLarge.thumb_height(), 108.0);
    }

    #[test]
    fn slider_builder() {
        let b = slider().range(0.0, 100.0).value(45.0).length(px(236.0));

        assert_eq!(b.min, 0.0);
        assert_eq!(b.max, 100.0);
        assert_eq!(b.value, 45.0);
        assert_eq!(b.length, px(236.0));
        // 236 px -> 40 ticks.
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

        // Thumb overrides win over the size, whatever the order.
        let t = slider().thumb_height(40.0).size(SliderSize::XLarge);
        assert_eq!(t.tick_spec().thumb_height, 40.0);
        assert_eq!(
            slider().size(SliderSize::Small).tick_spec().thumb_height,
            36.0
        );

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
    fn slider_sizes() {
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
        assert_eq!(
            (a.tick_spec().width, a.tick_spec().height),
            (px(4.0), px(2.0))
        );
        assert_eq!(a.tick_spec().gap, 8.0);
        assert_eq!(b.tick_spec().gap, 8.0);

        let r = slider().tick_radius(3.0).size(SliderSize::XSmall);
        assert_eq!(r.tick_spec().radius, 3.0);
        assert_eq!(
            (r.tick_spec().width, r.tick_spec().height),
            (px(2.0), px(16.0))
        );

        // Flipping orientation twice is a no-op for overrides.
        let f = slider().tick_height(px(30.0)).vertical().horizontal();
        assert_eq!(
            (f.tick_spec().width, f.tick_spec().height),
            (px(2.0), px(30.0))
        );
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
        assert_eq!(me.thumb_color(), None);
        // Vertical medium ticks are 2 px tall: 40 fit in 236 px.
        assert_eq!(me.tick_count(), 40);
        assert_eq!(me.ticks.len(), 40);
        assert_eq!(me.lit_ticks(), 40, "at max every tick is lit");
        assert!(!me.is_thumb_shown(), "no thumb until pressed");
        assert_eq!(me.thumb_index(), 39, "at max it would sit in the last slot");
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
            assert_eq!(me.tick_count(), expected);
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

        // 50% of 200 = 100 px: 2 px ticks, 4 px gap -> 17 ticks
        // using 17*2 + 16*4 = 98 px.
        settle(&mut app);
        ticks_fit(&app, 17);

        // Growing the parent adds ticks: 50% of 400 = 200 px -> 34 ticks.
        app.component_mut::<LayoutStyle>(root).unwrap().width = px(400.0);
        settle(&mut app);
        ticks_fit(&app, 34);

        // Shrinking removes them again: 50% of 120 = 60 px -> 10 ticks.
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
            assert_eq!(me.tick_count(), count);
            assert_eq!(me.ticks.len(), count);
            // Value 0: the thumb is slot 0, so the last slot is a plain tick.
            let r = app
                .component::<ComputedLayout>(*me.ticks.last().unwrap())
                .unwrap()
                .rect;
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
    fn thumb_shows_only_while_pressed_and_follows_the_drag() {
        use app::App;
        use layout::{LayoutModule, LayoutRoot};
        use paint::PaintModule;
        use theme::MechanixTheme;

        let mut app = App::new();
        app.add_module(LayoutModule)
            .add_module(PaintModule)
            .add_module(interactivity::InteractivityModule)
            .add_module(MechanixTheme::dark());
        add_font(&mut app);

        let root = app
            .spawn_with(
                app.root(),
                div().style(
                    LayoutStyle::default()
                        .size(px(400.0), px(200.0))
                        // Natural height, so a growing slider would show.
                        .align_items(Align::Start),
                ),
                (LayoutRoot(true),),
            )
            .id();
        // 236 px: 40 ticks, filling the track exactly.
        let s = app.spawn(
            root,
            slider()
                .size(SliderSize::Large)
                .length(px(236.0))
                .range(0.0, 39.0)
                .value(10.0)
                // Rounded, to check hidden neighbours keep it.
                .tick_radius(1.0),
        );
        let settle = |app: &mut App| {
            for _ in 0..4 {
                app.tick();
            }
        };
        let rect = |app: &App, i: usize| {
            let me = app.widget::<Slider>(s).unwrap();
            app.component::<ComputedLayout>(me.ticks[i]).unwrap().rect
        };
        let check = |app: &App, k: usize| {
            let me = app.widget::<Slider>(s).unwrap();
            assert_eq!(me.thumb, Some(k));
            let track = app.component::<ComputedLayout>(s).unwrap().content();
            let thumb = rect(app, k);
            assert_eq!(thumb.height(), 68.0, "large thumb height");
            assert_eq!(thumb.width(), 2.0);
            // Nothing moves: every slot, the thumb included, stays where it
            // sits at rest.
            for i in 0..me.ticks.len() {
                assert_eq!(rect(app, i).x() - track.x(), 6.0 * i as f32, "slot {i}");
                assert_eq!(rect(app, i).width(), 2.0);
            }
            // The ticks right beside the thumb are hidden; the rest show.
            let shown = |i: usize| match app.component::<paint::Paint>(me.ticks[i]) {
                Some(paint::Paint::Quad(q)) => q.color != Color::TRANSPARENT,
                _ => false,
            };
            for i in 0..me.ticks.len() {
                let beside = i + 1 == k || i == k + 1;
                assert_eq!(shown(i), !beside, "slot {i} with the thumb in {k}");
            }
        };

        settle(&mut app);
        // At rest: no thumb, every slot a plain tick, the track's slack at the end.
        {
            let me = app.widget::<Slider>(s).unwrap();
            assert!(!me.is_thumb_shown());
            let track = app.component::<ComputedLayout>(s).unwrap().content();
            for i in 0..me.ticks.len() {
                let r = rect(&app, i);
                assert_eq!(r.height(), 56.0, "slot {i} is a plain tick");
                assert_eq!(r.x() - track.x(), 6.0 * i as f32);
            }
        }

        // The slider already has the thumb's height, so pressing won't move
        // anything around it.
        let rest_box = app.component::<ComputedLayout>(s).unwrap().rect;
        assert_eq!(rest_box.height(), 68.0);

        // Dragging moves the thumb under the pointer; the slot it leaves is
        // a plain tick again. Slot `k`'s thumb centre is at `x + 1 + 6k`.
        let x0 = app.component::<ComputedLayout>(s).unwrap().content().x() + 1.0;
        let at = |k: usize| Point::new(x0 + 6.0 * k as f32, 0.0);
        app.emit(
            Press {
                contact: ContactId::Mouse,
                position: at(25),
                time: 0,
            },
            s.id(),
        );
        settle(&mut app);
        check(&app, 25);
        assert_eq!(
            app.component::<ComputedLayout>(s).unwrap().rect,
            rest_box,
            "pressing doesn't resize or move the slider"
        );
        assert_eq!(rect(&app, 10).height(), 56.0);
        assert_eq!(app.widget::<Slider>(s).unwrap().value(), 25.0);

        for k in [0usize, 39, 3] {
            app.emit(
                PressMoved {
                    contact: ContactId::Mouse,
                    position: at(k),
                    time: 0,
                },
                s.id(),
            );
            settle(&mut app);
            check(&app, k);
        }

        // Releasing hides the thumb again; its slot goes back to a tick.
        app.emit(
            Release {
                contact: ContactId::Mouse,
                position: at(3),
                time: 0,
            },
            s.id(),
        );
        settle(&mut app);
        let me = app.widget::<Slider>(s).unwrap();
        assert!(!me.is_thumb_shown());
        assert_eq!(rect(&app, 3).height(), 56.0);
        assert_eq!(me.value(), 3.0, "the value stays where it was dropped");
        // Every slot shows again, rounded, including the neighbours that were
        // hidden (a transparent quad is stored as `Paint::None`, radius lost).
        for i in 0..me.ticks.len() {
            match app.component::<paint::Paint>(me.ticks[i]) {
                Some(paint::Paint::Quad(q)) => {
                    assert_eq!(
                        q.radii,
                        geometry::Corners::all(1.0),
                        "slot {i} keeps its radius"
                    );
                }
                p => panic!("slot {i} should be a visible quad, got {p:?}"),
            }
        }

        // Pressing on the current value still brings the thumb up.
        app.emit(
            Press {
                contact: ContactId::Mouse,
                position: at(3),
                time: 0,
            },
            s.id(),
        );
        settle(&mut app);
        check(&app, 3);
    }

    #[test]
    fn value_label_sits_4px_past_the_thumb_while_pressed() {
        let (mut app, root) = pressable_app();
        // 236 px: 40 ticks, one per unit of 0..=39.
        let s = app.spawn(
            root,
            slider()
                .size(SliderSize::Large)
                .length(px(236.0))
                .range(0.0, 39.0)
                .value(10.0),
        );
        settle(&mut app);
        assert!(!app.widget::<Slider>(s).unwrap().is_label_shown());

        let x0 = app.component::<ComputedLayout>(s).unwrap().content().x() + 1.0;
        for k in [25usize, 0, 39] {
            if k == 25 {
                press(&mut app, s, Point::new(x0 + 6.0 * k as f32, 0.0));
            } else {
                app.emit(
                    PressMoved {
                        contact: ContactId::Mouse,
                        position: Point::new(x0 + 6.0 * k as f32, 0.0),
                        time: 0,
                    },
                    s.id(),
                );
                settle(&mut app);
            }
            let me = app.widget::<Slider>(s).unwrap();
            let (_, text_node) = me.label.expect("label shown while pressed");
            assert_eq!(me.value_text(), k.to_string());
            let thumb = app
                .component::<ComputedLayout>(me.ticks[me.thumb.unwrap()])
                .unwrap()
                .rect;
            let label = app.component::<ComputedLayout>(text_node).unwrap().rect;
            assert!(label.width() > 0.0 && label.height() > 0.0);
            assert!(
                (thumb.y() - label.bottom() - 4.0).abs() <= 1.0,
                "4 px above the thumb: thumb top {}, label bottom {}",
                thumb.y(),
                label.bottom()
            );
            let (tc, lc) = (
                thumb.x() + thumb.width() / 2.0,
                label.x() + label.width() / 2.0,
            );
            assert!((tc - lc).abs() <= 1.0, "centred on the thumb: {tc} vs {lc}");
        }

        app.emit(
            Release {
                contact: ContactId::Mouse,
                position: Point::ZERO,
                time: 0,
            },
            s.id(),
        );
        settle(&mut app);
        assert!(!app.widget::<Slider>(s).unwrap().is_label_shown());
    }

    #[test]
    fn vertical_value_label_sits_4px_left_of_the_thumb() {
        let (mut app, root) = pressable_app();
        let s = app.spawn(
            root,
            slider()
                .size(SliderSize::Large)
                .vertical()
                .length(px(200.0))
                .value(0.5)
                .value_format(|v| format!("{:.0}", v * 100.0)),
        );
        settle(&mut app);
        let content = app.component::<ComputedLayout>(s).unwrap().content();
        press(
            &mut app,
            s,
            Point::new(content.x(), content.y() + content.height() / 2.0),
        );
        let me = app.widget::<Slider>(s).unwrap();
        let (_, text_node) = me.label.expect("label shown while pressed");
        let thumb = app
            .component::<ComputedLayout>(me.ticks[me.thumb.unwrap()])
            .unwrap()
            .rect;
        let label = app.component::<ComputedLayout>(text_node).unwrap().rect;
        assert!(
            (thumb.x() - label.right() - 4.0).abs() <= 1.0,
            "4 px left of the thumb: thumb left {}, label right {}",
            thumb.x(),
            label.right()
        );
        let (tc, lc) = (
            thumb.y() + thumb.height() / 2.0,
            label.y() + label.height() / 2.0,
        );
        assert!((tc - lc).abs() <= 1.0, "centred on the thumb: {tc} vs {lc}");
    }

    #[test]
    fn show_value_false_never_shows_a_label() {
        let (mut app, root) = pressable_app();
        let s = app.spawn(root, slider().show_value(false));
        settle(&mut app);
        let c = app.component::<ComputedLayout>(s).unwrap().content();
        press(&mut app, s, Point::new(c.x() + 20.0, c.y()));
        let me = app.widget::<Slider>(s).unwrap();
        assert!(me.is_thumb_shown());
        assert!(!me.is_label_shown());
    }

    #[test]
    fn label_defaults() {
        assert_eq!(SliderSize::Medium.label_gap(), 4.0);
        assert_eq!(SliderSize::Medium.label_variant(), TextVariant::LabelLarge);
        let spec = slider().label_spec();
        assert!(spec.show);
        assert_eq!(spec.gap, 4.0);
        assert_eq!(spec.variant, TextVariant::LabelLarge);
        assert_eq!(spec.color, None);
        assert_eq!(default_value_format(50.0), "50");
        assert_eq!(default_value_format(0.25), "0.25");
        assert_eq!(default_value_format(-0.0001), "0");
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
