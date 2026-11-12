//! Protection tab. Arrives with UI-017.

use std::path::PathBuf;

use easyplay_core::ids::GameId;

use crate::app::UiState;
use crate::screens::Env;

/// Share a folder with a game (or the setup draft).
pub fn share_folder(state: &mut UiState, env: &mut Env<'_>, game: Option<GameId>, path: PathBuf) {
    let _ = (state, env, game, path);
}
