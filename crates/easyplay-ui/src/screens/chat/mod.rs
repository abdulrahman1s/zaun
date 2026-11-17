//! The Chat page (ADR-0033, SCREEN_SPECS_2 §17): friends, groups and conversations.
//!
//! Stub: the page is built on this module (and `vm::chat`, `strings::chat`).

use egui::Ui;

use super::Env;
use crate::app::UiState;

/// The Chat page.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let _ = (state, env);
    ui.label(crate::strings::nav::CHAT());
}
