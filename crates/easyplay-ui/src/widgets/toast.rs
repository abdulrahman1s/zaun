//! The toast (CSS `.toast`, notes §5 "Toast"): one at a time at the bottom centre, 24 from
//! the window's edge; a 38 high pill on `raised` with a 1 pt `line` outline and the pop
//! shadow, padded 16 before the text and 6 after; the text 13 pt, then an optional action
//! as a 26 high pill text button in `accent_text`. Every kind looks the same. It arrives
//! (alpha and a rise of 10 over 220 ms) and leaves (140 ms). Rendering only; timing lives in
//! [`crate::vm::toasts::ToastQueue`].

use easyplay_core::clock::MonoInstant;
use egui::{
    Align2, Area, Context, CornerRadius, Id, LayerId, Order, Rect, Sense, Shape, Stroke,
    StrokeKind, pos2, vec2,
};

use super::modal::{ExitKind, paint_exits, track_exit};
use super::{focus_ring, hover_t, press_t};
use crate::a11y;
use crate::strings;
use crate::theme::motion::ease_out;
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{control, layout, overlay, radius, stroke, travel};
use crate::theme::{self, Dur, Type};
use crate::vm::toasts::{ToastId, ToastQueue};

/// What the user did with the toast this frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToastOutput {
    /// Actions activated: (toast, action key).
    pub actions: Vec<(ToastId, u64)>,
}

fn area_id() -> Id {
    Id::unique("easyplay.toasts")
}

/// Paint the toast, update its hover/focus hold, handle its action. `bottom_inset` keeps it
/// above a footer. Also paints whatever is leaving ([`paint_exits`]): call it last in the
/// frame.
pub fn show(
    ctx: &Context,
    queue: &mut ToastQueue,
    now: MonoInstant,
    bottom_inset: f32,
) -> ToastOutput {
    let mut out = ToastOutput::default();
    let Some(t) = queue.visible().last().cloned() else {
        paint_exits(ctx);
        return out;
    };
    let theme = theme::theme(ctx);
    let p = theme.palette.clone();
    let motion = theme.motion;
    // While a dialog is open the toast sits below it: the dialog keeps keyboard focus, and
    // the toast's action waits until it closes.
    let modal_open = ctx.memory(|m| m.top_modal_layer()).is_some();
    let key = Id::unique(("easyplay.toast", t.id.0));
    let alpha = theme::motion::appear(ctx, key.with("alpha"), motion.fade(Dur::Base));
    let lift = ease_out(theme::motion::appear(
        ctx,
        key.with("move"),
        motion.movement(Dur::Base),
    ));
    let rise = (1.0 - lift) * travel::TOAST_RISE;
    let order = if modal_open {
        Order::Middle
    } else {
        Order::Foreground
    };
    let mut hold = false;
    let mut acted = false;
    // Each toast is its own area, so one replacing another leaves while the new one comes.
    let id = area_id().with(t.id.0);
    Area::new(id)
        .order(order)
        .anchor(
            Align2::CENTER_BOTTOM,
            vec2(0.0, -(layout::TOAST_BOTTOM + bottom_inset) + rise),
        )
        .interactable(true)
        .show(ctx, |ui| {
            a11y::labelled(
                ctx,
                ui.scope_id(),
                egui::Role::Log,
                strings::feedback::NOTIFICATIONS(),
            );
            ui.multiply_opacity(alpha);
            let [pad, end] = overlay::TOAST_PAD;
            let text = Type::Body.line(ui, &t.text, p.t1);
            let action = t
                .action
                .as_ref()
                .map(|a| (a, Type::BodyStrong.line(ui, &a.label, p.accent_text)));
            let action_w = action
                .as_ref()
                .map_or(0.0, |(_, g)| g.size().x + overlay::TOAST_ACTION_PAD * 2.0);
            let w = pad
                + text.size().x
                + overlay::TOAST_TEXT_END
                + if action.is_some() {
                    overlay::TOAST_GAP + action_w
                } else {
                    0.0
                }
                + end;
            let h = control::TOAST;
            let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
            let corner = CornerRadius::same(radius::PILL);
            let painter = ui.painter();
            painter.add(p.shade_pop.shadow().as_shape(rect, corner));
            painter.add(Shape::rect_filled(rect, corner, p.raised));
            painter.add(Shape::rect_stroke(
                rect,
                corner,
                Stroke::new(stroke::BORDER, p.line),
                StrokeKind::Inside,
            ));
            painter.galley(
                pos2(rect.left() + pad, rect.center().y - text.size().y / 2.0),
                text,
                p.t1,
            );
            a11y::labelled(ctx, resp.id, egui::Role::Status, &t.text);
            if let Some((a, g)) = action {
                let ah = control::TOAST_ACTION;
                let ar = Rect::from_min_size(
                    pos2(rect.right() - end - action_w, rect.center().y - ah / 2.0),
                    vec2(action_w, ah),
                );
                let r = ui.interact(ar, id.with("action"), Sense::click());
                a11y::button(&r, &a.label);
                let hover = hover_t(&r, &theme);
                let pressed = press_t(&r, &theme);
                let fill = mix(fade(p.o2, hover), p.o3, pressed);
                ui.painter().rect_filled(ar, corner, fill);
                ui.painter().galley(
                    pos2(
                        ar.center().x - g.size().x / 2.0,
                        ar.center().y - g.size().y / 2.0,
                    ),
                    g,
                    p.accent_text,
                );
                if r.has_focus() {
                    focus_ring(ui.painter(), ar, corner, &theme);
                }
                if r.clicked() && !modal_open {
                    out.actions.push((t.id, a.key));
                    acted = true;
                }
                hold |= r.has_focus();
            }
            hold |= ui.rect_contains_pointer(rect);
        });
    track_exit(
        ctx,
        id.with("exit"),
        LayerId::new(order, id),
        ExitKind::Toast,
    );
    queue.hold(t.id, hold, now);
    if acted {
        queue.dismiss(t.id, now);
    }
    paint_exits(ctx);
    out
}
