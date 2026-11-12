//! Arrives with a later task.

use egui::{Context, Id};

use super::Env;
use crate::app::UiState;

/// Show the sheet.
pub fn show(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>) {
    let _ = (ctx, id, state, env);
}
