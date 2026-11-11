#![forbid(unsafe_code)]
//! Duration- or speed-based transitions from Taffy's [`ComputedLayout`] to displayed [`Layout`].
//!
//! Install after `LayoutModule` and `WindowModule`, and before `RenderModule`.
//! This module maintains all displayed layouts: nonanimated nodes copy their
//! targets, while animated nodes interpolate.
//! Layout and paint settings inherit independently from the nearest configured
//! ancestor; a zero duration overrides inheritance and snaps that property.
//! Layout roots always snap. The first layout resolution is copied immediately
//! by the animation module. Later resolutions, including a compositor resize
//! during startup, can start transitions.
//!
//! After each layout pass, changed targets start transitions and active windows
//! request frames. On `Frame`, the clock is sampled and displayed geometry is
//! advanced before rendering. A replacement target restarts from the currently
//! displayed layout, without velocity matching. Hit testing still uses targets.
//!
//! # Scheduling
//!
//! There are two separate jobs, not two ways to advance an animation:
//!
//! - [`LayoutDone`](layout::LayoutDone) means the tick's layout pass has finished. Its system
//!   reconciles targets and settings with existing transitions, copies nodes
//!   that should not animate, and requests frames for windows needing work.
//!   The signal is sent even when no root needed recomputing, so disabling
//!   animation and continuing existing transitions do not require a new layout.
//! - [`Frame`] names one window with a drawing opportunity. Its system samples
//!   existing transitions into `Layout` before the renderer reads it. This is
//!   not a notification that pixels have reached the display.
//!
//! The normal order, with the modules installed as described above, is:
//!
//! ```text
//! PostTick:   layout resolves targets and queues LayoutDone; Time is sampled
//! LayoutDone: copy nonanimated targets, prepare transitions, notify, request frames
//! Frame(w):   sample Time, advance w's transitions, then render w
//! ```
//!
//! These are queued signals, not a fixed-rate clock or necessarily adjacent
//! operations. The platform decides when requested frames can run. [`Time`]
//! only samples the clock; it does not install a timer or wake the runner.

use std::time::Duration;

use app::prelude::*;
use window::Frame;

mod animatable;
mod context;
mod displayed_layout;
mod easing;
mod paint;
mod time;

pub use context::AnimationContext;
pub use displayed_layout::Layout;
pub use easing::Easing;
pub use paint::{AnimatedPaint, PaintTransition};
pub use time::Time;

/// The animation types normally imported by a consumer.
pub mod prelude {
    pub use crate::{
        AnimatedPaint, AnimationContext, AnimationModule, AnimationTime, Easing, Layout,
        LayoutAnimationSettings, PaintAnimationSettings, PaintTransition, Time,
    };
}

/// How a transition's duration is determined.
#[derive(Debug, Clone, Copy)]
pub enum AnimationTime {
    /// Elapsed monotonic time from a transition's start to its exact target.
    /// Zero means copy immediately rather than start a transition.
    Duration(Duration),
    /// Nominal units per second, finite and strictly positive.
    ///
    /// Duration is the largest absolute change among a value's numeric fields
    /// divided by this speed. For layout the fields are logical pixels; for
    /// quads they include both colors' RGBA channels (`0.0..=1.0`), corner
    /// radii and border widths (logical pixels). All fields share that duration;
    /// diagonal position changes use the largest axis change, not path length.
    /// Easing still applies, so only linear easing gives constant field rates.
    /// Durations beyond the representable range saturate at `Duration::MAX`.
    Speed(f32),
}

/// A node's optional explicit layout animation configuration.
///
/// Every live node has this component once [`AnimationModule`] is installed.
/// `Default` contains `None`, meaning "inherit", not "disable animation".
/// The nearest explicit layout settings on the node or its ancestors win,
/// independently of [`PaintAnimationSettings`]. If none exist, the node's
/// displayed layout copies its target without animating.
///
/// [`LayoutAnimationSettings::new`] contains `Some(settings)`, even for a zero
/// duration. An explicit zero duration therefore overrides an animated parent
/// and snaps; descendants inherit that choice unless they override it again.
#[derive(Debug, Clone, Copy, Default)]
pub struct LayoutAnimationSettings(Option<Settings>);

impl Component for LayoutAnimationSettings {}

impl LayoutAnimationSettings {
    /// Set this node's layout timing and easing, overriding inherited settings.
    ///
    /// `easing` converts normalized elapsed time to interpolation progress.
    /// Its output is not clamped, allowing overshoot. At completion, the exact
    /// target is assigned regardless of the easing curve. Use [`Self::custom`]
    /// for a function or noncapturing closure.
    ///
    /// # Panics
    ///
    /// Panics if a speed is zero, negative, or nonfinite. Use zero duration to
    /// disable animation rather than zero speed.
    ///
    /// ```
    /// use animation::{LayoutAnimationSettings, AnimationTime, Easing};
    /// use std::time::Duration;
    ///
    /// let settings = LayoutAnimationSettings::new(
    ///     AnimationTime::Duration(Duration::from_millis(1500)),
    ///     Easing::EaseInQuad,
    /// );
    /// ```
    pub fn new(time: AnimationTime, easing: Easing) -> Self {
        Self(Some(Settings::new(time, easing)))
    }

    /// Configure layout animation with a function or noncapturing closure.
    ///
    /// ```
    /// use animation::{AnimationTime, LayoutAnimationSettings};
    /// use std::time::Duration;
    ///
    /// let settings = LayoutAnimationSettings::custom(
    ///     AnimationTime::Duration(Duration::from_millis(500)),
    ///     |t| t * t,
    /// );
    /// ```
    pub fn custom(time: AnimationTime, easing: fn(f32) -> f32) -> Self {
        Self::new(time, Easing::Custom(easing))
    }
}

/// A node's optional explicit paint animation configuration.
///
/// `Default` means inherit the nearest explicit paint settings, independently
/// of [`LayoutAnimationSettings`]. An explicit zero duration overrides paint
/// inheritance and snaps. Only quad-to-quad paint changes can animate.
#[derive(Debug, Clone, Copy, Default)]
pub struct PaintAnimationSettings(Option<Settings>);

impl Component for PaintAnimationSettings {}

impl PaintAnimationSettings {
    /// Set this node's paint timing and easing, overriding inherited settings.
    ///
    /// Easing receives normalized elapsed time and may overshoot; completion
    /// still assigns the exact target. Use [`Self::custom`] for a function or
    /// noncapturing closure.
    ///
    /// # Panics
    ///
    /// Panics if a speed is zero, negative, or nonfinite. Use zero duration to
    /// disable paint animation rather than zero speed.
    pub fn new(time: AnimationTime, easing: Easing) -> Self {
        Self(Some(Settings::new(time, easing)))
    }

    /// Configure paint animation with a function or noncapturing closure.
    pub fn custom(time: AnimationTime, easing: fn(f32) -> f32) -> Self {
        Self::new(time, Easing::Custom(easing))
    }
}

/// Concrete configuration with no inheritance state. A running transition
/// captures its resolved duration and easing until it ends or is replaced.
#[derive(Debug, Clone, Copy)]
struct Settings {
    time: AnimationTime,
    easing: Easing,
}

impl Settings {
    fn new(time: AnimationTime, easing: Easing) -> Self {
        if let AnimationTime::Speed(speed) = time {
            assert!(
                speed.is_finite() && speed > 0.0,
                "animation speed must be finite and strictly positive"
            );
        }
        Self { time, easing }
    }
}

/*
TODO:

Maybe this trait can be removed. We only use it for the settings() function

Either way, both are self.0. It is useful to distinguish a settings type, if
only it was used globally and had a global meaning

Perhaps we can make this kind of like a "private component, don't touch" trait.
We can then mark the Transition structs as SettingsComponent(s).
*/
trait SettingsComponent: Component {
    fn explicit(&self) -> Option<Settings>;
}

impl SettingsComponent for LayoutAnimationSettings {
    fn explicit(&self) -> Option<Settings> {
        self.0
    }
}

impl SettingsComponent for PaintAnimationSettings {
    fn explicit(&self) -> Option<Settings> {
        self.0
    }
}

/// Installs the clock, settings and transition components, and animation systems.
///
/// Install after `layout::LayoutModule` and `window::WindowModule`, and before
/// the renderer. Layout only produces targets; this module initializes and
/// maintains their displayed values.
///
/// The systems sample [`Time`] on [`PostTick`] and [`Frame`], reconcile layout
/// targets on [`LayoutDone`], and prepare paint transitions when target paint
/// changes. Both kinds of transitions advance on `Frame` before rendering.
/// There is no independent timer or frame-rate loop here.
pub struct AnimationModule;
impl Module for AnimationModule {
    /// Register displayed layouts and the systems in execution order.
    fn install(self, app: &mut App) {
        // Nodes spawned before this module (at least the root) never send
        // it a `Spawned`; they start unresolved like any other.
        let existing: Vec<NodeId> = std::iter::once(app.root())
            .chain(app.descendants(app.root()))
            .collect();
        app.init_resource::<displayed_layout::Unresolved>();
        app.resource_mut::<displayed_layout::Unresolved>()
            .0
            .extend(existing);
        app.init_resource::<Time>()
            .system::<PostTick>(time::update_time)
            .system::<Frame>(time::update_time)
            .register_component::<LayoutAnimationSettings>()
            .register_component::<PaintAnimationSettings>()
            .register_component::<Layout>()
            .register_component::<displayed_layout::Transition>()
            .register_component::<AnimatedPaint>()
            .register_component::<PaintTransition>()
            .init_resource::<displayed_layout::Unresolved>()
            .init_resource::<displayed_layout::Active>()
            .system(paint::on_spawned)
            .system(displayed_layout::on_spawned)
            .system(displayed_layout::on_moved)
            .system(displayed_layout::on_layout_done)
            .system(displayed_layout::on_target_changed)
            .system(paint::on_paint_changed)
            .system(displayed_layout::on_frame)
            .system(paint::on_frame);
    }
}

/// Resolve a live node's explicit settings for one property, then its nearest
/// ancestor's.
/// Siblings are never consulted. Zero-duration settings still stop the search;
/// only `None` means continue inheriting. Nothing is copied into child settings.
fn settings<C: SettingsComponent>(app: &App, id: NodeId) -> Option<Settings> {
    std::iter::once(id)
        .chain(app.ancestors(id))
        .find_map(|node| app.component::<C>(node)?.explicit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_speeds_are_rejected() {
        for speed in [0.0, -0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(
                std::panic::catch_unwind(|| {
                    LayoutAnimationSettings::custom(AnimationTime::Speed(speed), |t| t)
                })
                .is_err(),
                "accepted invalid speed {speed}"
            );
        }
    }
}
