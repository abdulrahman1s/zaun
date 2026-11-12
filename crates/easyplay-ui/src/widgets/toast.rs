//! Toast stack: bottom centre, `bg.overlay`, `r.md`, max 420 wide, icon + text + optional
//! action (§7.5). Rendering only; timing lives in [`crate::vm::toasts::ToastQueue`].

use easyplay_core::clock::MonoInstant;
use egui::{Align, Align2, Area, Context, CornerRadius, Frame, Id, Layout, Order, Stroke, vec2};

use super::button::{Button, Size, Variant};
use super::icon_button::IconButton;
use crate::a11y;
use crate::strings;
use crate::theme::tokens::{icon as icon_size, layout, radius, space, stroke};
use crate::theme::{self, Type, icons};
use crate::vm::toasts::{ToastId, ToastKind, ToastQueue};

/// What the user did with the stack this frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToastOutput {
    /// Actions activated: (toast, action key).
    pub actions: Vec<(ToastId, u64)>,
}

/// Paint the visible toasts, update hover/focus holds, handle dismiss and actions.
/// `bottom_inset` keeps the stack above a footer.
pub fn show(
    ctx: &Context,
    queue: &mut ToastQueue,
    now: MonoInstant,
    bottom_inset: f32,
) -> ToastOutput {
    let theme = theme::theme(ctx);
    let p = theme.palette.clone();
    let mut out = ToastOutput::default();
    let toasts: Vec<_> = queue.visible().to_vec();
    if toasts.is_empty() {
        return out;
    }
    let mut dismiss = Vec::new();
    let mut holds = Vec::new();
    // While a dialog or sheet is open, toasts sit below it: the modal keeps keyboard focus
    // (UI-006, ACCESSIBILITY §2) and the toast's buttons wait until it closes.
    let modal_open = ctx.memory(|m| m.top_modal_layer()).is_some();
    // The newest toast slides up 12 px and fades in (USER_EXPERIENCE §10); with reduced
    // motion it only fades.
    let newest = toasts.last().map(|t| t.id);
    let t_in = newest.map_or(1.0, |n| {
        theme::motion::appear(
            ctx,
            Id::unique(("easyplay.toast", n.0)),
            theme.motion.fade(theme::Dur::Base),
        )
    });
    let rise = (1.0 - t_in) * space::S3 * f32::from(u8::from(!theme.motion.reduced));
    Area::new(Id::unique("easyplay.toasts"))
        .order(if modal_open {
            Order::Middle
        } else {
            Order::Foreground
        })
        .anchor(
            Align2::CENTER_BOTTOM,
            vec2(0.0, -(space::S6 + bottom_inset) + rise),
        )
        .interactable(true)
        .show(ctx, |ui| {
            a11y::labelled(
                ctx,
                ui.scope_id(),
                egui::Role::Log,
                strings::feedback::NOTIFICATIONS,
            );
            ui.spacing_mut().item_spacing.y = space::S2;
            for t in &toasts {
                let fading = Some(t.id) == newest && t_in < 1.0;
                let (icon, color) = match t.kind {
                    ToastKind::Info => (icons::INFO, p.info),
                    ToastKind::Success => (icons::CHECK, p.success),
                    ToastKind::Warning => (icons::WARNING, p.warning),
                };
                let r = Frame::NONE
                    .fill(p.bg_overlay)
                    .stroke(Stroke::new(stroke::BORDER, p.border_strong))
                    .corner_radius(CornerRadius::same(radius::MD))
                    .shadow(ctx.global_style().visuals.popup_shadow)
                    .inner_margin(egui::Margin::symmetric(space::S4 as i8, space::S2 as i8))
                    .multiply_with_opacity(if fading { t_in } else { 1.0 })
                    .show(ui, |ui| {
                        if fading {
                            ui.multiply_opacity(t_in);
                        }
                        ui.set_max_width(layout::TOAST_MAX_W - space::S8);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::S3;
                            ui.label(icons::icon(icon, icon_size::BUTTON).color(color));
                            ui.label(Type::Body.rich(&t.text).color(p.text_primary));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(
                                        IconButton::new(icons::CLOSE, strings::feedback::DISMISS)
                                            .size(icon_size::NAV + 4.0)
                                            .icon_size(icon_size::INLINE),
                                    )
                                    .clicked()
                                {
                                    dismiss.push(t.id);
                                }
                                if let Some(a) = &t.action
                                    && ui
                                        .add(
                                            Button::new(&a.label)
                                                .variant(Variant::Link)
                                                .size(Size::Sm),
                                        )
                                        .clicked()
                                {
                                    out.actions.push((t.id, a.key));
                                    dismiss.push(t.id);
                                }
                            });
                        });
                    })
                    .response;
                a11y::labelled(ctx, r.id, egui::Role::Status, &t.text);
                let inside = ui.rect_contains_pointer(r.rect)
                    || ctx.memory(|m| m.focused()).is_some_and(|f| {
                        ctx.read_response(f)
                            .is_some_and(|fr| r.rect.contains_rect(fr.rect))
                    });
                holds.push((t.id, inside));
            }
        });
    for (id, hold) in holds {
        queue.hold(id, hold, now);
    }
    for id in dismiss {
        queue.dismiss(id, now);
    }
    out
}
