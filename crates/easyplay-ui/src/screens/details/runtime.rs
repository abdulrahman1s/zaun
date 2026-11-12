//! Compatibility tab. Arrives with UI-019.

use easyplay_core::ids::GameId;
use egui::{Context, Id};

use crate::app::UiState;
use crate::route::Overlay;
use crate::screens::Env;

/// The game-fix picker sheet.
pub fn fix_picker(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>, game: GameId) {
    let _ = (ctx, id, env);
    state.nav.close(Overlay::FixPicker(game));
}
