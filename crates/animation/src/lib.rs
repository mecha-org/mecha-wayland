mod easing;

use std::time::Duration;

use smallvec::SmallVec;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AnimationId(pub u64);

#[derive(Clone, Copy, Debug)]
pub struct AnimationConfig {
    pub from: f32,
    pub to: f32,
    pub duration: Duration,
    pub easing: Easing,
    pub delay: Duration,
}

impl AnimationConfig {
    pub fn immediate(from: f32, to: f32, duration: Duration, easing: Easing) -> Self {
        Self {
            from,
            to,
            duration,
            easing,
            delay: Duration::ZERO,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum RepeatMode {
    None,
    PingPong {
        /// Pause between cycles. The animation runs forward then backward
        /// (2 × duration), then idles for `interval`. During the interval
        /// `is_active()` returns false and `next_resume_deadline()` returns the
        /// monotonic deadline when the next cycle begins.
        interval: Duration,
    },
}

struct Entry {
    id: AnimationId,
    config: AnimationConfig,
    repeat: RepeatMode,
    started_at: Duration,
}

impl Entry {
    fn active_from(&self) -> Duration {
        self.started_at + self.config.delay
    }

    fn progress(&self, now: Duration) -> Option<f32> {
        let active_from = self.active_from();
        if now < active_from {
            return None;
        }
        let elapsed = (now - active_from).as_secs_f32();
        let t = (elapsed / self.config.duration.as_secs_f32()).clamp(0.0, 1.0);
        Some(self.config.easing.apply(t))
    }

    fn value(&self, now: Duration) -> f32 {
        match self.repeat {
            RepeatMode::PingPong { interval } => {
                let active_from = self.active_from();
                if now < active_from {
                    return self.config.from;
                }
                let cycle_ns = (2 * self.config.duration + interval).as_nanos();
                let elapsed_ns = (now - active_from).as_nanos() as u128;
                let phase_ns = elapsed_ns % cycle_ns;
                let active_end_ns = (2 * self.config.duration).as_nanos();
                if phase_ns >= active_end_ns {
                    return self.config.from;
                }
                let half_ns = self.config.duration.as_nanos();
                if phase_ns < half_ns {
                    let t = (phase_ns as f64 / half_ns as f64) as f32;
                    let eased = self.config.easing.apply(t);
                    self.config.from + (self.config.to - self.config.from) * eased
                } else {
                    let reverse_elapsed = phase_ns - half_ns;
                    let t = (reverse_elapsed as f64 / half_ns as f64) as f32;
                    let eased = self.config.easing.apply(t);
                    self.config.to + (self.config.from - self.config.to) * eased
                }
            }
            RepeatMode::None => match self.progress(now) {
                None => self.config.from,
                Some(t) => self.config.from + (self.config.to - self.config.from) * t,
            },
        }
    }

    fn is_in_cycle(&self, now: Duration) -> bool {
        match self.repeat {
            RepeatMode::None => {
                now >= self.active_from() && now < self.active_from() + self.config.duration
            }
            RepeatMode::PingPong { interval } => {
                let cycle_len = 2 * self.config.duration + interval;
                let active_from = self.active_from();
                if now < active_from {
                    return false;
                }
                let phase = (now - active_from).as_nanos() as u128 % cycle_len.as_nanos();
                let active_end = (2 * self.config.duration).as_nanos();
                phase < active_end
            }
        }
    }

    fn resume_deadline(&self, now: Duration) -> Option<Duration> {
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
                let cycle_len = 2 * self.config.duration + interval;
                let active_from = self.active_from();
                if now < active_from {
                    return Some(active_from);
                }
                let elapsed_ns = (now - active_from).as_nanos() as u128;
                let phase = elapsed_ns % cycle_len.as_nanos();
                let active_end = (2 * self.config.duration).as_nanos();
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

pub struct Animator {
    entries: SmallVec<[Entry; 8]>,
    next_id: u64,
}

impl Default for Animator {
    fn default() -> Self {
        Self {
            entries: SmallVec::new(),
            next_id: 0,
        }
    }
}

impl Animator {
    pub fn new() -> Self {
        Self::default()
    }

    fn push(&mut self, config: AnimationConfig, repeat: RepeatMode) -> AnimationId {
        let id = AnimationId(self.next_id);
        self.next_id += 1;
        self.entries.push(Entry {
            id,
            config,
            repeat,
            started_at: monotonic_now(),
        });
        id
    }

    pub fn animate(&mut self, config: AnimationConfig) -> AnimationId {
        self.push(config, RepeatMode::None)
    }

    pub fn animate_pingpong(&mut self, config: AnimationConfig, interval: Duration) -> AnimationId {
        self.push(config, RepeatMode::PingPong { interval })
    }

    pub fn animate_by(&mut self, id: AnimationId, delta: f32, dur: Duration, easing: Easing) {
        let now = monotonic_now();
        let current = self.get(id);
        let cfg = AnimationConfig {
            from: current,
            to: current + delta,
            duration: dur,
            easing,
            delay: Duration::ZERO,
        };
        self.entries.retain(|e| e.id != id);
        self.entries.push(Entry {
            id,
            config: cfg,
            repeat: RepeatMode::None,
            started_at: now,
        });
    }

    pub fn get(&self, id: AnimationId) -> f32 {
        let now = monotonic_now();
        self.entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.value(now))
            .unwrap_or(0.0)
    }

    pub fn cancel(&mut self, id: AnimationId) {
        self.entries.retain(|e| e.id != id);
    }

    pub fn is_active(&self) -> bool {
        let now = monotonic_now();
        self.entries.iter().any(|e| e.is_in_cycle(now))
    }

    /// Absolute monotonic deadline when the next paused entry resumes.
    /// Feed this directly into a deadline timer.
    pub fn next_resume_deadline(&self) -> Option<Duration> {
        let now = monotonic_now();
        self.entries
            .iter()
            .filter_map(|e| e.resume_deadline(now))
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(from: f32, to: f32) -> AnimationConfig {
        AnimationConfig::immediate(from, to, Duration::from_secs(10), Easing::Linear)
    }

    fn config_with_delay() -> AnimationConfig {
        AnimationConfig {
            from: 0.0,
            to: 1.0,
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

    // ── Animator ────────────────────────────────────────────────

    #[test]
    fn get_unknown_id_is_zero() {
        let a = Animator::new();
        assert_eq!(a.get(AnimationId(42)), 0.0);
    }

    #[test]
    fn get_at_start_of_immediate() {
        let mut a = Animator::new();
        let id = a.animate(config(5.0, 10.0));
        assert!((a.get(id) - 5.0).abs() < 0.01);
    }

    #[test]
    fn get_returns_from_during_delay() {
        let mut a = Animator::new();
        let id = a.animate(config_with_delay());
        assert!((a.get(id) - 0.0).abs() < 0.01);
    }

    #[test]
    fn is_active_with_no_delay() {
        let mut a = Animator::new();
        a.animate(config(0.0, 1.0));
        assert!(a.is_active());
    }

    #[test]
    fn is_active_false_during_delay() {
        let mut a = Animator::new();
        a.animate(config_with_delay());
        assert!(!a.is_active());
    }

    #[test]
    fn is_active_pingpong_with_no_delay() {
        let mut a = Animator::new();
        a.animate_pingpong(config(0.0, 1.0), Duration::from_secs(5));
        assert!(a.is_active());
    }

    #[test]
    fn is_active_pingpong_false_during_delay() {
        let mut a = Animator::new();
        a.animate_pingpong(config_with_delay(), Duration::from_secs(5));
        assert!(!a.is_active());
    }

    #[test]
    fn pingpong_get_from_during_delay() {
        let mut a = Animator::new();
        let id = a.animate_pingpong(config_with_delay(), Duration::from_secs(5));
        assert!((a.get(id) - 0.0).abs() < 0.01);
    }

    #[test]
    fn resume_deadline_none_while_active() {
        let mut a = Animator::new();
        a.animate(config(0.0, 1.0));
        assert!(a.next_resume_deadline().is_none());
    }

    #[test]
    fn resume_deadline_some_during_delay() {
        let mut a = Animator::new();
        a.animate(config_with_delay());
        let d = a.next_resume_deadline().unwrap();
        assert!(d > monotonic_now());
    }

    #[test]
    fn resume_deadline_pingpong_some_during_delay() {
        let mut a = Animator::new();
        a.animate_pingpong(config_with_delay(), Duration::from_secs(5));
        let d = a.next_resume_deadline().unwrap();
        assert!(d > monotonic_now());
    }
}
