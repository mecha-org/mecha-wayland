//! Release velocity from recent drag positions, the way an iOS scroll
//! view estimates it.

/// Samples kept, as in Flutter.
const SAMPLES: usize = 20;

/// A finger still for longer than this has stopped
/// (`VelocityTracker._assumePointerMoveStoppedMilliseconds`).
const STOPPED_MS: u32 = 40;

/// Port of `IOSScrollViewFlingVelocityTracker`
/// (`references/flutter/packages/flutter/lib/src/gestures/velocity_tracker.dart`)
/// along one axis. Times are compositor milliseconds, compared with
/// `wrapping_sub` so the 32-bit clock may wrap mid-drag. Public only so
/// the example can log the release speed a `Scrollable` reads.
#[doc(hidden)]
#[derive(Clone, Debug, Default)]
pub struct VelocityTracker {
    samples: [Option<Sample>; SAMPLES],
    /// The newest sample.
    index: usize,
}

#[derive(Clone, Copy, Debug)]
struct Sample {
    time_ms: u32,
    position: f64,
}

impl VelocityTracker {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn add(&mut self, time_ms: u32, position: f64) {
        self.index = (self.index + 1) % SAMPLES;
        self.samples[self.index] = Some(Sample { time_ms, position });
    }

    /// Px/s at `now_ms`; `0` with no samples or when the newest is stale.
    pub fn velocity(&self, now_ms: u32) -> f64 {
        let Some(newest) = self.samples[self.index] else {
            return 0.0;
        };
        if now_ms.wrapping_sub(newest.time_ms) > STOPPED_MS {
            return 0.0;
        }
        // Weighted toward the older pairs: it approximates the scroll
        // view's speed at release, which lags the finger.
        0.6 * self.pair(-2) + 0.35 * self.pair(-1) + 0.05 * self.pair(0)
    }

    /// `_previousVelocityAt`: px/s between the sample `back` before the
    /// newest (`0` is the newest) and the one before that.
    fn pair(&self, back: isize) -> f64 {
        let at = |i: isize| {
            let i = (self.index as isize + i).rem_euclid(SAMPLES as isize) as usize;
            self.samples[i]
        };
        let (Some(end), Some(start)) = (at(back), at(back - 1)) else {
            return 0.0;
        };
        let dt = end.time_ms.wrapping_sub(start.time_ms);
        if dt == 0 {
            return 0.0;
        }
        (end.position - start.position) * 1000.0 / f64::from(dt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steady(start_ms: u32) -> VelocityTracker {
        let mut v = VelocityTracker::default();
        for i in 0..10u32 {
            v.add(start_ms.wrapping_add(i * 8), f64::from(i) * 8.0);
        }
        v
    }

    #[test]
    fn velocity_of_a_steady_drag() {
        let v = steady(1000);
        let px_s = v.velocity(1000 + 9 * 8);
        assert!((px_s - 1000.0).abs() < 1e-9, "{px_s}");
    }

    #[test]
    fn stale_samples_give_zero() {
        let v = steady(1000);
        assert_eq!(v.velocity(1000 + 9 * 8 + 50), 0.0);
    }

    #[test]
    fn same_time_samples_are_ignored() {
        let mut v = VelocityTracker::default();
        for i in 0..6 {
            v.add(100, f64::from(i) * 10.0);
        }
        let px_s = v.velocity(100);
        assert!(px_s.is_finite(), "{px_s}");
    }

    #[test]
    fn timestamps_wrap() {
        let start = u32::MAX - 20;
        let wrapped = steady(start).velocity(start.wrapping_add(9 * 8));
        let plain = steady(1000).velocity(1000 + 9 * 8);
        assert_eq!(wrapped, plain);
    }

    #[test]
    fn reset_forgets_samples() {
        let mut v = steady(1000);
        v.reset();
        assert_eq!(v.velocity(1000 + 9 * 8), 0.0);
    }
}
