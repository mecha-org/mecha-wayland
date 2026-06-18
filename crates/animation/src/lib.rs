mod easing;

use std::time::Duration;

pub use easing::Easing;

fn monotonic_now() -> Duration {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

/// Perceptually-uniform sRGB color interpolation via Oklab.
/// `t` is 0.0→1.0 progress. All channels expected in [0, 1].
#[inline]
pub fn lerp_color(
    (r1, g1, b1): (f32, f32, f32),
    (r2, g2, b2): (f32, f32, f32),
    t: f32,
) -> (f32, f32, f32) {
    let from = oklab::srgb_f32_to_oklab(oklab::Rgb {
        r: r1,
        g: g1,
        b: b1,
    });
    let to = oklab::srgb_f32_to_oklab(oklab::Rgb {
        r: r2,
        g: g2,
        b: b2,
    });
    let l = from.l + (to.l - from.l) * t;
    let a = from.a + (to.a - from.a) * t;
    let b = from.b + (to.b - from.b) * t;
    let c = oklab::oklab_to_srgb_f32(oklab::Oklab { l, a, b });
    (c.r, c.g, c.b)
}

// ── Animatable ────────────────────────────────────────────────────────────────

pub trait Animatable: Clone {
    fn lerp(start: &Self, end: &Self, t: f32) -> Self;
}

impl Animatable for f32 {
    fn lerp(start: &Self, end: &Self, t: f32) -> Self {
        start + (end - start) * t
    }
}

impl Animatable for utils::Color {
    fn lerp(start: &Self, end: &Self, t: f32) -> Self {
        let (r, g, b) = lerp_color((start.r, start.g, start.b), (end.r, end.g, end.b), t);
        utils::Color {
            r,
            g,
            b,
            a: start.a + (end.a - start.a) * t,
        }
    }
}

// ── AnimationConfig ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct AnimationConfig {
    pub duration: Duration,
    pub easing: Easing,
    pub delay: Duration,
}

impl AnimationConfig {
    pub fn new(duration: Duration, easing: Easing) -> Self {
        AnimationConfig::with_delay(duration, easing, Duration::ZERO)
    }

    pub fn with_delay(duration: Duration, easing: Easing, delay: Duration) -> Self {
        Self {
            duration,
            easing,
            delay,
        }
    }
}

impl Default for AnimationConfig {
    fn default() -> Self {
        Self {
            duration: Duration::from_millis(300),
            easing: Easing::EaseInOut,
            delay: Duration::ZERO,
        }
    }
}

// ── RepeatMode ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub enum RepeatMode {
    None,
    PingPong {
        /// Pause between cycles. The animation runs forward then backward
        /// (2 × duration), then idles for `interval`. During the interval
        /// `is_animating()` returns false and `resume_deadline()` returns the
        /// monotonic deadline when the next cycle begins.
        interval: Duration,
    },
}

// ── Animated<T> ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Animated<T: Animatable> {
    from: T,
    to: T,
    easing: Easing,
    repeat: RepeatMode,
    delay: Duration,
    duration: Duration,
    started_at: Duration,
}

impl<T: Animatable> Animated<T> {
    /// One-shot animation from `from` to `to` using the given timing config.
    pub fn new(from: T, to: T, config: AnimationConfig) -> Self {
        Self {
            from,
            to,
            easing: config.easing,
            repeat: RepeatMode::None,
            delay: config.delay,
            duration: config.duration,
            started_at: monotonic_now(),
        }
    }

    /// Repeating ping-pong animation. Runs from→to→from, then pauses for
    /// `interval` before the next cycle.
    pub fn new_pingpong(
        from: T,
        to: T,
        duration: Duration,
        easing: Easing,
        interval: Duration,
    ) -> Self {
        Self {
            from,
            to,
            easing,
            repeat: RepeatMode::PingPong { interval },
            delay: Duration::ZERO,
            duration,
            started_at: monotonic_now(),
        }
    }

    /// No animation — always returns `value`.
    pub fn static_value(value: T) -> Self {
        Self {
            from: value.clone(),
            to: value,
            easing: Easing::Linear,
            repeat: RepeatMode::None,
            delay: Duration::ZERO,
            duration: Duration::ZERO,
            started_at: Duration::ZERO,
        }
    }

    // ── queries ────────────────────────────────────────────────────────────

    /// Current interpolated value computed from `monotonic_now()`.
    pub fn get(&self) -> T {
        let now = monotonic_now();
        self.value(now)
    }

    /// Whether the animation is currently in an active cycle.
    pub fn is_animating(&self) -> bool {
        let now = monotonic_now();
        self.is_in_cycle(now)
    }

    /// Whether the animation has completed and has no pending resumes.
    pub fn is_finished(&self) -> bool {
        !self.is_animating() && self.compute_resume_deadline(monotonic_now()).is_none()
    }

    /// Absolute monotonic deadline when a paused animation resumes, or `None`
    /// if currently active or permanently finished.
    pub fn resume_deadline(&self) -> Option<Duration> {
        let now = monotonic_now();
        self.compute_resume_deadline(now)
    }

    // ── mutations ──────────────────────────────────────────────────────────

    /// Animate from the current interpolated value to a new target, retaining
    /// existing timing settings.
    pub fn set_target(&mut self, to: T) {
        self.from = self.get();
        self.to = to;
        self.started_at = monotonic_now();
    }

    /// Animate from the current value to a new target with fresh timing config.
    pub fn animate_to(&mut self, to: T, config: AnimationConfig) {
        self.from = self.get();
        self.to = to;
        self.easing = config.easing;
        self.duration = config.duration;
        self.delay = config.delay;
        self.started_at = monotonic_now();
    }

    /// Restart the animation from the original `from` value.
    pub fn reset(&mut self) {
        self.started_at = monotonic_now();
    }

    // ── internals ──────────────────────────────────────────────────────────

    fn active_from(&self) -> Duration {
        self.started_at + self.delay
    }

    fn progress(&self, now: Duration) -> Option<f32> {
        let active_from = self.active_from();
        if now < active_from {
            return None;
        }
        if self.duration.is_zero() {
            return Some(1.0);
        }
        let elapsed = (now - active_from).as_secs_f32();
        let t = (elapsed / self.duration.as_secs_f32()).clamp(0.0, 1.0);
        Some(self.easing.apply(t))
    }

    fn value(&self, now: Duration) -> T {
        if self.duration.is_zero() {
            return self.from.clone();
        }
        match self.repeat {
            RepeatMode::PingPong { interval } => {
                let active_from = self.active_from();
                if now < active_from {
                    return self.from.clone();
                }
                let cycle_ns = (2 * self.duration + interval).as_nanos();
                let elapsed_ns = (now - active_from).as_nanos() as u128;
                let phase_ns = elapsed_ns % cycle_ns;
                let active_end_ns = (2 * self.duration).as_nanos();
                if phase_ns >= active_end_ns {
                    return self.from.clone();
                }
                let half_ns = self.duration.as_nanos();
                if phase_ns < half_ns {
                    let t = (phase_ns as f64 / half_ns as f64) as f32;
                    let eased = self.easing.apply(t);
                    T::lerp(&self.from, &self.to, eased)
                } else {
                    let reverse_elapsed = phase_ns - half_ns;
                    let t = (reverse_elapsed as f64 / half_ns as f64) as f32;
                    let eased = self.easing.apply(t);
                    T::lerp(&self.to, &self.from, eased)
                }
            }
            RepeatMode::None => match self.progress(now) {
                None => self.from.clone(),
                Some(t) => T::lerp(&self.from, &self.to, t),
            },
        }
    }

    fn is_in_cycle(&self, now: Duration) -> bool {
        match self.repeat {
            RepeatMode::None => {
                now >= self.active_from() && now < self.active_from() + self.duration
            }
            RepeatMode::PingPong { interval } => {
                let active_from = self.active_from();
                if now < active_from {
                    return false;
                }
                let cycle_len = 2 * self.duration + interval;
                let phase = (now - active_from).as_nanos() as u128 % cycle_len.as_nanos();
                let active_end = (2 * self.duration).as_nanos();
                phase < active_end
            }
        }
    }

    fn compute_resume_deadline(&self, now: Duration) -> Option<Duration> {
        match self.repeat {
            RepeatMode::None => {
                let active_from = self.active_from();
                if now < active_from {
                    Some(active_from)
                } else {
                    None
                }
            }
            RepeatMode::PingPong { interval } => {
                let active_from = self.active_from();
                if now < active_from {
                    return Some(active_from);
                }
                let cycle_len = 2 * self.duration + interval;
                let elapsed_ns = (now - active_from).as_nanos() as u128;
                let phase = elapsed_ns % cycle_len.as_nanos();
                let active_end = (2 * self.duration).as_nanos();
                if phase < active_end {
                    None
                } else {
                    let remaining = cycle_len.as_nanos() - phase;
                    Some(now + Duration::from_nanos(remaining as u64))
                }
            }
        }
    }
}

// ── From<T> ───────────────────────────────────────────────────────────────────

impl<T: Animatable> From<T> for Animated<T> {
    fn from(value: T) -> Self {
        Self::static_value(value)
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AnimationConfig {
        AnimationConfig::new(Duration::from_secs(10), Easing::Linear)
    }

    fn config_with_delay() -> AnimationConfig {
        AnimationConfig {
            duration: Duration::from_secs(10),
            easing: Easing::Linear,
            delay: Duration::from_secs(60),
        }
    }

    // ── lerp_color ──────────────────────────────────────────────

    #[test]
    fn lerp_color_endpoints() {
        let c = lerp_color((0.0, 0.0, 0.0), (1.0, 1.0, 1.0), 0.0);
        assert!((c.0 - 0.0).abs() < 0.01);
        assert!((c.1 - 0.0).abs() < 0.01);
        assert!((c.2 - 0.0).abs() < 0.01);

        let c = lerp_color((0.0, 0.0, 0.0), (1.0, 1.0, 1.0), 1.0);
        assert!((c.0 - 1.0).abs() < 0.01);
        assert!((c.1 - 1.0).abs() < 0.01);
        assert!((c.2 - 1.0).abs() < 0.01);
    }

    #[test]
    fn lerp_color_identity() {
        let c = lerp_color((0.3, 0.5, 0.7), (0.3, 0.5, 0.7), 0.5);
        assert!((c.0 - 0.3).abs() < 0.01);
        assert!((c.1 - 0.5).abs() < 0.01);
        assert!((c.2 - 0.7).abs() < 0.01);
    }

    #[test]
    fn lerp_color_monotonic() {
        let a = lerp_color((0.0, 0.0, 0.0), (1.0, 1.0, 1.0), 0.2);
        let b = lerp_color((0.0, 0.0, 0.0), (1.0, 1.0, 1.0), 0.5);
        let c = lerp_color((0.0, 0.0, 0.0), (1.0, 1.0, 1.0), 0.8);
        assert!(a.0 < b.0 && b.0 < c.0);
    }

    // ── Animated<f32> ──────────────────────────────────────────

    #[test]
    fn static_value() {
        let a = Animated::static_value(5.0_f32);
        assert!((a.get() - 5.0).abs() < 0.01);
        assert!(!a.is_animating());
        assert!(a.resume_deadline().is_none());
    }

    #[test]
    fn get_at_start_of_animation() {
        let a = Animated::new(5.0, 10.0, config());
        assert!((a.get() - 5.0).abs() < 0.01);
    }

    #[test]
    fn get_returns_from_during_delay() {
        let a = Animated::new(0.0, 1.0, config_with_delay());
        assert!((a.get() - 0.0).abs() < 0.01);
    }

    #[test]
    fn is_animating_with_no_delay() {
        let a = Animated::new(0.0, 1.0, config());
        assert!(a.is_animating());
    }

    #[test]
    fn is_animating_false_during_delay() {
        let a = Animated::new(0.0, 1.0, config_with_delay());
        assert!(!a.is_animating());
    }

    #[test]
    fn is_animating_pingpong_no_delay() {
        let a = Animated::new_pingpong(
            0.0,
            1.0,
            Duration::from_secs(10),
            Easing::Linear,
            Duration::from_secs(5),
        );
        assert!(a.is_animating());
    }

    #[test]
    fn resume_deadline_none_while_active() {
        let a = Animated::new(0.0, 1.0, config());
        assert!(a.resume_deadline().is_none());
    }

    #[test]
    fn resume_deadline_some_during_delay() {
        let a = Animated::new(0.0, 1.0, config_with_delay());
        let d = a.resume_deadline().unwrap();
        assert!(d > monotonic_now());
    }

    #[test]
    fn from_trait_creates_static() {
        let a: Animated<f32> = 5.0.into();
        assert!((a.get() - 5.0).abs() < 0.01);
        assert!(!a.is_animating());
    }

    #[test]
    fn set_target_renews() {
        let mut a = Animated::new(5.0, 10.0, config());
        a.set_target(20.0);
        assert!((a.get() - 5.0).abs() < 0.01);
        assert!(a.is_animating());
    }

    #[test]
    fn reset_restarts() {
        let mut a = Animated::new(5.0, 10.0, config());
        a.reset();
        assert!((a.get() - 5.0).abs() < 0.01);
        assert!(a.is_animating());
    }

    #[test]
    fn clone_is_independent() {
        let a = Animated::new(5.0, 10.0, config());
        let b = a.clone();
        assert!((b.get() - 5.0).abs() < 0.01);
        assert!(b.is_animating());
    }

    #[test]
    fn is_finished_after_completion() {
        let a = Animated::new(
            0.0,
            1.0,
            AnimationConfig {
                duration: Duration::from_nanos(1),
                easing: Easing::Linear,
                delay: Duration::ZERO,
            },
        );
        std::thread::sleep(Duration::from_millis(1));
        assert!(a.is_finished());
    }
}
