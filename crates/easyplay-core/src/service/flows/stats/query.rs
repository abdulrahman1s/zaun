//! `QueryStats`: run a statistics request on a read-only connection on the job pool (PERFORMANCE §2).
use std::sync::PoisonError;

use crate::config::schema::WeekStart;
use crate::error::catalog::{self, DescribeCtx};
use crate::service::prelude::*;
use crate::stats::rollup::{key_date, local_day};
use crate::stats::{QueryContext, StatsRequest, queries};
use crate::storage::ReadDb;

impl AppService {
    pub(crate) fn query_stats(&mut self, req: &StatsRequest, reply_to: QueryId) -> Result<()> {
        let Some(ctx) = self.stats_context(req) else {
            return Ok(());
        };
        let db = Arc::clone(&self.flow.stats_db);
        let file = self.deps.paths.db_file.clone();
        let req = req.clone();
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("stats-query"),
            move |_| {
                let mut guard = db.lock().unwrap_or_else(PoisonError::into_inner);
                if guard.is_none() {
                    match ReadDb::open(&file) {
                        Ok(d) => *guard = Some(d),
                        Err(e) => tracing::warn!(error = %e, "statistics connection failed"),
                    }
                }
                let result = match guard.as_mut() {
                    Some(d) => queries::run(d, &ctx, &req).map_err(Error::from),
                    None => Err(err!(DbBusy)),
                };
                let result = match result {
                    Ok(r) => QueryResult::Stats(r),
                    Err(e) => QueryResult::Failed(Box::new(describe_plain(&e))),
                };
                post(Internal::Flow(Box::new(FlowDone::Query {
                    id: reply_to,
                    result,
                })));
            },
        );
        Ok(())
    }

    /// Today and the first weekday. The UI computes ranges in local time, so a range's end
    /// date is "today" for streaks.
    fn stats_context(&self, req: &StatsRequest) -> Option<QueryContext> {
        let today = match req {
            StatsRequest::LibraryOverview { range }
            | StatsRequest::GameOverview { range, .. }
            | StatsRequest::TopGames { range, .. }
            | StatsRequest::WeekdayDistribution { range } => Some(range.to),
            StatsRequest::SessionHistory { .. } => None,
        }
        .or_else(|| key_date(local_day(self.deps.clock.now(), 0)))?;
        let week_start = match self.prefs.statistics.week_starts_on {
            WeekStart::Sunday => jiff::civil::Weekday::Sunday,
            WeekStart::System | WeekStart::Monday => jiff::civil::Weekday::Monday,
        };
        Some(QueryContext { today, week_start })
    }
}

/// Describe on the job pool, where the service's redactor is not at hand.
fn describe_plain(e: &Error) -> crate::error::UserError {
    let redact = |s: &str| s.to_owned();
    catalog::describe(
        e,
        &DescribeCtx {
            game: None,
            session: None,
            redact: &redact,
        },
    )
}
