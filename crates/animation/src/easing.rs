#[derive(Clone, Copy, Debug)]
pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

impl Easing {
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => t,
            Easing::EaseIn => t * t,
            Easing::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            Easing::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - 2.0 * (t - 1.0) * (t - 1.0)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear() {
        assert_eq!(Easing::Linear.apply(0.0), 0.0);
        assert_eq!(Easing::Linear.apply(0.5), 0.5);
        assert_eq!(Easing::Linear.apply(1.0), 1.0);
    }

    #[test]
    fn ease_in() {
        assert_eq!(Easing::EaseIn.apply(0.0), 0.0);
        assert_eq!(Easing::EaseIn.apply(0.5), 0.25);
        assert_eq!(Easing::EaseIn.apply(1.0), 1.0);
    }

    #[test]
    fn ease_out() {
        assert_eq!(Easing::EaseOut.apply(0.0), 0.0);
        assert_eq!(Easing::EaseOut.apply(0.5), 0.75);
        assert_eq!(Easing::EaseOut.apply(1.0), 1.0);
    }

    #[test]
    fn ease_in_out() {
        assert_eq!(Easing::EaseInOut.apply(0.0), 0.0);
        assert_eq!(Easing::EaseInOut.apply(0.25), 0.125);
        assert_eq!(Easing::EaseInOut.apply(0.5), 0.5);
        assert_eq!(Easing::EaseInOut.apply(0.75), 0.875);
        assert_eq!(Easing::EaseInOut.apply(1.0), 1.0);
    }

    #[test]
    fn clamp_below_zero() {
        assert_eq!(Easing::Linear.apply(-0.5), 0.0);
        assert_eq!(Easing::EaseIn.apply(-10.0), 0.0);
    }

    #[test]
    fn clamp_above_one() {
        assert_eq!(Easing::Linear.apply(1.5), 1.0);
        assert_eq!(Easing::EaseOut.apply(99.0), 1.0);
    }
}
