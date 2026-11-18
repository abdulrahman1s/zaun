//! Remove a game: park while it runs, detach now, purge its files on the pool (SVC-007, LOGIC_SPECS §11).
//!
//! The database steps run on the service thread; deleting files runs on the job pool
//! (PERFORMANCE §2): the game is detached at once and purged when its files are gone.
use crate::library::remove::NoHooks;
use crate::library::{RemoveFiles, RemoveJob, RemoveOptions};
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn remove_game(&mut self, id: GameId, options: RemoveOptions) -> Result<()> {
        if self.is_busy(id) {
            let req = self.prompts.park(
                &*self.deps.clock,
                PromptKind::RemoveRunningGame { game: id },
                Continuation::RemoveRunning { game: id, options },
            );
            self.emit(Event::Prompt(req));
            return Ok(());
        }
        self.lib.begin_flow(id)?;
        let job = self.lib.begin_remove(id, options).inspect_err(|_| {
            self.lib.end_flow(id);
            self.publish_library();
        })?;
        self.publish_library();
        let (fs, paths, post) = (
            Arc::clone(&self.deps.fs),
            self.deps.paths.clone(),
            self.poster(),
        );
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("remove-game"),
            move |_| {
                let files = job.run(&*fs, &paths, &NoHooks);
                post(Internal::Flow(Box::new(FlowDone::Removed { job, files })));
            },
        );
        Ok(())
    }

    pub(crate) fn on_removed(&mut self, job: &RemoveJob, files: RemoveFiles) -> Result<()> {
        let r = self.lib.finish_remove(job, files);
        self.lib.end_flow(job.id);
        self.publish_library();
        let report = r?;
        self.notice(Notice::GameRemoved {
            name: report.name,
            saved_data: report.saved_data,
        });
        Ok(())
    }

    pub(crate) fn resume_remove(
        &mut self,
        then: Continuation,
        answer: &PromptAnswer,
    ) -> Result<()> {
        let (Continuation::RemoveRunning { game, options }, PromptAnswer::StopAndRemove) =
            (then, answer)
        else {
            return Ok(());
        };
        if let Some(s) = self.running_session(game) {
            self.remove_after_stop.insert(game, options);
            self.stop_session(s, StopMode::Graceful);
            return Ok(());
        }
        // Still starting: the removal follows once the cancelled launch has ended
        // (`settle_cancelled`), or its session if one started anyway.
        if self.launching.contains_key(&game) {
            self.remove_after_stop.insert(game, options);
            self.cancel_launch(game);
            return Ok(());
        }
        self.remove_game(game, options)
    }
}
