//! ResetPrefix (SVC-007, LOGIC_SPECS §12): the game's flow lock is held until its files are replaced.
use crate::library::ResetOptions;
use crate::library::prefix_ops::{self, ResetReport};
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn reset_prefix(&mut self, id: GameId, options: ResetOptions) -> Result<()> {
        ensure!(!self.is_busy(id), SesAlreadyRunning, game = id);
        self.lib.begin_flow(id)?;
        let internal = self.lib.reset_inputs(id).inspect_err(|_| {
            self.lib.end_flow(id);
        })?;
        let fs = Arc::clone(&self.deps.fs);
        let paths = self.deps.paths.clone();
        let stamp = self.deps.clock.now().as_millis();
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("reset-prefix"),
            move |_| {
                let result = prefix_ops::reset_files(&*fs, &paths, id, options, internal, stamp);
                post(Internal::Flow(Box::new(FlowDone::Reset {
                    game: id,
                    result,
                })));
            },
        );
        Ok(())
    }

    pub(crate) fn on_reset(&mut self, id: GameId, result: Result<ResetReport>) -> Result<()> {
        let r = result.and_then(|_| self.lib.finish_reset(id));
        self.lib.end_flow(id);
        r?;
        self.publish_library();
        self.notice(Notice::PrefixReset { game: id });
        Ok(())
    }
}
