//! Toasts (`chat-calls.html` §G CLOSED): one per arriving message while the overlay is
//! closed, 340 wide at the top corner opposite the call widget's side, 24 from the edges,
//! newest on top, 8 apart, at most three (a fourth pushes the oldest out). Each lives 6 s;
//! in from 24 beyond its edge over 300 out, out the mirror over 300 in (alpha only, 90, with
//! reduced motion). A ringing call is the same toast with the ping ring, "is calling you"
//! and "… to answer"; it stays while the call rings, at most 20 s.
use std::time::Duration;

use easyplay_core::chat::overlay::Corner;
use easyplay_core::chat::overlay::snapshot::{MAX_TOASTS, Person};
use easyplay_ui::theme::Type;
use egui::{Align2, CornerRadius, Rect, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::look::Look;
use crate::model::{LiveToast, Model, ToastKey};
use crate::motion::{self, MOVE, REDUCED};
use crate::paint::{avatar, text};
use crate::words;

const WIDTH: f32 = 340.0;
const EDGE: f32 = 24.0;
const GAP: f32 = 8.0;
const LIFE: f64 = 6.0;
const RING_LIFE: f64 = 20.0;

impl Model {
    /// Bring the toasts on screen up to date with the snapshot at `now`.
    pub fn step_toasts(&mut self, now: f64) {
        let ringing = self.snap.ringing.is_some();
        let has_call = self
            .toasts
            .iter()
            .any(|t| t.key == ToastKey::Call && t.leaving.is_none());
        if ringing && !has_call {
            self.toasts.insert(
                0,
                LiveToast {
                    key: ToastKey::Call,
                    born: now,
                    leaving: None,
                },
            );
        }
        let snap = std::sync::Arc::clone(&self.snap);
        self.seen
            .retain(|id| snap.toasts.iter().any(|t| t.id == *id));
        let show = snap.look.toasts && !self.open.is_on();
        for id in snap.toasts.iter().map(|t| t.id) {
            if !self.seen.contains(&id) {
                self.seen.push(id);
                if show {
                    self.toasts.insert(
                        0,
                        LiveToast {
                            key: ToastKey::Message(id),
                            born: now,
                            leaving: None,
                        },
                    );
                }
            }
        }
        for t in &mut self.toasts {
            let done = match t.key {
                ToastKey::Message(_) => now - t.born >= LIFE,
                ToastKey::Call => !ringing || now - t.born >= RING_LIFE,
            };
            if done && t.leaving.is_none() {
                t.leaving = Some(now);
            }
        }
        let mut live = 0;
        for t in &mut self.toasts {
            if t.leaving.is_none() {
                live += 1;
                if live > MAX_TOASTS {
                    t.leaving = Some(now);
                }
            }
        }
        let len = self.toast_motion();
        self.toasts
            .retain(|t| t.leaving.is_none_or(|l| now - l < len));
    }

    fn toast_motion(&self) -> f64 {
        if self.reduced() { REDUCED } else { MOVE }
    }

    /// When the toasts next change by themselves (`None` = not before a snapshot).
    pub fn toasts_wake(&self, now: f64) -> Option<f64> {
        let len = self.toast_motion();
        self.toasts
            .iter()
            .map(|t| match (t.leaving, t.key) {
                (Some(l), _) => l + len,
                (None, _) if now - t.born < len => now,
                (None, ToastKey::Message(_)) => t.born + LIFE,
                (None, ToastKey::Call) => t.born + RING_LIFE,
            })
            .reduce(f64::min)
    }
}

pub fn draw(ui: &Ui, m: &Model, now: f64, alpha: f32) {
    let screen = ui.clip_rect();
    // Opposite the widget's side: top left when the widget is on the right.
    let left = matches!(m.snap.look.corner, Corner::TopRight | Corner::BottomRight);
    let len = m.toast_motion();
    let mut y = screen.top() + EDGE;
    for t in &m.toasts {
        let (shown, slide) = match t.leaving {
            None => {
                let p = motion::progress(now, t.born, len);
                (p, 1.0 - motion::ease_out(p))
            }
            Some(l) => {
                let p = motion::progress(now, l, len);
                (1.0 - p, motion::ease_in(p))
            }
        };
        let slide = if m.reduced() { 0.0 } else { slide * 24.0 };
        let x = if left {
            screen.left() + EDGE - slide
        } else {
            screen.right() - EDGE - WIDTH + slide
        };
        let a = alpha * shown;
        let h = match t.key {
            ToastKey::Call => m.snap.ringing.as_ref().map_or(0.0, |r| {
                toast(
                    ui,
                    m,
                    x,
                    y,
                    &r.who,
                    None,
                    words::text("chat.details.calling_you"),
                    true,
                    now,
                    a,
                )
            }),
            ToastKey::Message(id) => m.snap.toasts.iter().find(|s| s.id == id).map_or(0.0, |s| {
                let l = m.labels.toasts.iter().find(|l| l.id == id);
                toast(
                    ui,
                    m,
                    x,
                    y,
                    &s.who,
                    l.map(|l| l.group.as_str()).filter(|g| !g.is_empty()),
                    l.map_or("", |l| l.line.as_str()),
                    false,
                    now,
                    a,
                )
            }),
        };
        y += h + GAP;
    }
    if let Some(wake) = m.toasts_wake(now) {
        let wait = (wake - now).max(0.0);
        ui.ctx()
            .request_repaint_after(Duration::from_secs_f64(wait));
    }
    if m.snap.ringing.is_some() && !m.reduced() {
        // The ping ring moves: about 30 frames a second while it rings, never more.
        ui.ctx().request_repaint_after(Duration::from_millis(33));
    }
}

/// One toast at (`x`, `y`); returns its height.
#[allow(clippy::too_many_arguments)]
fn toast(
    ui: &Ui,
    m: &Model,
    x: f32,
    y: f32,
    who: &Person,
    group: Option<&str>,
    line: &str,
    call: bool,
    now: f64,
    a: f32,
) -> f32 {
    let look = &m.look;
    let fade = |c| Look::alpha(c, a);
    let rtl = words::rtl();
    let inner = WIDTH - 24.0 - 36.0 - 12.0;
    let name = text::galley(
        ui,
        Type::BodyBold,
        if call {
            &who.name
        } else {
            words::first_name(&who.name)
        },
        fade(look.t1),
        inner,
        1,
    );
    let line_g = text::galley(
        ui,
        Type::Body,
        line,
        fade(if call { look.t2 } else { look.t1 }),
        inner,
        1,
    );
    let height = 12.0 + 18.0 + 1.0 + 18.0 + 6.0 + 14.0 + 12.0;
    let rect = Rect::from_min_size(pos2(x, y), vec2(WIDTH, height));
    let p = ui.painter();
    let plate = look
        .raised
        .gamma_multiply(if m.snap.look.high_contrast { 1.0 } else { 0.94 });
    p.rect_filled(rect, CornerRadius::same(12), fade(plate));
    p.rect_stroke(
        rect,
        CornerRadius::same(12),
        Stroke::new(1.0, fade(look.line)),
        StrokeKind::Inside,
    );
    let (av_x, tx, anchor) = if rtl {
        (
            rect.right() - 12.0 - 36.0,
            rect.right() - 12.0 - 36.0 - 12.0,
            Align2::RIGHT_TOP,
        )
    } else {
        (
            rect.left() + 12.0,
            rect.left() + 12.0 + 36.0 + 12.0,
            Align2::LEFT_TOP,
        )
    };
    let av = Rect::from_min_size(pos2(av_x, rect.top() + 12.0), vec2(36.0, 36.0));
    if call {
        let t = (!m.reduced()).then(|| (now % 1.5 / 1.5) as f32);
        avatar::ping(p, av.center(), 18.0, t, look);
    }
    avatar::avatar(p, av, who, false, &m.pictures, look, a);
    let top = rect.top() + 12.0;
    let n = text::put(ui, &name, pos2(tx, top), anchor);
    if let Some(g) = group {
        let gx = if rtl { n.left() - 6.0 } else { n.right() + 6.0 };
        let g = text::galley(
            ui,
            Type::Caption,
            g,
            fade(look.t3),
            inner - n.width() - 6.0,
            1,
        );
        text::put(
            ui,
            &g,
            pos2(gx, n.center().y + 1.0),
            if rtl {
                Align2::RIGHT_CENTER
            } else {
                Align2::LEFT_CENTER
            },
        );
    }
    text::put(ui, &line_g, pos2(tx, top + 19.0), anchor);
    crate::closed::hint::key_line(ui, m, call, pos2(tx, top + 18.0 + 1.0 + 18.0 + 6.0), rtl, a);
    height
}

#[cfg(test)]
mod tests {
    use super::*;
    use easyplay_core::chat::overlay::ChatKey;
    use easyplay_core::chat::overlay::snapshot::{BodyKind, OverlayRing, Toast};

    fn toast(id: u64) -> Toast {
        Toast {
            id,
            chat: ChatKey::Direct(1),
            who: Person::default(),
            group: None,
            kind: BodyKind::Text,
            text: "hi".into(),
        }
    }

    fn model() -> Model {
        let mut m = Model::new(0);
        m.snap_mut().look.toasts = true;
        m
    }

    #[test]
    fn a_fourth_toast_pushes_the_oldest_out_and_each_leaves_after_six_seconds() {
        let mut m = model();
        m.snap_mut().toasts = (1..=4).map(toast).collect();
        m.step_toasts(10.0);
        let live: Vec<_> = m
            .toasts
            .iter()
            .filter(|t| t.leaving.is_none())
            .map(|t| t.key)
            .collect();
        assert_eq!(
            live,
            vec![
                ToastKey::Message(4),
                ToastKey::Message(3),
                ToastKey::Message(2)
            ]
        );
        m.step_toasts(16.1);
        assert!(m.toasts.iter().all(|t| t.leaving.is_some()));
        m.step_toasts(17.0);
        assert!(m.toasts.is_empty());
        // Shown once: the same snapshot does not bring them back.
        m.step_toasts(18.0);
        assert!(m.toasts.is_empty());
    }

    #[test]
    fn a_ringing_call_is_a_toast_until_it_stops_ringing() {
        let mut m = model();
        m.snap_mut().ringing = Some(OverlayRing {
            chat: ChatKey::Direct(1),
            who: Person::default(),
            since: 0,
        });
        m.step_toasts(1.0);
        assert_eq!(m.toasts[0].key, ToastKey::Call);
        m.step_toasts(15.0);
        assert!(m.toasts[0].leaving.is_none(), "rings past six seconds");
        m.snap_mut().ringing = None;
        m.step_toasts(16.0);
        assert!(m.toasts[0].leaving.is_some());
    }

    #[test]
    fn no_toast_appears_while_the_panel_is_open_or_toasts_are_off() {
        let mut m = model();
        m.open.set(true, 0.0);
        m.snap_mut().toasts = vec![toast(1)];
        m.step_toasts(1.0);
        assert!(m.toasts.is_empty());
        let mut off = model();
        off.snap_mut().look.toasts = false;
        off.snap_mut().toasts = vec![toast(1)];
        off.step_toasts(1.0);
        assert!(off.toasts.is_empty());
    }
}
