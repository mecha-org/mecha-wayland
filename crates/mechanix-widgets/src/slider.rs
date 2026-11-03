use crate::color::{ColorSource, IntoColorSource};
use app::{Build, Context, Event, Handle, Spawner, Widget};
use geometry::{Color, Point};
use interactivity::prelude::{ContactId, ContextClaim, Press, PressCancelled, PressMoved, Release};
use layout::{ComputedLayout, LayoutStyle, px};
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
}

/// Emitted when a press or drag selects a different value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderChange {
    pub value: f32,
    pub step_index: usize,
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
    pub steps: usize,
    pub tick_gap: f32,
    pub tick_radius: f32,
    pub tick_width: f32,
    pub tick_height: f32,
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
        let size = SliderSize::Medium;
        let (tick_width, tick_height) = size.tick_size(SliderOrientation::Horizontal);
        Self {
            size,
            value: 0.0,
            min: 0.0,
            max: 1.0,
            steps: 40,
            tick_gap: size.gap(),
            tick_radius: size.radius(),
            tick_width,
            tick_height,
            active_color: None,
            inactive_color: None,
            style: None,
            orientation: SliderOrientation::Horizontal,
        }
    }

    /// Sets the slider size variant and updates tick dimensions, gap, and radius.
    pub fn size(mut self, size: SliderSize) -> Self {
        self.size = size;
        let (w, h) = size.tick_size(self.orientation);
        self.tick_width = w;
        self.tick_height = h;
        self.tick_gap = size.gap();
        self.tick_radius = size.radius();
        self
    }

    /// Alias for [`size`](Self::size).
    pub fn variant(self, variant: SliderSize) -> Self {
        self.size(variant)
    }

    pub fn horizontal(mut self) -> Self {
        if self.orientation == SliderOrientation::Vertical {
            self.orientation = SliderOrientation::Horizontal;
            std::mem::swap(&mut self.tick_width, &mut self.tick_height);
        }
        self
    }

    pub fn vertical(mut self) -> Self {
        if self.orientation == SliderOrientation::Horizontal {
            self.orientation = SliderOrientation::Vertical;
            std::mem::swap(&mut self.tick_width, &mut self.tick_height);
        }
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

    /// Sets the number of intervals; the slider has `steps + 1` selectable values.
    pub fn steps(mut self, steps: usize) -> Self {
        self.steps = steps.max(1);
        self
    }

    pub fn tick_gap(mut self, gap: f32) -> Self {
        self.tick_gap = gap;
        self
    }

    pub fn tick_radius(mut self, radius: f32) -> Self {
        self.tick_radius = radius;
        self
    }

    pub fn tick_width(mut self, width: f32) -> Self {
        self.tick_width = width;
        self
    }

    pub fn tick_height(mut self, height: f32) -> Self {
        self.tick_height = height;
        self
    }

    pub fn tick_size(mut self, width: f32, height: f32) -> Self {
        self.tick_width = width;
        self.tick_height = height;
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

pub struct Slider {
    pub size: SliderSize,
    pub value: f32,
    pub min: f32,
    pub max: f32,
    /// Always at least 1.
    pub steps: usize,
    pub active_color: Option<ColorSource>,
    pub inactive_color: Option<ColorSource>,
    pub orientation: SliderOrientation,
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

    /// Number of lit ticks, in `0..=steps`.
    #[inline]
    pub fn step_index(&self) -> usize {
        (self.fraction() * self.steps as f32).round() as usize
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

    /// Value represented by a step, keeping the endpoints exact.
    fn value_at_step(&self, index: usize) -> f32 {
        if index == 0 {
            self.min
        } else if index >= self.steps {
            self.max
        } else {
            self.min + (self.max - self.min) * (index as f32 / self.steps as f32)
        }
    }

    fn snap(&self, value: f32) -> f32 {
        let value = self.clamp(value);
        if !(self.max - self.min).is_finite() || self.max <= self.min {
            return value;
        }
        let index = (self.fraction_for(value) * self.steps as f32).round() as usize;
        self.value_at_step(index)
    }

    /// Returns whether the selected value changed.
    fn select_step(&mut self, index: usize) -> bool {
        let value = self.value_at_step(index);
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
        let mut style = b.style.unwrap_or_default();
        match b.orientation {
            SliderOrientation::Horizontal => {
                style = style.row().center().column_gap(px(b.tick_gap));
            }
            SliderOrientation::Vertical => {
                style = style.column_reverse().center().row_gap(px(b.tick_gap));
            }
        }
        *s.component_mut::<LayoutStyle>(me).unwrap() = style;

        let mut slider = Slider {
            size: b.size,
            value: b.value,
            min: b.min,
            max: b.max,
            steps: b.steps.max(1),
            active_color: b.active_color,
            inactive_color: b.inactive_color,
            orientation: b.orientation,
            ticks: Vec::new(),
            dragging: None,
        };
        slider.value = slider.snap(slider.value);

        let (active, inactive) = tick_colors(b.active_color, b.inactive_color, s);
        let lit = slider.step_index();
        slider.ticks = (0..slider.steps)
            .map(|i| {
                let tick_style = LayoutStyle::default()
                    .width(px(b.tick_width))
                    .height(px(b.tick_height));
                s.spawn(
                    me,
                    div()
                        .style(tick_style)
                        .radius(b.tick_radius)
                        .background(if i < lit { active } else { inactive }),
                )
            })
            .collect();

        s.on::<Press>(me, on_press);
        s.on::<PressMoved>(me, on_press_moved);
        s.on::<Release>(me, |ctx, e: &Release| end_drag(ctx, e.contact));
        s.on::<PressCancelled>(me, |ctx, e: &PressCancelled| end_drag(ctx, e.contact));
        s.on_theme(me, sync_ticks);

        slider
    }
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

/// Snaps the pointer's position to the nearest step and emits on change.
fn set_from_pointer(ctx: &mut Context<'_, Slider>, pos: Point) {
    let range = ctx.me().max - ctx.me().min;
    if !range.is_finite() || range <= 0.0 {
        return;
    }
    let Some(content) = ctx.component::<ComputedLayout>().map(|l| l.content()) else {
        return;
    };
    let fraction = match ctx.me().orientation {
        SliderOrientation::Horizontal => {
            if content.width() <= 0.0 {
                return;
            }
            ((pos.x - content.x()) / content.width()).clamp(0.0, 1.0)
        }
        SliderOrientation::Vertical => {
            if content.height() <= 0.0 {
                return;
            }
            ((content.y() + content.height() - pos.y) / content.height()).clamp(0.0, 1.0)
        }
    };

    let me = ctx.me();
    let step_index = (fraction * me.steps as f32).round() as usize;
    if !me.select_step(step_index) {
        return;
    }
    let value = me.value;

    sync_ticks(ctx);
    let handle = ctx.handle();
    ctx.emit(SliderChange { value, step_index }, handle);
}

fn sync_ticks(ctx: &mut Context<'_, Slider>) {
    let (lit, count) = {
        let me = ctx.me();
        (me.step_index(), me.ticks.len())
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
    fn step_index(&mut self) -> usize;
    fn orientation(&mut self) -> SliderOrientation;
    /// Clamps and snaps to the nearest selectable step without emitting an event.
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

    fn step_index(&mut self) -> usize {
        self.me().step_index()
    }

    fn orientation(&mut self) -> SliderOrientation {
        self.me().orientation
    }

    fn set_value(&mut self, value: f32) {
        let me = self.me();
        let value = me.snap(value);
        if me.value != value {
            me.value = value;
            sync_ticks(self);
        }
    }

    fn set_range(&mut self, min: f32, max: f32) {
        let me = self.me();
        me.min = min;
        me.max = max;
        me.value = me.snap(me.value);
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

    fn slider_at(value: f32, min: f32, max: f32, steps: usize) -> Slider {
        Slider {
            size: SliderSize::Medium,
            value,
            min,
            max,
            steps,
            active_color: None,
            inactive_color: None,
            orientation: SliderOrientation::Horizontal,
            ticks: vec![],
            dragging: None,
        }
    }

    #[test]
    fn slider_fraction_and_step_index() {
        let mut s = slider_at(20.0, 0.0, 100.0, 50);
        assert_eq!(s.fraction(), 0.2);
        assert_eq!(s.step_index(), 10);

        s.value = 50.0;
        assert_eq!(s.fraction(), 0.5);
        assert_eq!(s.step_index(), 25);

        s.value = 100.0;
        assert_eq!(s.fraction(), 1.0);
        assert_eq!(s.step_index(), 50);
    }

    #[test]
    fn slider_tolerates_inverted_range() {
        let s = slider_at(0.0, 10.0, 0.0, 1);
        assert_eq!(s.clamp(5.0), 0.0);
        assert_eq!(s.fraction(), 0.0);
    }

    #[test]
    fn slider_snaps_values_to_selectable_steps() {
        let mut s = slider_at(0.0, 0.0, 100.0, 4);
        assert_eq!(s.snap(38.0), 50.0);
        assert_eq!(s.snap(-10.0), 0.0);
        assert_eq!(s.snap(200.0), 100.0);
        assert_eq!(s.snap(f32::NAN), 0.0);

        assert!(s.select_step(2));
        assert_eq!(s.value, 50.0);
        assert!(!s.select_step(2));
        assert!(s.select_step(4));
        assert_eq!(s.value, 100.0);
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
        let b = slider().range(0.0, 100.0).value(45.0).steps(40);

        assert_eq!(b.min, 0.0);
        assert_eq!(b.max, 100.0);
        assert_eq!(b.value, 45.0);
        assert_eq!(b.steps, 40);
        assert_eq!(b.size, SliderSize::Medium);
        assert_eq!(b.tick_width, 2.0);
        assert_eq!(b.tick_height, 40.0);
        assert_eq!(b.tick_gap, 4.0);
        assert_eq!(b.orientation, SliderOrientation::Horizontal);

        let bv = slider().vertical();
        assert_eq!(bv.orientation, SliderOrientation::Vertical);
        assert_eq!(bv.tick_width, 40.0);
        assert_eq!(bv.tick_height, 2.0);

        let bh = slider().vertical().horizontal();
        assert_eq!(bh.orientation, SliderOrientation::Horizontal);
        assert_eq!(bh.tick_width, 2.0);
        assert_eq!(bh.tick_height, 40.0);

        let custom = slider().tick_size(4.0, 32.0);
        assert_eq!(custom.tick_width, 4.0);
        assert_eq!(custom.tick_height, 32.0);
    }

    #[test]
    fn slider_size_and_variants() {
        let xs = slider().size(SliderSize::XSmall);
        assert_eq!(xs.size, SliderSize::XSmall);
        assert_eq!(xs.tick_height, 16.0);
        assert_eq!(xs.tick_width, 2.0);
        assert_eq!(xs.tick_gap, 4.0);

        let sm = slider().size(SliderSize::Small);
        assert_eq!(sm.size, SliderSize::Small);
        assert_eq!(sm.tick_height, 24.0);
        assert_eq!(sm.tick_width, 2.0);
        assert_eq!(sm.tick_gap, 4.0);

        let md = slider().size(SliderSize::Medium);
        assert_eq!(md.size, SliderSize::Medium);
        assert_eq!(md.tick_height, 40.0);
        assert_eq!(md.tick_width, 2.0);
        assert_eq!(md.tick_gap, 4.0);

        let lg = slider().size(SliderSize::Large);
        assert_eq!(lg.size, SliderSize::Large);
        assert_eq!(lg.tick_height, 56.0);
        assert_eq!(lg.tick_width, 2.0);
        assert_eq!(lg.tick_gap, 4.0);

        let xl = slider().size(SliderSize::XLarge);
        assert_eq!(xl.size, SliderSize::XLarge);
        assert_eq!(xl.tick_height, 96.0);
        assert_eq!(xl.tick_width, 2.0);
        assert_eq!(xl.tick_gap, 4.0);

        // variant method alias
        let v = slider().variant(SliderVariant::Large);
        assert_eq!(v.size, SliderSize::Large);

        // vertical orientation with specific size: swap width and height
        let v_lg = slider().size(SliderSize::Large).vertical();
        assert_eq!(v_lg.orientation, SliderOrientation::Vertical);
        assert_eq!(v_lg.tick_width, 56.0);
        assert_eq!(v_lg.tick_height, 2.0);

        // setting size after setting vertical orientation: swap width and height
        let v_lg2 = slider().vertical().size(SliderSize::Large);
        assert_eq!(v_lg2.orientation, SliderOrientation::Vertical);
        assert_eq!(v_lg2.tick_width, 56.0);
        assert_eq!(v_lg2.tick_height, 2.0);
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
