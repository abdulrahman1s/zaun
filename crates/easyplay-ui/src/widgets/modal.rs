//! Dialogs over `egui::Modal` [F-306] with focus return (UI_DESIGN_SYSTEM §7.5,
//! ACCESSIBILITY §2): max 480 wide; icon, title, body, actions; default button is the safe
//! choice; `Enter` activates it and `Esc` cancels.
//!
//! Focus: when an overlay first appears, the widget that had focus (the invoker) is
//! remembered; [`closed`] hands focus back to it. egui's modal layer keeps pointer input
//! inside the topmost modal.

use egui::{
    Align, Color32, Context, CornerRadius, Frame, Id, Key, Layout, Margin, Modal, Stroke, Ui,
};

use super::button::{Button, Variant};
use crate::a11y;
use crate::theme::palette::fade;
use crate::theme::tokens::{icon as icon_size, layout, radius, space, stroke, travel};
use crate::theme::{self, Dur, Icon, Type, icons};

#[derive(Clone, Copy, Debug, Default)]
struct Invoker(Option<Id>);

fn invoker_key(overlay: Id) -> Id {
    overlay.with("invoker")
}

/// Remember the focused widget when `overlay` first shows (idempotent while it stays open).
pub fn remember_invoker(ctx: &Context, overlay: Id) {
    let key = invoker_key(overlay);
    if ctx.data(|d| d.get_temp::<Invoker>(key)).is_none() {
        let focused = ctx.memory(|m| m.focused());
        ctx.data_mut(|d| d.insert_temp(key, Invoker(focused)));
    }
}

/// Set the invoker explicitly (when focus moved before the overlay opened).
pub fn set_invoker(ctx: &Context, overlay: Id, invoker: Id) {
    ctx.data_mut(|d| d.insert_temp(invoker_key(overlay), Invoker(Some(invoker))));
}

fn pending_key() -> Id {
    Id::unique("easyplay.focus_return")
}

/// The overlay closed: return focus to its invoker. The request is applied by
/// [`apply_focus_return`] at the start of the next frame, once the modal layer is gone
/// (egui keeps focus inside the topmost modal while it is still drawn).
pub fn closed(ctx: &Context, overlay: Id) {
    let key = invoker_key(overlay);
    if let Some(Invoker(Some(id))) = ctx.data_mut(|d| d.remove_temp::<Invoker>(key)) {
        ctx.data_mut(|d| d.insert_temp(pending_key(), Invoker(Some(id))));
        ctx.request_repaint();
    }
}

/// Apply a pending focus return. Call once at the start of every frame (the shell does).
pub fn apply_focus_return(ctx: &Context) {
    // Wait until egui no longer considers a modal layer topmost; focus requests for widgets
    // below a modal are ignored.
    if ctx.memory(|m| m.top_modal_layer()).is_some() {
        if ctx.data(|d| d.get_temp::<Invoker>(pending_key())).is_some() {
            ctx.request_repaint();
        }
        return;
    }
    if let Some(Invoker(Some(id))) = ctx.data_mut(|d| d.remove_temp::<Invoker>(pending_key())) {
        ctx.memory_mut(|m| m.request_focus(id));
    }
}

/// The invoker recorded for `overlay` (tests).
pub fn invoker(ctx: &Context, overlay: Id) -> Option<Id> {
    ctx.data(|d| d.get_temp::<Invoker>(invoker_key(overlay)))
        .and_then(|i| i.0)
}

/// The modal frame used by dialogs and sheets.
pub fn overlay_frame(ctx: &Context, max_w: f32) -> Frame {
    let theme = theme::theme(ctx);
    let p = &theme.palette;
    let _ = max_w;
    Frame::NONE
        .fill(p.bg_overlay)
        .stroke(Stroke::new(stroke::BORDER, p.hairline))
        .corner_radius(CornerRadius::same(radius::LG))
        .shadow(ctx.global_style().visuals.window_shadow)
        .inner_margin(Margin::same(layout::SHEET_PAD as i8))
}

/// Show `content` in the overlay frame with the entrance of dialogs and sheets: the panel
/// fades in while it rises 8 px and grows from 96 % (USER_EXPERIENCE §10); the backdrop
/// only fades. `opacity` is the 0–1 progress from [`theme::motion::appear`]. Call inside a
/// `Modal` whose own frame is `Frame::NONE`.
pub fn overlay_panel<R>(
    ui: &mut Ui,
    max_w: f32,
    opacity: f32,
    content: impl FnOnce(&mut Ui) -> R,
) -> R {
    let ctx = ui.ctx().clone();
    let theme = theme::theme(&ctx);
    let rest = 1.0 - opacity;
    let (scale, rise) = if theme.motion.reduced {
        (1.0, 0.0)
    } else {
        (1.0 - (1.0 - travel::POP_FROM) * rest, travel::RISE * rest)
    };
    let transform =
        theme::motion::scale_about(ctx.content_rect().center(), scale, egui::vec2(0.0, rise));
    ui.with_visual_transform(transform, |ui| {
        overlay_frame(&ctx, max_w)
            .multiply_with_opacity(opacity)
            .show(ui, |ui| {
                ui.multiply_opacity(opacity);
                content(ui)
            })
            .inner
    })
    .inner
}

/// A dialog action.
#[derive(Clone, Copy, Debug)]
pub struct Action<'a> {
    /// Verb label ("Remove game").
    pub label: &'a str,
    /// Emphasis.
    pub variant: Variant,
}

/// What happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogOutcome {
    /// Still open.
    Open,
    /// An action was chosen (index into `actions`).
    Chose(usize),
    /// Dismissed with `Esc` or the backdrop: treat as the safe choice.
    Cancelled,
}

/// Show a dialog. `default` is the index of the safe action (activated by `Enter`; never a
/// destructive one). Actions are laid out right-aligned, the last rightmost.
pub fn dialog(
    ctx: &Context,
    id: Id,
    icon: Option<Icon>,
    title: &str,
    body: &str,
    actions: &[Action<'_>],
    default: usize,
) -> DialogOutcome {
    let p = theme::theme(ctx).palette.clone();
    rich_dialog(ctx, id, icon, title, actions, default, |ui| {
        ui.label(Type::Body.rich(body).color(p.text_secondary));
    })
}

/// A dialog with a custom body (checkbox lists, details disclosure). The dialog's accessible
/// description is its first body line; `Enter` chooses `default` unless a control inside has
/// focus; `Esc` and the backdrop cancel.
pub fn rich_dialog(
    ctx: &Context,
    id: Id,
    icon: Option<Icon>,
    title: &str,
    actions: &[Action<'_>],
    default: usize,
    body: impl FnOnce(&mut Ui),
) -> DialogOutcome {
    remember_invoker(ctx, id);
    let theme = theme::theme(ctx);
    let p = theme.palette.clone();
    let opacity = theme::motion::appear(ctx, id.with("in"), theme.motion.fade(Dur::Base));
    let mut outcome = DialogOutcome::Open;
    // Three actions with long verbs need more room than the standard width.
    let want = if actions.len() >= 3 {
        layout::DIALOG_MAX_W + space::S16 + space::S6
    } else {
        layout::DIALOG_MAX_W
    };
    let max_w = want.min(ctx.content_rect().width() - space::S8);
    let resp = Modal::new(id)
        .backdrop_color(fade(p.backdrop, opacity))
        .frame(Frame::NONE)
        .show(ctx, |ui| {
            overlay_panel(ui, max_w, opacity, |ui| {
                ui.set_max_width(max_w - layout::SHEET_PAD * 2.0);
                ui.set_min_width((max_w - layout::SHEET_PAD * 2.0).min(360.0));
                ui.spacing_mut().item_spacing.y = space::S3;
                if let Some(icon) = icon {
                    ui.label(icons::icon(icon, icon_size::NAV + 4.0).color(p.text_secondary));
                }
                ui.label(Type::Title.rich(title).color(p.text_primary));
                a11y::labelled(ctx, ui.scope_id(), egui::Role::Dialog, title);
                egui::ScrollArea::vertical()
                    .max_height((ctx.content_rect().height() - 220.0).max(120.0))
                    .auto_shrink([false, true])
                    .show(ui, body);
                ui.add_space(space::S2);
                ui.with_layout(
                    Layout::right_to_left(Align::Center).with_main_wrap(true),
                    |ui| {
                        for (i, a) in actions.iter().enumerate().rev() {
                            let r = ui.add(Button::new(a.label).variant(a.variant));
                            if i == default && ui.memory(|m| m.focused().is_none()) {
                                r.request_focus();
                            }
                            if i == default && r.has_focus() {
                                // `Enter` chooses this one: always marked.
                                super::default_ring(
                                    ui.painter(),
                                    r.rect,
                                    CornerRadius::same(radius::MD),
                                    &theme,
                                );
                            }
                            if r.clicked() {
                                outcome = DialogOutcome::Chose(i);
                            }
                        }
                    },
                );
            });
        });
    if outcome == DialogOutcome::Open
        && resp.is_top_modal
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter))
        && ctx.memory(|m| m.focused()).is_none()
    {
        outcome = DialogOutcome::Chose(default);
    }
    if outcome == DialogOutcome::Open && resp.should_close() {
        outcome = DialogOutcome::Cancelled;
    }
    if outcome != DialogOutcome::Open {
        closed(ctx, id);
    }
    let _ = Color32::TRANSPARENT;
    outcome
}

/// Lay out a footer row of actions right-aligned (primary rightmost). Returns the clicked
/// index.
pub fn action_row(ui: &mut Ui, actions: &[Action<'_>]) -> Option<usize> {
    let mut clicked = None;
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        for (i, a) in actions.iter().enumerate().rev() {
            if ui.add(Button::new(a.label).variant(a.variant)).clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}
