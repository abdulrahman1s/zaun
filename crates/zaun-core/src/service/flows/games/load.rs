//! `LoadGame`: a game's full record, answered through its query.
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn load_game(&mut self, id: GameId, reply_to: QueryId) -> Result<()> {
        let result = match self.lib.game(id) {
            Ok(g) => QueryResult::Game(Box::new(g)),
            Err(e) => QueryResult::Failed(Box::new(self.describe(&e, Some(id)))),
        };
        self.emit(Event::QueryResult {
            id: reply_to,
            result,
        });
        Ok(())
    }
}
