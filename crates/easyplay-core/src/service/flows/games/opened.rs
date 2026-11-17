//! The classification job finished: launch, prompt, show or draft (SVC-003, LOGIC_SPECS §1).
use super::open::Opened;
use crate::identity::matcher::{Change, MatchOutcome};
use crate::model::{Fingerprint, PeInfo};
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn on_opened(&mut self, o: Opened) -> Result<()> {
        let c = o.result.inspect_err(|_| {
            tracing::info!(path = %o.path.display(), "opened path not usable");
        })?;
        let origin = o.origin;
        let (canonical, fp, pe) = (c.canonical, c.fingerprint, c.pe);
        match c.outcome {
            MatchOutcome::Known { game, exe } => self.launch_or_show(game, Some(exe), origin),
            MatchOutcome::KnownChanged {
                game,
                exe,
                change: Change::SameIdentity,
            } => {
                self.update_fingerprint(exe, fp, Some(pe))?;
                self.launch_or_show(game, Some(exe), origin)
            }
            MatchOutcome::KnownChanged {
                game,
                exe,
                change: Change::IdentityDiffers,
            } => {
                self.ask_exe_changed(game, exe, (canonical, fp, pe), origin);
                Ok(())
            }
            MatchOutcome::InsideKnownRoot { game } => {
                let kind = PromptKind::InsideKnownRoot {
                    game,
                    path: canonical.clone(),
                };
                self.ask_attach(kind, canonical, game, origin);
                Ok(())
            }
            MatchOutcome::CopyCandidate { game, .. } => {
                let kind = PromptKind::CopyOfKnown {
                    game,
                    path: canonical.clone(),
                };
                self.ask_attach(kind, canonical, game, origin);
                Ok(())
            }
            MatchOutcome::MovedCandidate { game, exe } => {
                self.lib.relink(exe, &canonical, true)?;
                self.publish_library();
                self.notice(Notice::FoundMoved { game });
                self.launch_or_show(game, Some(exe), origin)
            }
            MatchOutcome::Unknown { .. } => {
                match c.draft {
                    Some(d) => self.request_setup(d, origin),
                    None => self.draft_job(canonical, origin),
                }
                Ok(())
            }
        }
    }

    /// The program differs from the stored one: ask whether it is an update.
    fn ask_exe_changed(
        &mut self,
        game: GameId,
        exe: ExecutableId,
        (path, new, pe): (PathBuf, Fingerprint, PeInfo),
        origin: OpenOrigin,
    ) {
        let old = self
            .lib
            .game(game)
            .ok()
            .and_then(|g| {
                g.executables
                    .iter()
                    .find(|e| e.id == exe)
                    .map(|e| e.fingerprint)
            })
            .unwrap_or(new);
        let req = self.prompts.park(
            &*self.deps.clock,
            PromptKind::ExeChanged {
                game,
                exe,
                old,
                new,
            },
            Continuation::OpenChanged {
                path,
                game,
                exe,
                fingerprint: new,
                pe: Box::new(pe),
                origin,
            },
        );
        self.emit(Event::Prompt(req));
    }

    /// The path is inside, or a copy of, a known game: ask what to do with it.
    fn ask_attach(&mut self, kind: PromptKind, path: PathBuf, game: GameId, origin: OpenOrigin) {
        let req = self.prompts.park(
            &*self.deps.clock,
            kind,
            Continuation::OpenAttach { path, game, origin },
        );
        self.emit(Event::Prompt(req));
    }
}
