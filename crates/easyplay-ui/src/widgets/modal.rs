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
use crate::theme::tokens::{icon as icon_size, layout, radius, space, stroke};
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
        .stroke(Stroke::new(
            stroke::BORDER,
            if p.allows_translucency() {
                p.border_subtle
            } else {
                p.border_strong
            },
        ))
        .corner_radius(CornerRadius::same(radius::LG))
        .shadow(ctx.global_style().visuals.window_shadow)
        .inner_margin(Margin::same(layout::SHEET_PAD as i8))
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
    let opacity = ctx.animate_bool_with_time(id.with("in"), true, theme.motion.fade(Dur::Base));
    let mut outcome = DialogOutcome::Open;
    let max_w = layout::DIALOG_MAX_W.min(ctx.content_rect().width() - space::S8);
    let resp = Modal::new(id)
        .backdrop_color(fade(p.backdrop, opacity))
        .frame(overlay_frame(ctx, max_w))
        .show(ctx, |ui| {
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
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                for (i, a) in actions.iter().enumerate().rev() {
                    let r = ui.add(Button::new(a.label).variant(a.variant));
                    if i == default && ui.memory(|m| m.focused().is_none()) {
                        r.request_focus();
                    }
                    if r.clicked() {
                        outcome = DialogOutcome::Chose(i);
                    }
                }
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
