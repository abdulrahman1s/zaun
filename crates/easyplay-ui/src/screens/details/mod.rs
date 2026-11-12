//! The game page (SCREEN_SPECS §4). Arrives with UI-014.

pub mod permissions;
pub mod runtime;

use easyplay_core::ids::GameId;
use egui::Ui;

use super::Env;
use crate::app::UiState;

/// Show the game page.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, id: GameId) {
    let _ = (ui, state, env, id);
}
