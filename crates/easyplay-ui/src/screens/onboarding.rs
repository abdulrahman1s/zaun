//! Arrives with a later task.

use egui::Ui;

use super::Env;
use crate::app::UiState;

/// Show the screen.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let _ = (ui, state, env);
}
