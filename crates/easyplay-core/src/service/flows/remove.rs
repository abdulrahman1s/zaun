//! RemoveGame and ResetPrefix (SVC-007, LOGIC_SPECS §11–12), including stop-then-remove.

use super::super::app_service::AppService;
use super::super::command::StopMode;
use super::super::event::{Event, Notice};
use super::super::prompts::Continuation;
use crate::error::{Error, ErrorCode};
use crate::ids::GameId;
use crate::library::remove::NoHooks;
use crate::library::{RemoveOptions, ResetOptions};
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
        let r = self.lib.remove(id, options, &NoHooks);
        self.lib.end_flow(id);
        match r {
            Ok(report) => {
                self.publish_library();
                self.notice(Notice::GameRemoved {
                    name: report.name,
                    saved_data: report.saved_data,
                });
            }
            Err(e) => {
                self.publish_library();
                self.fail(e);
            }
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
                None => self.remove_game(game, options),
            }
        }
    }

    pub(crate) fn reset_prefix(&mut self, id: GameId, options: ResetOptions) {
        if self.running_session(id).is_some() {
            return self.fail(Error::new(ErrorCode::SesAlreadyRunning).with("game", id));
        }
        if let Err(e) = self.lib.begin_flow(id) {
            return self.fail(e);
        }
        let r = self.lib.reset_prefix(id, options);
        self.lib.end_flow(id);
        match r {
            Ok(_) => {
                self.publish_library();
                self.notice(Notice::PrefixReset { game: id });
            }
            Err(e) => self.fail(e),
        }
    }
}
