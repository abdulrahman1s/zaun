//! Statistics. Arrives with UI-022.

use egui::{Context, Id, Ui};

use super::Env;
use crate::app::UiState;
use crate::route::Overlay;

/// Show the screen.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let _ = (ui, state, env);
}

/// The full history sheet.
pub fn history_sheet(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>) {
    let _ = (ctx, id, env);
    state.nav.close(Overlay::History);
}
