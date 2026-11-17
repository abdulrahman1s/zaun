//! `MeasurePrefix`: size of a game's private storage (prefix and private home), answered through its query.
use std::time::Duration;

use crate::library::prefix_ops;
use crate::service::prelude::*;

/// Time budget of a prefix measurement.
pub const MEASURE_BUDGET: Duration = Duration::from_secs(10);

impl AppService {
    pub(crate) fn measure_prefix(&mut self, id: GameId, reply_to: QueryId) -> Result<()> {
        let fs = Arc::clone(&self.deps.fs);
        let clock = Arc::clone(&self.deps.clock);
        let dir = self.deps.paths.game_dir(id);
        let describe = self.describer(Some(id));
        self.answer_on_pool(
            "measure-prefix",
            reply_to,
            move |cancel| match prefix_ops::tree_size(&*fs, &*clock, &dir, MEASURE_BUDGET, cancel) {
                Ok(r) => QueryResult::PrefixSize {
                    bytes: r.bytes,
                    complete: r.complete,
                },
                Err(e) => QueryResult::Failed(Box::new(describe(&e))),
            },
        );
        Ok(())
    }
}
