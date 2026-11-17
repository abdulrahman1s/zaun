//! `ListSessions`: the game's recent sessions of every kind, newest first (DEV-003).
use crate::service::prelude::*;

/// Sessions listed for the selector.
const SESSION_LIST: u64 = 50;

impl AppService {
    pub(crate) fn list_sessions(&mut self, game: GameId, reply_to: QueryId) -> Result<()> {
        let result = match self
            .lib
            .db_mut()
            .session_history(Some(game), &[], 0, SESSION_LIST)
        {
            Ok(mut v) => {
                // Open sessions first: they are the newest.
                for s in self.sessions.values() {
                    if s.game_id == Some(game) && !v.iter().any(|x| x.id == s.id) {
                        v.insert(0, s.clone());
                    }
                }
                QueryResult::Sessions(v)
            }
            Err(e) => QueryResult::Failed(Box::new(self.describe(&e.into(), Some(game)))),
        };
        self.emit(Event::QueryResult {
            id: reply_to,
            result,
        });
        Ok(())
    }
}
