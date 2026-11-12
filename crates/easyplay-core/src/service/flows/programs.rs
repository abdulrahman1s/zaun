//! A game's programs from the game page (UI-018, UI-020, LOGIC_SPECS §1, §8): adding a
//! program, *Locate…* for a moved one, and detaching a program for "Set up as a new game".

use std::path::{Path, PathBuf};

use super::super::app_service::AppService;
use super::super::command::OpenOrigin;
use super::super::event::{Event, Notice};
use super::super::prompts::Continuation;
use crate::error::{Error, ErrorCode};
use crate::ids::{ExecutableId, GameId};
use crate::library::RelinkOutcome;
use crate::model::{ExeRole, PromptAnswer, PromptKind};

impl AppService {
    pub(crate) fn add_program(&mut self, id: GameId, path: &Path, role: ExeRole) {
        match self.lib.add_executable(id, path, role) {
            Ok(_) => {
                self.publish_library();
                self.notice(Notice::ProgramAdded { game: id });
            }
            Err(e) => self.fail(e),
        }
    }

    /// *Locate…*: the primary (or `exe`) now lives at `new_path`.
    pub(crate) fn locate_program(
        &mut self,
        id: GameId,
        exe: Option<ExecutableId>,
        new_path: &Path,
    ) {
        let exe = match exe {
            Some(x) => x,
            None => match self.lib.game(id).map(|g| g.primary().map(|e| e.id)) {
                Ok(Some(x)) => x,
                Ok(None) => {
                    return self.fail(Error::new(ErrorCode::LibExeNotFound).with("game", id));
                }
                Err(e) => return self.fail(e),
            },
        };
        self.relink_program(id, exe, new_path, false);
    }

    fn relink_program(&mut self, id: GameId, exe: ExecutableId, path: &Path, confirmed: bool) {
        if let Err(e) = self.lib.begin_flow(id) {
            return self.fail(e);
        }
        let r = self.lib.relink(exe, path, confirmed);
        self.lib.end_flow(id);
        match r {
            Ok(RelinkOutcome::Relinked { game, .. }) => {
                self.publish_library();
                self.notice(Notice::ProgramLocated { game });
            }
            Ok(RelinkOutcome::NeedsConfirmation { game, .. }) => {
                let fp = crate::identity::fingerprint::compute(&*self.deps.fs, path);
                let old = self.lib.game(game).ok().and_then(|g| {
                    g.executables
                        .iter()
                        .find(|e| e.id == exe)
                        .map(|e| e.fingerprint)
                });
                let (Ok(new), Some(old)) = (fp, old) else {
                    return self.fail(Error::new(ErrorCode::LibExeNotFound).with("game", game));
                };
                let req = self.prompts.park(
                    &*self.deps.clock,
                    PromptKind::ExeChanged {
                        game,
                        exe,
                        old,
                        new,
                    },
                    Continuation::Locate {
                        game,
                        exe,
                        path: path.to_path_buf(),
                    },
                );
                self.emit(Event::Prompt(req));
            }
            Err(e) => self.fail(e),
        }
    }

    pub(crate) fn resume_locate(&mut self, then: Continuation, answer: &PromptAnswer) {
        let Continuation::Locate { game, exe, path } = then else {
            return;
        };
        match answer {
            PromptAnswer::ItsAnUpdate => self.relink_program(game, exe, &path, true),
            PromptAnswer::SetUpAsNew => self.draft_job(path, OpenOrigin::Picker),
            _ => {}
        }
    }

    /// "Set up as a new game" (LOGIC_SPECS §1, §4): the file leaves its old game, then a draft
    /// follows.
    pub(crate) fn set_up_as_new(&mut self, path: PathBuf, origin: OpenOrigin) {
        let owner = {
            use crate::storage::ExecutableRepo;
            self.lib.db_mut().executable_by_canonical_path(&path)
        };
        match owner {
            Ok(Some((game, row))) => {
                if self.sessions.values().any(|s| s.game_id == Some(game))
                    || self.launching.contains_key(&game)
                {
                    return self.fail(Error::new(ErrorCode::SesAlreadyRunning).with("game", game));
                }
                if let Err(e) = self.lib.begin_flow(game) {
                    return self.fail(e);
                }
                let r = self.lib.detach_program(row.id);
                self.lib.end_flow(game);
                match r {
                    Ok(_) => self.publish_library(),
                    Err(e) => return self.fail(e),
                }
            }
            Ok(None) => {}
            Err(e) => return self.fail(e.into()),
        }
        self.draft_job(path, origin);
    }
}
