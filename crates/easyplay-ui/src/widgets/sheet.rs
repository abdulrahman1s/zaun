//! Sheet: centred, max 720 wide, `bg.overlay`, `r.lg`, header (title, close), scrollable
//! body, sticky footer with actions right-aligned (§7.5). The backdrop closes it only when
//! there is no unsaved input.

use egui::{Align, Context, Id, Layout, Modal, ScrollArea, Ui};

use super::icon_button::IconButton;
use super::modal::{closed, overlay_frame, remember_invoker};
use crate::a11y;
use crate::strings;
use crate::theme::palette::fade;
use crate::theme::tokens::{layout, space};
use crate::theme::{self, Dur, Type, icons};

/// What happened this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SheetResponse<R> {
    /// The user asked to close (close button, `Esc`, or backdrop when allowed).
    pub close_requested: bool,
    /// The footer's return value.
    pub footer: R,
}

/// Show a sheet. When `close_requested` is true the caller removes the overlay; focus returns
/// to the invoker automatically.
pub fn sheet<R>(
    ctx: &Context,
    id: Id,
    title: &str,
    unsaved: bool,
    body: impl FnOnce(&mut Ui),
    footer: impl FnOnce(&mut Ui) -> R,
) -> SheetResponse<R> {
    remember_invoker(ctx, id);
    let theme = theme::theme(ctx);
    let p = theme.palette.clone();
    let opacity = theme::motion::appear(ctx, id.with("in"), theme.motion.fade(Dur::Base));
    let rise = (1.0 - opacity) * space::S2 * f32::from(u8::from(!theme.motion.reduced));
    let mut close = false;
    let max_w = layout::SHEET_MAX_W.min(ctx.content_rect().width() - space::S8);
    let max_h = ctx.content_rect().height() - space::S12;
    let area = Modal::default_area(id).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, rise));
    let resp = Modal::new(id)
        .area(area)
        .backdrop_color(fade(p.backdrop, opacity))
        .frame(overlay_frame(ctx, max_w))
        .show(ctx, |ui| {
            ui.multiply_opacity(opacity);
            let inner_w = max_w - layout::SHEET_PAD * 2.0;
            ui.set_width(inner_w);
            ui.set_max_height(max_h);
            a11y::labelled(ctx, ui.scope_id(), egui::Role::Dialog, title);
            ui.horizontal(|ui| {
                ui.label(Type::Title.rich(title).color(p.text_primary));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            IconButton::new(icons::CLOSE, strings::feedback::CLOSE).shortcut("Esc"),
                        )
                        .clicked()
                    {
                        close = true;
                    }
                });
            });
            ui.add_space(space::S4);
            let footer_h = layout::SHEET_PAD + space::S12;
            ScrollArea::vertical()
                .max_height((max_h - footer_h - space::S16).max(space::S16))
                .auto_shrink([false, true])
                .show(ui, body);
            ui.add_space(space::S4);
            ui.with_layout(Layout::right_to_left(Align::Center), footer)
                .inner
        });
    if resp.should_close() {
        // Backdrop clicks are ignored while there is unsaved input; Esc still asks.
        let backdrop = resp.backdrop_response.clicked();
        if !(backdrop && unsaved) {
            close = true;
        }
    }
    if close {
        closed(ctx, id);
    }
    SheetResponse {
        close_requested: close,
        footer: resp.inner,
    }
}
