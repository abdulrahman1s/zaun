//! *Locate…* for a moved program (UI-020, LOGIC_SPECS §1, §8): relink, or ask when it changed.
use std::path::Path;

use crate::identity::fingerprint;
use crate::library::RelinkOutcome;
use crate::service::prelude::*;

impl AppService {
    /// The primary (or `exe`) now lives at `new_path`.
    pub(crate) fn locate_program(
        &mut self,
        id: GameId,
        exe: Option<ExecutableId>,
        new_path: &Path,
    ) -> Result<()> {
        let exe = match exe {
            Some(x) => x,
            None => self
                .lib
                .game(id)?
                .primary()
                .map(|e| e.id)
                .ok_or_else(|| err!(LibExeNotFound, game = id))?,
        };
        self.relink_program(id, exe, new_path, false)
    }

    fn relink_program(
        &mut self,
        id: GameId,
        exe: ExecutableId,
        path: &Path,
        confirmed: bool,
    ) -> Result<()> {
        self.lib.begin_flow(id)?;
        let r = self.lib.relink(exe, path, confirmed);
        self.lib.end_flow(id);
        match r? {
            RelinkOutcome::Relinked { game, .. } => {
                self.publish_library();
                self.check_availability();
                self.notice(Notice::ProgramLocated { game });
                Ok(())
            }
            RelinkOutcome::NeedsConfirmation { game, .. } => self.ask_relink(game, exe, path),
        }
    }

    /// The file at the new path is not the stored one: ask whether it is an update.
    fn ask_relink(&mut self, game: GameId, exe: ExecutableId, path: &Path) -> Result<()> {
        let new = fingerprint::compute(&*self.deps.fs, path);
        let old = self.lib.game(game).ok().and_then(|g| {
            g.executables
                .iter()
                .find(|e| e.id == exe)
                .map(|e| e.fingerprint)
        });
        let (Ok(new), Some(old)) = (new, old) else {
            bail!(LibExeNotFound, game = game);
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
        Ok(())
    }

    pub(crate) fn resume_locate(
        &mut self,
        then: Continuation,
        answer: &PromptAnswer,
    ) -> Result<()> {
        let Continuation::Locate { game, exe, path } = then else {
            return Ok(());
        };
        match answer {
            PromptAnswer::ItsAnUpdate => self.relink_program(game, exe, &path, true),
            PromptAnswer::SetUpAsNew => {
                self.draft_job(path, OpenOrigin::Picker);
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
