//! Motion tokens, easing curves and the reduced-motion switch (UI_DESIGN_SYSTEM §5,
//! USER_EXPERIENCE §10). Durations are in seconds, as egui's animation API takes them.

/// A duration token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dur {
    /// 90 ms: hover, focus, pill fade.
    Fast,
    /// 160 ms: overlays, cross-fades.
    Base,
    /// 240 ms.
    Slow,
    /// 280 ms: route changes.
    Page,
    /// 200 ms: sidebar width.
    Sidebar,
    /// 120 ms: toast out, drop zone.
    Out,
}

impl Dur {
    /// The nominal value in seconds.
    pub const fn secs(self) -> f32 {
        match self {
            Dur::Fast => 0.090,
            Dur::Base => 0.160,
            Dur::Slow => 0.240,
            Dur::Page => 0.280,
            Dur::Sidebar => 0.200,
            Dur::Out => 0.120,
        }
    }
}

/// Whether movement is allowed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Motion {
    /// Reduced motion is in effect (system setting or preference).
    pub reduced: bool,
}

impl Motion {
    /// Duration of a movement (position, size, shimmer): zero under reduced motion.
    pub const fn movement(self, d: Dur) -> f32 {
        if self.reduced { 0.0 } else { d.secs() }
    }

    /// Duration of an opacity change: capped at `dur.fast` under reduced motion.
    pub const fn fade(self, d: Dur) -> f32 {
        if self.reduced && d.secs() > Dur::Fast.secs() {
            Dur::Fast.secs()
        } else {
            d.secs()
        }
    }

    /// `Style.animation_time`: `dur.base`, or zero under reduced motion.
    pub const fn animation_time(self) -> f32 {
        self.movement(Dur::Base)
    }

    /// Whether continuous effects (skeleton shimmer) may run.
    pub const fn shimmer(self) -> bool {
        !self.reduced
    }
}

/// A cubic Bézier easing `cubic-bezier(x1, y1, x2, y2)` evaluated at `t` (0–1).
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let bez = |a: f32, b: f32, s: f32| {
        let u = 1.0 - s;
        3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s
    };
    // Solve x(s) = t by bisection; cheap and monotone for valid easing curves.
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if bez(x1, x2, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez(y1, y2, 0.5 * (lo + hi))
}

/// `ease.standard`: cubic-bezier(0.2, 0, 0, 1).
pub fn ease_standard(t: f32) -> f32 {
    cubic_bezier(0.2, 0.0, 0.0, 1.0, t)
}

/// `ease.exit`: cubic-bezier(0.4, 0, 1, 1).
pub fn ease_exit(t: f32) -> f32 {
    cubic_bezier(0.4, 0.0, 1.0, 1.0, t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduced_motion_zeroes_movement_and_caps_fades() {
        let m = Motion { reduced: true };
        for d in [
            Dur::Fast,
            Dur::Base,
            Dur::Slow,
            Dur::Page,
            Dur::Sidebar,
            Dur::Out,
        ] {
            assert_eq!(m.movement(d), 0.0);
            assert!(m.fade(d) <= Dur::Fast.secs());
        }
        assert_eq!(m.animation_time(), 0.0);
        assert!(!m.shimmer());
    }

    #[test]
    fn full_motion_uses_token_values() {
        let m = Motion::default();
        assert_eq!(m.movement(Dur::Base), 0.160);
        assert_eq!(m.fade(Dur::Page), 0.280);
    }

    #[test]
    fn easing_endpoints_and_monotonicity() {
        for f in [ease_standard, ease_exit] {
            assert!(f(0.0).abs() < 1e-3);
            assert!((f(1.0) - 1.0).abs() < 1e-3);
            let mut prev = 0.0;
            for i in 1..=20 {
                let v = f(i as f32 / 20.0);
                assert!(v + 1e-4 >= prev);
                prev = v;
            }
        }
        // Standard easing decelerates: more than half done at the midpoint.
        assert!(ease_standard(0.5) > 0.5);
    }
}
