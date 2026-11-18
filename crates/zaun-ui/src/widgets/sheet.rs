//! A sheet: the design has no sheets, so a sheet is the wide dialog (560) with a close
//! button (×) and its actions right-aligned under a scrolling body
//! ([`super::modal::show`]). A click on the scrim closes it only when there is no unsaved
//! input.

use egui::{Context, Id, Ui};

use super::modal::{Spec, show};

/// What happened this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SheetResponse<R> {
    /// The user asked to close (close button, `Esc`, or the scrim when allowed).
    pub close_requested: bool,
    /// The footer's return value.
    pub footer: R,
}

/// Show a sheet: the wide dialog titled `title`. `footer` lays out the actions right to
/// left (the main one first). When `close_requested` is true the caller removes the
/// overlay; it leaves on its own and focus returns to the invoker.
pub fn sheet<R>(
    ctx: &Context,
    id: Id,
    title: &str,
    unsaved: bool,
    body: impl FnOnce(&mut Ui),
    footer: impl FnOnce(&mut Ui) -> R,
) -> SheetResponse<R> {
    let shown = show(
        ctx,
        id,
        Spec::new(title).wide().close_button().unsaved(unsaved),
        body,
        footer,
    );
    SheetResponse {
        close_requested: shown.close_requested,
        footer: shown.footer,
    }
}
