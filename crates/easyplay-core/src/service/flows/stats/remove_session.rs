//! `RemoveSession`: delete one ended session from history (STATISTICS §9).
use crate::service::prelude::*;
use crate::stats;
use crate::storage::SessionRepo;

impl AppService {
    pub(crate) fn remove_session(&mut self, id: SessionId) -> Result<()> {
        let game = self
            .lib
            .db_mut()
            .session(id)
            .ok()
            .flatten()
            .and_then(|s| s.game_id);
        let removed = self
            .lib
            .db_mut()
            .transaction(|tx| stats::delete_session(tx, id))?;
        if !removed {
            return Ok(());
        }
        if let Some(g) = game
            && let Err(e) = self.lib.refresh_game(g)
        {
            tracing::warn!(error = %e, "game refresh after removing a session failed");
        }
        self.publish_library();
        self.notice(Notice::SessionRemoved { session: id });
        Ok(())
    }
}
