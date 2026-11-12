//! Queries answered on the job pool (folder scan, prefix size) and statistics edits
//! (STATISTICS §9).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::super::app_service::{AppService, Internal};
use super::super::event::{Notice, QueryResult};
use super::FlowDone;
use crate::error::Error;
use crate::ids::{GameId, QueryId, SessionId};
use crate::jobs::{JobLabel, JobPriority};

/// Time budget of a prefix measurement.
pub const MEASURE_BUDGET: Duration = Duration::from_secs(10);

impl AppService {
    pub(crate) fn answer_on_pool(
        &self,
        label: &'static str,
        id: QueryId,
        f: impl FnOnce(&crate::jobs::CancelToken) -> QueryResult + Send + 'static,
    ) {
        let post = self.poster();
        let _ = self
            .deps
            .jobs
            .submit(JobPriority::Interactive, JobLabel(label), move |c| {
                let result = f(c);
                post(Internal::Flow(Box::new(FlowDone::Query { id, result })));
            });
    }

    /// `ScanFolder` (GAME_LIBRARY §11).
    pub(crate) fn scan_folder(&mut self, folder: PathBuf, reply_to: QueryId) {
        let fs = Arc::clone(&self.deps.fs);
        let clock = Arc::clone(&self.deps.clock);
        self.answer_on_pool("scan-folder", reply_to, move |_| {
            QueryResult::Folder(crate::library::folder_scan::scan(&*fs, &*clock, &folder))
        });
    }

    /// `MeasurePrefix`: size of the game's private storage (prefix and private home).
    pub(crate) fn measure_prefix(&mut self, id: GameId, reply_to: QueryId) {
        let fs = Arc::clone(&self.deps.fs);
        let clock = Arc::clone(&self.deps.clock);
        let dir = self.deps.paths.game_dir(id);
        let describe = self.describer(Some(id));
        self.answer_on_pool("measure-prefix", reply_to, move |cancel| {
            match crate::library::prefix_ops::tree_size(&*fs, &*clock, &dir, MEASURE_BUDGET, cancel)
            {
                Ok(r) => QueryResult::PrefixSize {
                    bytes: r.bytes,
                    complete: r.complete,
                },
                Err(e) => QueryResult::Failed(Box::new(describe(&e))),
            }
        });
    }

    /// A `describe` usable from a job.
    pub(crate) fn describer(
        &self,
        game: Option<GameId>,
    ) -> impl Fn(&Error) -> crate::error::UserError + Send + 'static {
        let r = self.redactor();
        move |e| {
            let redact = |s: &str| r.redact_str(s);
            crate::error::catalog::describe(
                e,
                &crate::error::catalog::DescribeCtx {
                    game,
                    session: None,
                    redact: &redact,
                },
            )
        }
    }

    /// `ClearStatistics` (`None`) and `ResetGameStatistics`.
    pub(crate) fn clear_statistics(&mut self, game: Option<GameId>) {
        let r = self.lib.db_mut().transaction(|tx| match game {
            Some(g) => crate::stats::reset_game(tx, g),
            None => crate::stats::clear_all(tx),
        });
        match r {
            Ok(sessions) => {
                let refreshed = match game {
                    Some(g) => self.lib.refresh_game(g),
                    None => self.lib.reload(),
                };
                if let Err(e) = refreshed {
                    tracing::warn!(error = %e, "library refresh after a statistics edit failed");
                }
                self.publish_library();
                self.notice(Notice::StatisticsCleared { game, sessions });
            }
            Err(e) => self.fail(e.into()),
        }
    }

    /// `RemoveSession`: delete one ended session from history.
    pub(crate) fn remove_session(&mut self, id: SessionId) {
        let game = {
            use crate::storage::SessionRepo;
            self.lib
                .db_mut()
                .session(id)
                .ok()
                .flatten()
                .and_then(|s| s.game_id)
        };
        let r = self
            .lib
            .db_mut()
            .transaction(|tx| crate::stats::delete_session(tx, id));
        match r {
            Ok(true) => {
                if let Some(g) = game
                    && let Err(e) = self.lib.refresh_game(g)
                {
                    tracing::warn!(error = %e, "game refresh after removing a session failed");
                }
                self.publish_library();
                self.notice(Notice::SessionRemoved { session: id });
            }
            Ok(false) => {}
            Err(e) => self.fail(e.into()),
        }
    }
}
