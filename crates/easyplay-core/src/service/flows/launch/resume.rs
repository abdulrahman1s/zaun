//! Answers for the launch prompts: exe changed, runtime unavailable (SVC-005).
use crate::identity::fingerprint;
use crate::model::{Executable, Game, PromptAnswer, RuntimeRef};
use crate::service::prelude::*;
use crate::storage::GamePatch;

/// The executable `exe` names, or the game's main program.
fn pick(game: &Game, exe: Option<ExecutableId>) -> Option<&Executable> {
    match exe {
        Some(id) => game.executables.iter().find(|e| e.id == id),
        None => game.primary(),
    }
}

impl AppService {
    pub(crate) fn resume_launch(
        &mut self,
        then: Continuation,
        answer: &PromptAnswer,
    ) -> Result<()> {
        match (then, answer) {
            (_, PromptAnswer::Cancel) => {}
            (Continuation::LaunchExeChanged { game, exe, origin }, PromptAnswer::ItsAnUpdate) => {
                let (exe_id, fp) = self
                    .lib
                    .game(game)
                    .ok()
                    .and_then(|g| {
                        let e = pick(&g, exe)?;
                        fingerprint::compute(&*self.deps.fs, &e.canonical_path)
                            .ok()
                            .map(|fp| (e.id, fp))
                    })
                    .ok_or_else(|| err!(LibExeNotFound, game = game))?;
                self.update_fingerprint(exe_id, fp, None)?;
                self.start_launch(game, exe, origin);
            }
            (Continuation::LaunchExeChanged { game, exe, .. }, PromptAnswer::SetUpAsNew) => {
                let path = self
                    .lib
                    .game(game)
                    .ok()
                    .and_then(|g| pick(&g, exe).map(|e| e.canonical_path.clone()))
                    .ok_or_else(|| err!(LibExeNotFound, game = game))?;
                self.set_up_as_new(path, OpenOrigin::Picker)?;
            }
            (Continuation::LaunchRuntime { game, exe, origin }, a) => {
                let reference = match a {
                    PromptAnswer::UseDefaultRuntime => RuntimeRef::Default,
                    PromptAnswer::ChooseRuntime(r) => r.clone(),
                    _ => return Ok(()),
                };
                let patch = GamePatch {
                    runtime: Some(reference),
                    ..GamePatch::default()
                };
                self.lib.update(game, patch)?;
                self.publish_library();
                self.start_launch(game, exe, origin);
            }
            (other, a) => tracing::warn!(?a, ?other, "unexpected answer for a launch prompt"),
        }
        Ok(())
    }
}
