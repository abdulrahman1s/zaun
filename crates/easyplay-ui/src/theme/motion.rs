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
    /// 80 ms: a press.
    Press,
    /// 90 ms: a menu leaving.
    Quick,
    /// 100 ms: a tooltip.
    Tip,
    /// 140 ms: hover fills, a cover under the pointer, a menu arriving, exits of dialogs and
    /// toasts.
    Fast,
    /// 220 ms: the tab underline, the segmented thumb, a switch's knob, a tab's body, a
    /// toast arriving.
    Base,
    /// 300 ms: travel between resting places (the wall's reflow, a dialog arriving, bars
    /// growing).
    Move,
    /// 360 ms: the favourite star.
    Slow,
    /// 400 ms: a cover flying between the wall and its page, a page's parts rising.
    Page,
}

impl Dur {
    /// The nominal value in seconds.
    pub const fn secs(self) -> f32 {
        match self {
            Dur::Press => 0.080,
            Dur::Quick => 0.090,
            Dur::Tip => 0.100,
            Dur::Fast => 0.140,
            Dur::Base => 0.220,
            Dur::Move => 0.300,
            Dur::Slow => 0.360,
            Dur::Page => 0.400,
        }
    }
}

/// The longest fade under reduced motion.
const REDUCED_MAX: f32 = 0.120;

/// Whether movement is allowed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Motion {
    /// Reduced motion is in effect (system setting or preference).
    pub reduced: bool,
}

impl Motion {
    /// Duration of a movement (position, size, scale): zero under reduced motion.
    pub const fn movement(self, d: Dur) -> f32 {
        if self.reduced { 0.0 } else { d.secs() }
    }

    /// Duration of an opacity change: under reduced motion every change is a fade of at
    /// most 120 ms.
    pub const fn fade(self, d: Dur) -> f32 {
        if !self.reduced {
            return d.secs();
        }
        let secs = match d {
            Dur::Press | Dur::Quick | Dur::Tip => d.secs(),
            Dur::Fast => 0.090,
            Dur::Base => 0.110,
            Dur::Move | Dur::Slow | Dur::Page => REDUCED_MAX,
        };
        if secs > REDUCED_MAX {
            REDUCED_MAX
        } else {
            secs
        }
    }

    /// `Style.animation_time`: `dur.fast`, or zero under reduced motion.
    pub const fn animation_time(self) -> f32 {
        self.movement(Dur::Fast)
    }

    /// Whether continuous effects (the loading pulse, the running game's bars) may run.
    pub const fn shimmer(self) -> bool {
        !self.reduced
    }
}

/// Progress (0 → 1, linear) of something that appears: a dialog, a toast, the drop zone, a
/// page's content. It starts at 0 whenever `id` was not shown in the previous pass and
/// reaches 1 after `secs`; with `secs` 0 it is 1 at once. Opacity takes the value as it is;
/// what moves with it passes it through [`ease_out`]. egui's own `animate_bool` starts at
/// its target, so a plain call would never fade in.
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
    ctx.animate_bool_with_time_and_easing(anim, true, secs, egui::emath::easing::linear)
}

/// A value that travels to `target` over `secs` with [`ease_out`]. It starts at `target`
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
        (f.from + (f.to - f.from) * ease_out(t), t)
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

/// `out`, for anything that moves: 1 − (1 − t)⁵.
pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(5)
}

/// `in`, for exits: t³.
pub fn ease_in(t: f32) -> f32 {
    t.clamp(0.0, 1.0).powi(3)
}

/// `pop`, the favourite star: arrives with a 5 % overshoot.
pub fn ease_pop(t: f32) -> f32 {
    cubic_bezier(0.34, 1.4, 0.64, 1.0, t)
}

/// Progress (0–1) of something that started `delay` seconds after `started` and takes
/// `secs`, at time `now`.
pub fn progress(now: f64, started: f64, delay: f32, secs: f32) -> f32 {
    if secs <= 0.0 {
        return 1.0;
    }
    (((now - started) as f32 - delay) / secs).clamp(0.0, 1.0)
}

/// Seconds since `id` came into view. The clock restarts whenever `id` was not asked about in
/// the previous pass (the page was away, the tab was closed), so whatever arrives arrives
/// again. Repaints are requested until `until` seconds have passed.
pub fn shown_for(ctx: &Context, id: Id, until: f32) -> f32 {
    let pass = ctx.cumulative_pass_nr();
    let now = ctx.input(|i| i.time);
    let key = id.with("shown_for");
    let prev = ctx.data(|d| d.get_temp::<(u64, f64)>(key));
    let started = match prev {
        Some((last, started)) if last + 1 >= pass => started,
        _ => now,
    };
    ctx.data_mut(|d| d.insert_temp(key, (pass, started)));
    let t = (now - started) as f32;
    if t < until {
        ctx.request_repaint();
    }
    t
}

/// Content that arrives: a tab's body, a settings section, a cover newly shown, a part of
/// a page. Returns its opacity (linear) and how far below its place it still is (eased
/// out), `delay` seconds after `id` came into view and over `secs`. Under reduced motion
/// nothing travels and the fade is short.
pub fn arrive(
    ctx: &Context,
    id: Id,
    motion: Motion,
    delay: f32,
    d: Dur,
    distance: f32,
) -> (f32, f32) {
    let (fade, travel) = (motion.fade(d), motion.movement(d));
    let delay = if motion.reduced { 0.0 } else { delay };
    let t = shown_for(ctx, id, delay + fade.max(travel));
    let opacity = if fade <= 0.0 {
        1.0
    } else {
        ((t - delay) / fade).clamp(0.0, 1.0)
    };
    let offset = if travel <= 0.0 {
        0.0
    } else {
        distance * (1.0 - ease_out(((t - delay) / travel).clamp(0.0, 1.0)))
    };
    (opacity, offset)
}

/// How much of its height a bar has grown to (0–1): bars rise from nothing over
/// [`Dur::Move`], `index` × 10 ms apart (capped at 30), from the moment `id` came into
/// view. At full height at once under reduced motion.
pub fn bar_growth(ctx: &Context, id: Id, motion: Motion, index: usize) -> f32 {
    use super::tokens::travel;
    let secs = motion.movement(Dur::Move);
    if secs <= 0.0 {
        return 1.0;
    }
    let delay = index.min(travel::BAR_STAGGER_CAP) as f32 * travel::BAR_STAGGER;
    let until = secs + travel::BAR_STAGGER_CAP as f32 * travel::BAR_STAGGER;
    let t = shown_for(ctx, id, until);
    ease_out(((t - delay) / secs).clamp(0.0, 1.0))
}

/// A one-shot timeline that starts whenever `key` changes (and on the first sight of `id`
/// when `on_first` is set): returns the seconds since it started, or `None` once `secs`
/// have passed. Requests repaints while it runs.
pub fn since_change<K>(ctx: &Context, id: Id, key: K, secs: f32, on_first: bool) -> Option<f32>
where
    K: Clone + PartialEq + Send + Sync + 'static,
{
    let now = ctx.input(|i| i.time);
    let slot = id.with("since_change");
    let prev = ctx.data(|d| d.get_temp::<(K, f64)>(slot));
    let started = match prev {
        Some((k, at)) if k == key => at,
        Some(_) => now,
        None if on_first => now,
        None => f64::NEG_INFINITY,
    };
    ctx.data_mut(|d| d.insert_temp(slot, (key, started)));
    let t = (now - started) as f32;
    if secs > 0.0 && t < secs {
        ctx.request_repaint();
        Some(t)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduced_motion_zeroes_movement_and_caps_fades() {
        let m = Motion { reduced: true };
        for d in [
            Dur::Press,
            Dur::Quick,
            Dur::Tip,
            Dur::Fast,
            Dur::Base,
            Dur::Move,
            Dur::Slow,
            Dur::Page,
        ] {
            assert_eq!(m.movement(d), 0.0);
            assert!(m.fade(d) <= 0.120);
        }
        assert_eq!(m.animation_time(), 0.0);
        assert!(!m.shimmer());
    }

    #[test]
    fn full_motion_uses_token_values() {
        let m = Motion::default();
        assert_eq!(m.movement(Dur::Base), 0.220);
        assert_eq!(m.fade(Dur::Page), 0.400);
        assert_eq!(m.movement(Dur::Fast), 0.140);
        assert_eq!(m.movement(Dur::Move), 0.300);
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
        let secs = Dur::Move.secs();
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
        for f in [ease_out, ease_in] {
            assert!(f(0.0).abs() < 1e-3);
            assert!((f(1.0) - 1.0).abs() < 1e-3);
            let mut prev = 0.0;
            for i in 1..=20 {
                let v = f(i as f32 / 20.0);
                assert!(v + 1e-4 >= prev);
                prev = v;
            }
        }
        // `out` decelerates: nearly there at the midpoint; `in` has barely left.
        assert!((ease_out(0.5) - 0.96875).abs() < 1e-4);
        assert!((ease_in(0.5) - 0.125).abs() < 1e-4);
        // `pop` overshoots by about a twentieth and settles.
        let peak = (0..=100)
            .map(|i| ease_pop(i as f32 / 100.0))
            .fold(0.0, f32::max);
        assert!(peak > 1.02 && peak < 1.12, "{peak}");
        assert!((ease_pop(1.0) - 1.0).abs() < 1e-3);
    }
}
