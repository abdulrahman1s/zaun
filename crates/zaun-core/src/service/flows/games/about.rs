//! `AboutGame` (ADR-0028): the engine and hints from a game's files, answered through its query.
use crate::error::catalog::{self, DescribeCtx};
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn about_game(&mut self, id: GameId, reply_to: QueryId) -> Result<()> {
        let game = match self.lib.game(id) {
            Ok(g) => g,
            Err(e) => {
                let describe = self.describer(Some(id));
                self.answer_on_pool("about-game", reply_to, move |_| {
                    QueryResult::Failed(Box::new(describe(&e)))
                });
                return Ok(());
            }
        };
        let fs = Arc::clone(&self.deps.fs);
        let clock = Arc::clone(&self.deps.clock);
        self.answer_on_pool("about-game", reply_to, move |_| {
            QueryResult::About(Box::new(crate::identity::about(&*fs, &*clock, &game)))
        });
        Ok(())
    }

    /// A `describe` usable from a job.
    pub(crate) fn describer(
        &self,
        game: Option<GameId>,
    ) -> impl Fn(&Error) -> UserError + Send + 'static {
        let r = self.redactor();
        move |e| {
            let redact = |s: &str| r.redact_str(s);
            catalog::describe(
                e,
                &DescribeCtx {
                    game,
                    session: None,
                    redact: &redact,
                },
            )
        }
    }
}
