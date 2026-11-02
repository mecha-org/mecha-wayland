//! Drag resistance and the fling/spring simulation, in logical pixels
//! and seconds. Ported from Flutter's `BouncingScrollPhysics` and the
//! simulations under it (BSD-3, `references/flutter/packages/flutter/lib/src/`).

use app::prelude::*;

/// The iOS feel, as numbers. The defaults match UIKit as Flutter
/// approximates it; the example exposes them to tune by hand.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct Tuning {
    /// Distance a press moves before it becomes a drag, logical px.
    pub slop: f64,
    /// Release speeds below this, px/s, do not fling.
    pub min_fling: f64,
    /// Release speeds are clamped to this, px/s.
    pub max_fling: f64,
    /// Fraction of fling velocity left after one second.
    pub friction_drag: f64,
    /// Drag resistance at the edge of the range.
    pub overscroll_friction: f64,
    pub spring_mass: f64,
    pub spring_stiffness: f64,
    pub spring_ratio: f64,
    /// Seconds for a released overscroll to halve.
    pub rubber_band_half_life: f64,
    /// Fastest a fling may hit the spring at the end, px/s.
    pub max_spring_transfer: f64,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            slop: 10.0,
            min_fling: 100.0,
            max_fling: 8000.0,
            friction_drag: 0.135,
            overscroll_friction: 0.52,
            spring_mass: 0.5,
            spring_stiffness: 100.0,
            spring_ratio: 1.1,
            rubber_band_half_life: 0.07,
            max_spring_transfer: 5000.0,
        }
    }
}

impl Resource for Tuning {}

/// Offsets the content can rest at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Range {
    pub(crate) min: f64,
    pub(crate) max: f64,
}

impl Range {
    /// Content `extent` long seen through a `viewport`.
    pub(crate) fn new(extent: f64, viewport: f64) -> Self {
        Self {
            min: 0.0,
            max: (extent - viewport).max(0.0),
        }
    }

    pub(crate) fn contains(self, offset: f64) -> bool {
        offset >= self.min && offset <= self.max
    }
}

/// The offset after the finger moves `delta` from `offset`. Inside the
/// range the content follows 1:1; past an end it resists, more the
/// further out it is, and eases back with less resistance.
///
/// Port of `BouncingScrollPhysics.applyPhysicsToUserOffset` and
/// `_applyFriction` (`widgets/scroll_physics.dart`), with three changes:
/// a delta that leaves the range is split at the end, so only the part
/// beyond it resists (Flutter moves the whole delta 1:1); pulling further
/// out moves `delta * γ` outright, where Flutter also runs it through
/// `_applyFriction`'s limit, as it does easing back; and
/// Flutter's `offset` is a pixel decrease where `delta` here is an
/// increase.
pub(crate) fn drag(t: &Tuning, offset: f64, delta: f64, range: Range, viewport: f64) -> f64 {
    if delta == 0.0 {
        return offset;
    }
    if viewport <= 0.0 {
        return (offset + delta).clamp(range.min, range.max);
    }
    if range.contains(offset) {
        let to = offset + delta;
        return if to > range.max {
            range.max + (to - range.max) * friction_factor(t, 0.0)
        } else if to < range.min {
            range.min - (range.min - to) * friction_factor(t, 0.0)
        } else {
            to
        };
    }
    let past_start = (range.min - offset).max(0.0);
    let past_end = (offset - range.max).max(0.0);
    let past = past_start.max(past_end);
    let easing = (past_start > 0.0 && delta > 0.0) || (past_end > 0.0 && delta < 0.0);
    let abs = delta.abs();
    if !easing {
        return offset + delta * friction_factor(t, past / viewport);
    }
    // Less resistance easing back than pulling out.
    let gamma = friction_factor(t, (past - abs) / viewport);
    let to_limit = past / gamma;
    if abs < to_limit {
        return offset + delta.signum() * abs * gamma;
    }
    // Back at the end with some delta left: the rest moves as in range.
    let end = if past_end > 0.0 { range.max } else { range.min };
    drag(t, end, delta.signum() * (abs - to_limit), range, viewport)
}

/// `BouncingScrollPhysics.frictionFactor` (`widgets/scroll_physics.dart`).
fn friction_factor(t: &Tuning, overscroll_fraction: f64) -> f64 {
    t.overscroll_friction * (1.0 - overscroll_fraction).powi(2)
}

/// `ScrollPhysics.toleranceFor` (`widgets/scroll_physics.dart`).
#[derive(Clone, Copy, Debug)]
struct Tolerance {
    distance: f64,
    velocity: f64,
}

impl Tolerance {
    fn new(dpr: f64) -> Self {
        Self {
            distance: 1.0 / dpr,
            velocity: 1.0 / (0.05 * dpr),
        }
    }
}

/// The motion after a release: the fling coasts under friction and, if
/// it would cross an end, hands over to a spring that settles on it; a
/// release outside the range springs back at once.
///
/// Port of `BouncingScrollSimulation` (`widgets/scroll_simulation.dart`)
/// as `BouncingScrollPhysics.createBallisticSimulation` builds it.
#[derive(Clone, Debug)]
pub(crate) struct Ballistic {
    friction: Option<Friction>,
    /// The spring and the time it takes over; `-inf` from the start.
    spring: Option<(f64, Spring)>,
}

impl Ballistic {
    /// `None` when there is nothing to animate: at rest inside the range.
    pub(crate) fn new(
        t: &Tuning,
        offset: f64,
        velocity: f64,
        range: Range,
        dpr: f64,
    ) -> Option<Ballistic> {
        let tolerance = Tolerance::new(dpr);
        let stationary = velocity.abs() < tolerance.velocity;
        let inside = range.contains(offset);
        if stationary && inside {
            return None;
        }
        let spring = if stationary {
            rubber_band_spring(t)
        } else {
            SpringDescription::with_damping_ratio(t.spring_mass, t.spring_stiffness, t.spring_ratio)
        };
        if !inside {
            let end = offset.clamp(range.min, range.max);
            let s = Spring::new(spring, offset, end, velocity, tolerance);
            return Some(Ballistic {
                friction: None,
                spring: Some((f64::NEG_INFINITY, s)),
            });
        }
        let friction = Friction::new(t.friction_drag, offset, velocity, tolerance);
        let final_x = friction.final_x();
        let end = if velocity > 0.0 && final_x > range.max {
            Some(range.max)
        } else if velocity < 0.0 && final_x < range.min {
            Some(range.min)
        } else {
            None
        };
        let spring = end.and_then(|end| {
            let time = friction.time_at_x(end);
            // Coasted to rest before reaching the end: never springs.
            if friction.is_done(time) {
                return None;
            }
            let cap = t.max_spring_transfer;
            let v = friction.dx(time).clamp(-cap, cap);
            Some((time, Spring::new(spring, end, end, v, tolerance)))
        });
        Some(Ballistic {
            friction: Some(friction),
            spring,
        })
    }

    pub(crate) fn x(&self, time: f64) -> f64 {
        match self.phase(time) {
            Phase::Friction(f) => f.x(time),
            Phase::Spring(start, s) => s.x(time - start),
        }
    }

    pub(crate) fn dx(&self, time: f64) -> f64 {
        match self.phase(time) {
            Phase::Friction(f) => f.dx(time),
            Phase::Spring(start, s) => s.dx(time - start),
        }
    }

    /// Once true, `x` holds the rest position from then on.
    pub(crate) fn is_done(&self, time: f64) -> bool {
        match self.phase(time) {
            Phase::Friction(f) => f.is_done(time),
            Phase::Spring(start, s) => s.is_done(time - start),
        }
    }

    /// `BouncingScrollSimulation._simulation`: the active part and the
    /// time it started.
    fn phase(&self, time: f64) -> Phase<'_> {
        match (&self.spring, &self.friction) {
            (Some((start, s)), _) if time > *start => Phase::Spring(start.max(0.0), s),
            (_, Some(f)) => Phase::Friction(f),
            (Some((start, s)), None) => Phase::Spring(start.max(0.0), s),
            (None, None) => unreachable!("a ballistic has friction or a spring"),
        }
    }
}

enum Phase<'a> {
    Friction(&'a Friction),
    Spring(f64, &'a Spring),
}

/// `BouncingScrollPhysics.rubberBandSpring` (`widgets/scroll_physics.dart`):
/// an overdamped spring with roots `-λ` and `-1e5·λ`, so a stationary
/// overscroll decays as `e^(-λt)` and halves every `half_life`.
fn rubber_band_spring(t: &Tuning) -> SpringDescription {
    let lambda = std::f64::consts::LN_2 / t.rubber_band_half_life;
    SpringDescription {
        mass: 1.0,
        stiffness: 1e5 * lambda * lambda,
        damping: (1e5 + 1.0) * lambda,
    }
}

/// `FrictionSimulation` (`physics/friction_simulation.dart`) without
/// constant deceleration: velocity falls to `drag` of itself each second.
#[derive(Clone, Debug)]
struct Friction {
    drag: f64,
    drag_log: f64,
    x: f64,
    v: f64,
    /// When the speed drops under tolerance; `x` holds still after it.
    done_time: f64,
}

impl Friction {
    fn new(drag: f64, x: f64, v: f64, tolerance: Tolerance) -> Self {
        let drag_log = drag.ln();
        // Solves |v|·drag^t = tolerance.velocity.
        let done_time = if v.abs() < tolerance.velocity {
            0.0
        } else {
            (tolerance.velocity / v.abs()).ln() / drag_log
        };
        Self {
            drag,
            drag_log,
            x,
            v,
            done_time,
        }
    }

    fn x(&self, time: f64) -> f64 {
        let time = time.min(self.done_time);
        self.x + self.v * self.drag.powf(time) / self.drag_log - self.v / self.drag_log
    }

    fn dx(&self, time: f64) -> f64 {
        self.v * self.drag.powf(time)
    }

    fn final_x(&self) -> f64 {
        self.x - self.v / self.drag_log
    }

    /// When `x` reaches `target`, which lies between the start and
    /// `final_x`. Flutter iterates Newton's method; this inverts `x`.
    fn time_at_x(&self, target: f64) -> f64 {
        if target == self.x {
            return 0.0;
        }
        (1.0 + (target - self.x) * self.drag_log / self.v).ln() / self.drag_log
    }

    fn is_done(&self, time: f64) -> bool {
        time >= self.done_time
    }
}

/// `SpringDescription` (`physics/spring_simulation.dart`).
#[derive(Clone, Copy, Debug)]
struct SpringDescription {
    mass: f64,
    stiffness: f64,
    damping: f64,
}

impl SpringDescription {
    fn with_damping_ratio(mass: f64, stiffness: f64, ratio: f64) -> Self {
        Self {
            mass,
            stiffness,
            damping: ratio * 2.0 * (mass * stiffness).sqrt(),
        }
    }
}

/// `ScrollSpringSimulation` (`physics/spring_simulation.dart`): a spring
/// from `start` to `end` whose `x` snaps to `end` once done.
#[derive(Clone, Debug)]
struct Spring {
    end: f64,
    solution: Solution,
    tolerance: Tolerance,
}

impl Spring {
    fn new(
        spring: SpringDescription,
        start: f64,
        end: f64,
        velocity: f64,
        tolerance: Tolerance,
    ) -> Self {
        Self {
            end,
            solution: Solution::new(spring, start - end, velocity),
            tolerance,
        }
    }

    fn x(&self, time: f64) -> f64 {
        if self.is_done(time) {
            self.end
        } else {
            self.end + self.solution.x(time)
        }
    }

    fn dx(&self, time: f64) -> f64 {
        self.solution.dx(time)
    }

    fn is_done(&self, time: f64) -> bool {
        self.solution.x(time).abs() < self.tolerance.distance
            && self.solution.dx(time).abs() < self.tolerance.velocity
    }
}

/// `_SpringSolution` and its three cases (`physics/spring_simulation.dart`),
/// as displacement from the end.
#[derive(Clone, Debug)]
enum Solution {
    Critical { r: f64, c1: f64, c2: f64 },
    Over { r1: f64, r2: f64, c1: f64, c2: f64 },
    Under { w: f64, r: f64, c1: f64, c2: f64 },
}

impl Solution {
    fn new(s: SpringDescription, distance: f64, velocity: f64) -> Self {
        let cmk = s.damping * s.damping - 4.0 * s.mass * s.stiffness;
        if cmk > 0.0 {
            let r1 = (-s.damping - cmk.sqrt()) / (2.0 * s.mass);
            let r2 = (-s.damping + cmk.sqrt()) / (2.0 * s.mass);
            let c2 = (velocity - r1 * distance) / (r2 - r1);
            Solution::Over {
                r1,
                r2,
                c1: distance - c2,
                c2,
            }
        } else if cmk < 0.0 {
            let w = (4.0 * s.mass * s.stiffness - s.damping * s.damping).sqrt() / (2.0 * s.mass);
            let r = -(s.damping / 2.0 / s.mass);
            Solution::Under {
                w,
                r,
                c1: distance,
                c2: (velocity - r * distance) / w,
            }
        } else {
            let r = -s.damping / (2.0 * s.mass);
            Solution::Critical {
                r,
                c1: distance,
                c2: velocity - r * distance,
            }
        }
    }

    fn x(&self, t: f64) -> f64 {
        match *self {
            Solution::Critical { r, c1, c2 } => (c1 + c2 * t) * (r * t).exp(),
            Solution::Over { r1, r2, c1, c2 } => c1 * (r1 * t).exp() + c2 * (r2 * t).exp(),
            Solution::Under { w, r, c1, c2 } => {
                (r * t).exp() * (c1 * (w * t).cos() + c2 * (w * t).sin())
            }
        }
    }

    fn dx(&self, t: f64) -> f64 {
        match *self {
            Solution::Critical { r, c1, c2 } => {
                let power = (r * t).exp();
                r * (c1 + c2 * t) * power + c2 * power
            }
            Solution::Over { r1, r2, c1, c2 } => {
                c1 * r1 * (r1 * t).exp() + c2 * r2 * (r2 * t).exp()
            }
            Solution::Under { w, r, c1, c2 } => {
                let power = (r * t).exp();
                let (sin, cos) = (w * t).sin_cos();
                power * (c2 * w * cos - c1 * w * sin) + r * power * (c2 * sin + c1 * cos)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: f64 = 800.0;

    fn range(max: f64) -> Range {
        Range { min: 0.0, max }
    }

    #[test]
    fn drag_is_one_to_one_in_range() {
        let t = Tuning::default();
        assert_eq!(drag(&t, 100.0, 50.0, range(1000.0), VIEWPORT), 150.0);
        assert_eq!(drag(&t, 100.0, -60.0, range(1000.0), VIEWPORT), 40.0);
        assert_eq!(drag(&t, 0.0, 1000.0, range(1000.0), VIEWPORT), 1000.0);
    }

    #[test]
    fn drag_resists_past_the_end() {
        let t = Tuning::default();
        let max = 1000.0;
        let to = drag(&t, max, 100.0, range(max), VIEWPORT);
        assert!(to > max && to <= max + 52.0, "{to}");
        let to = drag(&t, 0.0, -100.0, range(max), VIEWPORT);
        assert!(to < 0.0 && to >= -52.0, "{to}");
        // Only the part beyond the end resists.
        let to = drag(&t, max - 50.0, 100.0, range(max), VIEWPORT);
        assert!(to > max && to <= max + 26.0, "{to}");
    }

    #[test]
    fn drag_eases_back_without_resistance_curve_flip() {
        let t = Tuning::default();
        let max = 1000.0;
        for over in [10.0, 100.0, 300.0] {
            let from = max + over;
            let out = drag(&t, from, 20.0, range(max), VIEWPORT) - from;
            let back = from - drag(&t, from, -20.0, range(max), VIEWPORT);
            assert!(
                out > 0.0 && back >= out,
                "over {over}: out {out}, back {back}"
            );
        }
        // Easing all the way back moves 1:1 once in range.
        let to = drag(&t, max + 10.0, -500.0, range(max), VIEWPORT);
        assert!(to < max - 400.0 && to > max - 500.0, "{to}");
    }

    #[test]
    fn fling_decelerates_like_ios() {
        let t = Tuning::default();
        let b = Ballistic::new(&t, 0.0, 1000.0, range(1e6), 1.0).unwrap();
        let v = b.dx(1.0);
        assert!((v - 1000.0 * 0.135).abs() <= 1000.0 * 0.135 * 0.01, "{v}");
        let mut last = b.x(0.0);
        let mut time = 0.0;
        while !b.is_done(time) {
            time += 1.0 / 120.0;
            assert!(time < 10.0, "never done");
            let x = b.x(time);
            assert!(x >= last, "not monotone at {time}: {x} < {last}");
            last = x;
        }
        assert_eq!(b.x(time + 1.0), b.x(time));
    }

    #[test]
    fn fling_past_the_end_springs_back_to_it() {
        let t = Tuning::default();
        let max = 1000.0;
        let b = Ballistic::new(&t, max - 100.0, 3000.0, range(max), 1.0).unwrap();
        let mut peak = f64::MIN;
        let mut time = 0.0;
        while !b.is_done(time) {
            time += 1.0 / 120.0;
            assert!(time < 10.0, "never done");
            peak = peak.max(b.x(time));
        }
        assert!(peak > max, "{peak}");
        assert_eq!(b.x(time), max);
        assert_eq!(b.x(time + 1.0), max);
    }

    #[test]
    fn stationary_overscroll_release_decays() {
        let t = Tuning::default();
        let max = 1000.0;
        let b = Ballistic::new(&t, max + 100.0, 0.0, range(max), 1.0).unwrap();
        let half = (0..1000)
            .map(|i| f64::from(i) * 0.0005)
            .find(|&time| b.x(time) <= max + 50.0)
            .unwrap();
        assert!((half - 0.07).abs() <= 0.007, "{half}");
        let mut time = 0.0;
        while !b.is_done(time) {
            time += 1.0 / 120.0;
            assert!(time < 10.0, "never done");
            assert!(b.x(time) >= max, "undershot at {time}");
        }
        assert_eq!(b.x(time), max);
    }

    #[test]
    fn at_rest_in_range_is_none() {
        let t = Tuning::default();
        assert!(Ballistic::new(&t, 500.0, 0.0, range(1000.0), 1.0).is_none());
        assert!(Ballistic::new(&t, 500.0, 5.0, range(1000.0), 2.0).is_none());
        assert!(Ballistic::new(&t, 1000.0, 0.0, range(1000.0), 1.0).is_none());
    }
}
