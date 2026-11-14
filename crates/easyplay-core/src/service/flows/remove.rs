//! RemoveGame and ResetPrefix (SVC-007, LOGIC_SPECS §11–12), including stop-then-remove.
//!
//! The database steps run on the service thread; deleting and renaming files runs on the job
//! pool (PERFORMANCE §2): a removal detaches the game at once and purges it when its files are
//! gone; a reset holds the game's flow lock until its files are replaced.

use std::sync::Arc;

use super::super::app_service::{AppService, Internal};
use super::super::command::StopMode;
use super::super::event::{Event, Notice};
use super::super::prompts::Continuation;
use super::FlowDone;
use crate::error::{Error, ErrorCode};
use crate::ids::GameId;
use crate::jobs::{JobLabel, JobPriority};
use crate::library::remove::NoHooks;
use crate::library::{RemoveFiles, RemoveJob, RemoveOptions, ResetOptions};
use crate::model::{PromptAnswer, PromptKind};

impl AppService {
    fn running_session(&self, id: GameId) -> Option<crate::ids::SessionId> {
        self.sessions
            .values()
            .find(|s| s.game_id == Some(id))
            .map(|s| s.id)
    }

    pub(crate) fn remove_game(&mut self, id: GameId, options: RemoveOptions) {
        if self.running_session(id).is_some() || self.launching.contains_key(&id) {
            let req = self.prompts.park(
                &*self.deps.clock,
                PromptKind::RemoveRunningGame { game: id },
                Continuation::RemoveRunning { game: id, options },
            );
            self.emit(Event::Prompt(req));
            return;
        }
        if let Err(e) = self.lib.begin_flow(id) {
            return self.fail(e);
        }
        let job = match self.lib.begin_remove(id, options) {
            Ok(j) => j,
            Err(e) => {
                self.lib.end_flow(id);
                self.publish_library();
                return self.fail(e);
            }
        };
        self.publish_library();
        let fs = Arc::clone(&self.deps.fs);
        let paths = self.deps.paths.clone();
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("remove-game"),
            move |_| {
                let files = job.run(&*fs, &paths, &NoHooks);
                post(Internal::Flow(Box::new(FlowDone::Removed { job, files })));
            },
        );
    }

    pub(crate) fn on_removed(&mut self, job: &RemoveJob, files: RemoveFiles) {
        let r = self.lib.finish_remove(job, files);
        self.lib.end_flow(job.id);
        self.publish_library();
        match r {
            Ok(report) => self.notice(Notice::GameRemoved {
                name: report.name,
                saved_data: report.saved_data,
            }),
            Err(e) => self.fail(e),
        }
    }

    pub(crate) fn resume_remove(&mut self, then: Continuation, answer: &PromptAnswer) {
        if let (Continuation::RemoveRunning { game, options }, PromptAnswer::StopAndRemove) =
            (then, answer)
        {
            match self.running_session(game) {
                Some(s) => {
                    self.remove_after_stop.insert(game, options);
                    self.stop_session(s, StopMode::Graceful);
                }
                // Still starting: the removal follows once the cancelled launch has ended
                // (`settle_cancelled`), or its session if one started anyway.
                None if self.launching.contains_key(&game) => {
                    self.remove_after_stop.insert(game, options);
                    self.cancel_launch(game);
                }
                None => self.remove_game(game, options),
            }
        }
    }

    pub(crate) fn reset_prefix(&mut self, id: GameId, options: ResetOptions) {
        if self.running_session(id).is_some() || self.launching.contains_key(&id) {
            return self.fail(Error::new(ErrorCode::SesAlreadyRunning).with("game", id));
        }
        if let Err(e) = self.lib.begin_flow(id) {
            return self.fail(e);
        }
        let internal = match self.lib.reset_inputs(id) {
            Ok(i) => i,
            Err(e) => {
                self.lib.end_flow(id);
                return self.fail(e);
            }
        };
        let fs = Arc::clone(&self.deps.fs);
        let paths = self.deps.paths.clone();
        let stamp = self.deps.clock.now().as_millis();
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("reset-prefix"),
            move |_| {
                let result = crate::library::prefix_ops::reset_files(
                    &*fs, &paths, id, options, internal, stamp,
                );
                post(Internal::Flow(Box::new(FlowDone::Reset {
                    game: id,
                    result,
                })));
            },
        );
    }

    pub(crate) fn on_reset(
        &mut self,
        id: GameId,
        result: crate::error::Result<crate::library::prefix_ops::ResetReport>,
    ) {
        let r = result.and_then(|_| self.lib.finish_reset(id));
        self.lib.end_flow(id);
        match r {
            Ok(()) => {
                self.publish_library();
                self.notice(Notice::PrefixReset { game: id });
            }
            Err(e) => self.fail(e),
        }
    }
}
