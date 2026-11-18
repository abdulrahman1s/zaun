//! Library edits of a Steam game: favourite, hidden, tags and notes (ADR-0031).
use super::details::app_id;
use crate::ids::TagName;
use crate::library::steam::not_for_steam;
use crate::service::prelude::*;

impl AppService {
    pub(super) fn steam_set_favorite(&mut self, id: GameId, favorite: bool) -> Result<()> {
        self.lib.steam_set_favorite(app_id(id)?, favorite)?;
        self.publish_library();
        Ok(())
    }

    pub(super) fn steam_set_hidden(&mut self, id: GameId, hidden: bool) -> Result<()> {
        self.lib.steam_set_hidden(app_id(id)?, hidden)?;
        self.publish_library();
        Ok(())
    }

    pub(super) fn steam_set_tags(&mut self, id: GameId, tags: &[TagName]) -> Result<()> {
        self.lib.steam_set_tags(app_id(id)?, tags)?;
        self.publish_library();
        Ok(())
    }

    pub(super) fn steam_update_game(&mut self, id: GameId, edits: Vec<GameEdit>) -> Result<()> {
        let app = app_id(id)?;
        for e in edits {
            match e {
                GameEdit::Notes(n) => self.lib.steam_set_notes(app, &n)?,
                // Name, protection, compatibility layer, launch options and programs are
                // Steam's or do not apply.
                GameEdit::Name(_)
                | GameEdit::Policy(_)
                | GameEdit::Launch(_)
                | GameEdit::Runtime { .. }
                | GameEdit::UmuId(_)
                | GameEdit::SteamAppId(_)
                | GameEdit::Preset(_)
                | GameEdit::ResetName
                | GameEdit::Kind(_)
                | GameEdit::ExeRole(..)
                | GameEdit::ExeLabel(..)
                | GameEdit::ExeArgs(..)
                | GameEdit::RemoveExe(_) => return Err(not_for_steam("game edit")),
            }
        }
        self.publish_library();
        Ok(())
    }
}
