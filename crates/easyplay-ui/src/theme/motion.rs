//! Motion tokens, easing curves and the reduced-motion switch (UI_DESIGN_SYSTEM §5,
//! USER_EXPERIENCE §10). Durations are in seconds, as egui's animation API takes them.
//!
//! Three helpers cover what moves: [`appear`] for things that enter (opacity from 0),
//! [`glide`] for things that travel between two resting places (a selection, a thumb, a card
//! finding its new slot) and [`pulse`] for a one-shot accent (the favourite star). All three
//! request repaints only while they run, so an idle window stays idle.

use egui::{Context, Id, Pos2, Rect, Vec2, emath::TSTransform};

/// A duration token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dur {
    /// 90 ms: hover, press, focus.
    Fast,
    /// 120 ms: small reveals and exits (controls on a cover, the drop zone, a toast leaving).
    Quick,
    /// 160 ms: overlays, menus, cross-fades, the segmented thumb.
    Base,
    /// 200 ms: travel between resting places (sidebar selection, sidebar width, grid reflow).
    Glide,
    /// 240 ms.
    Slow,
    /// 280 ms: route changes, the first appearance of the library.
    Page,
}

impl Dur {
    /// The nominal value in seconds.
    pub const fn secs(self) -> f32 {
        match self {
            Dur::Fast => 0.090,
            Dur::Quick => 0.120,
            Dur::Base => 0.160,
            Dur::Glide => 0.200,
            Dur::Slow => 0.240,
            Dur::Page => 0.280,
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

/// Opacity (0 → 1) of something that appears: a sheet, a dialog, a toast, the drop zone,
/// a route's content. It starts at 0 whenever `id` was not shown in the previous pass and
/// reaches 1 after `secs` (`ease.standard`); with `secs` 0 it is 1 at once. egui's own
/// `animate_bool` starts at its target, so a plain call would never fade in.
pub fn appear(ctx: &Context, id: Id, secs: f32) -> f32 {
    let pass = ctx.cumulative_pass_nr();
    let key = id.with("appear.seen");
    let (last, generation) = ctx
        .data(|d| d.get_temp::<(u64, u64)>(key))
        .unwrap_or((0, 0));
    let fresh = last == 0 || last + 1 < pass;
    let generation = generation + u64::from(fresh);
    ctx.data_mut(|d| d.insert_temp(key, (pass.max(1), generation)));
    if secs <= 0.0 {
        return 1.0;
    }
    let anim = id.with(("appear", generation));
    if fresh {
        // Seed the animation at 0; the next pass animates towards 1.
        ctx.animate_bool_with_time(anim, false, secs);
        ctx.request_repaint();
        return 0.0;
    }
    ctx.animate_bool_with_time_and_easing(anim, true, secs, ease_standard)
}

/// A value that travels to `target` over `secs` with `ease.standard`. It starts at `target`
/// whenever `id` was not animated in the previous pass (so nothing flies in from a stale
/// place), and a change of target mid-flight continues from where the value is.
pub fn glide(ctx: &Context, id: Id, target: f32, secs: f32) -> f32 {
    #[derive(Clone, Copy)]
    struct Flight {
        from: f32,
        to: f32,
        started: f64,
        pass: u64,
    }
    let pass = ctx.cumulative_pass_nr();
    let now = ctx.input(|i| i.time);
    let key = id.with("glide");
    let prev = ctx.data(|d| d.get_temp::<Flight>(key));
    let mut f = match prev {
        Some(f) if secs > 0.0 && f.pass + 1 >= pass => f,
        _ => Flight {
            from: target,
            to: target,
            started: now,
            pass,
        },
    };
    let at = |f: &Flight| {
        let t = ((now - f.started) as f32 / secs.max(f32::EPSILON)).clamp(0.0, 1.0);
        (f.from + (f.to - f.from) * ease_standard(t), t)
    };
    if (f.to - target).abs() > f32::EPSILON {
        f.from = at(&f).0;
        f.to = target;
        f.started = now;
    }
    let (value, t) = at(&f);
    f.pass = pass;
    ctx.data_mut(|d| d.insert_temp(key, f));
    if t < 1.0 && (f.to - f.from).abs() > f32::EPSILON {
        ctx.request_repaint();
        value
    } else {
        target
    }
}

/// [`glide`] for a position.
pub fn glide_pos(ctx: &Context, id: Id, target: Pos2, secs: f32) -> Pos2 {
    Pos2::new(
        glide(ctx, id.with("x"), target.x, secs),
        glide(ctx, id.with("y"), target.y, secs),
    )
}

/// A one-shot 0 → 1 → 0 pulse that starts whenever `on` turns true (not on the first sight
/// of `id`): the favourite star's pop. Returns 0 at rest.
pub fn pulse(ctx: &Context, id: Id, on: bool, secs: f32) -> f32 {
    let key = id.with("pulse");
    let now = ctx.input(|i| i.time);
    let (was, started) = ctx
        .data(|d| d.get_temp::<(bool, f64)>(key))
        .unwrap_or((on, f64::NEG_INFINITY));
    let started = if on && !was { now } else { started };
    ctx.data_mut(|d| d.insert_temp(key, (on, started)));
    if secs <= 0.0 {
        return 0.0;
    }
    let t = ((now - started) as f32 / secs).clamp(0.0, 1.0);
    if t >= 1.0 {
        return 0.0;
    }
    ctx.request_repaint();
    (t * std::f32::consts::PI).sin()
}

/// The transform that scales by `scale` around `pivot` and then moves by `offset`.
pub fn scale_about(pivot: Pos2, scale: f32, offset: Vec2) -> TSTransform {
    TSTransform::new(pivot.to_vec2() * (1.0 - scale) + offset, scale)
}

/// `rect` scaled by `scale` around its centre.
pub fn scaled(rect: Rect, scale: f32) -> Rect {
    Rect::from_center_size(rect.center(), rect.size() * scale)
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
            Dur::Quick,
            Dur::Base,
            Dur::Glide,
            Dur::Slow,
            Dur::Page,
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
    fn appear_fades_in_from_zero_and_again_after_hiding() {
        let ctx = Context::default();
        let id = Id::unique("sheet");
        let run = |show: bool, dt: f32| {
            let mut out = None;
            let input = egui::RawInput {
                predicted_dt: dt,
                ..Default::default()
            };
            let mut full = ctx.run_ui(input, |ui| {
                if show {
                    out = Some(appear(ui.ctx(), id, Dur::Base.secs()));
                }
            });
            full.textures_delta.clear();
            out
        };
        assert_eq!(run(true, 0.05), Some(0.0));
        let mid = run(true, 0.05).unwrap();
        assert!(mid > 0.0 && mid < 1.0, "{mid}");
        for _ in 0..10 {
            run(true, 0.05);
        }
        assert_eq!(run(true, 0.05), Some(1.0));
        // Hidden for a pass: the next appearance fades in again.
        run(false, 0.05);
        assert_eq!(run(true, 0.05), Some(0.0));
        // Reduced motion with no fade: visible at once.
        assert_eq!(appear(&ctx, Id::unique("x"), 0.0), 1.0);
    }

    fn pass<R>(ctx: &Context, dt: f32, f: impl FnOnce(&Context) -> R) -> R {
        let mut out = None;
        let input = egui::RawInput {
            predicted_dt: dt,
            ..Default::default()
        };
        let mut f = Some(f);
        let mut full = ctx.run_ui(input, |ui| {
            if let Some(f) = f.take() {
                out = Some(f(ui.ctx()));
            }
        });
        full.textures_delta.clear();
        out.expect("ran")
    }

    #[test]
    fn glide_starts_at_rest_travels_and_snaps_after_a_gap() {
        let ctx = Context::default();
        let id = Id::unique("thumb");
        let secs = Dur::Glide.secs();
        assert_eq!(pass(&ctx, 0.05, |c| glide(c, id, 10.0, secs)), 10.0);
        // A new target: the value leaves 10 and arrives at 50 without overshooting.
        let mut last = 10.0;
        for _ in 0..3 {
            let v = pass(&ctx, 0.05, |c| glide(c, id, 50.0, secs));
            assert!(v >= last && v <= 50.0, "{v}");
            last = v;
        }
        assert!(last > 10.0);
        for _ in 0..6 {
            last = pass(&ctx, 0.05, |c| glide(c, id, 50.0, secs));
        }
        assert_eq!(last, 50.0);
        // Not animated for a pass: the next target is taken at once.
        pass(&ctx, 0.05, |_| ());
        assert_eq!(pass(&ctx, 0.05, |c| glide(c, id, 200.0, secs)), 200.0);
        // No movement allowed: always at the target.
        assert_eq!(pass(&ctx, 0.05, |c| glide(c, id, 7.0, 0.0)), 7.0);
    }

    #[test]
    fn pulse_runs_once_when_turned_on() {
        let ctx = Context::default();
        let id = Id::unique("star");
        let secs = Dur::Slow.secs();
        // Already on when first seen: no pulse.
        assert_eq!(pass(&ctx, 0.05, |c| pulse(c, id, true, secs)), 0.0);
        assert_eq!(pass(&ctx, 0.05, |c| pulse(c, id, false, secs)), 0.0);
        assert_eq!(pass(&ctx, 0.05, |c| pulse(c, id, true, secs)), 0.0);
        let mid = pass(&ctx, 0.1, |c| pulse(c, id, true, secs));
        assert!(mid > 0.5, "{mid}");
        for _ in 0..5 {
            pass(&ctx, 0.1, |c| pulse(c, id, true, secs));
        }
        assert_eq!(pass(&ctx, 0.1, |c| pulse(c, id, true, secs)), 0.0);
    }

    #[test]
    fn scale_about_keeps_the_pivot_in_place() {
        let pivot = Pos2::new(40.0, 60.0);
        let t = scale_about(pivot, 0.5, Vec2::ZERO);
        assert!((t.mul_pos(pivot) - pivot).length() < 1e-4);
        let moved = scale_about(pivot, 1.0, Vec2::new(0.0, 8.0));
        assert_eq!(moved.mul_pos(pivot), Pos2::new(40.0, 68.0));
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
