//! `ClearStatistics` (`None`) and `ResetGameStatistics` (STATISTICS §9).
use crate::service::prelude::*;
use crate::stats;

impl AppService {
    pub(crate) fn clear_statistics(&mut self, game: Option<GameId>) -> Result<()> {
        let sessions = self.lib.db_mut().transaction(|tx| match game {
            Some(g) => stats::reset_game(tx, g),
            None => stats::clear_all(tx),
        })?;
        let refreshed = match game {
            Some(g) => self.lib.refresh_game(g),
            None => self.lib.reload(),
        };
        if let Err(e) = refreshed {
            tracing::warn!(error = %e, "library refresh after a statistics edit failed");
        }
        self.publish_library();
        self.notice(Notice::StatisticsCleared { game, sessions });
        Ok(())
    }
}
