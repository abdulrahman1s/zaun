//! OpenPaths (SVC-003, LOGIC_SPECS §1): classify each path on the job pool, then act on its match.
use std::path::PathBuf;

use super::classify::Classified;
use crate::model::{ExeRole, Fingerprint, PeInfo};
use crate::service::prelude::*;

/// Most paths handled per request.
pub const MAX_PATHS: usize = 64;

/// The classification job's result.
pub(crate) struct Opened {
    pub path: PathBuf,
    pub origin: OpenOrigin,
    pub result: Result<Classified>,
}

impl AppService {
    pub(crate) fn open_paths(&mut self, mut paths: Vec<PathBuf>, origin: OpenOrigin) -> Result<()> {
        if paths.len() > MAX_PATHS {
            paths.truncate(MAX_PATHS);
            self.notice(Notice::OnlyFirstPaths {
                processed: MAX_PATHS,
            });
        }
        for path in paths {
            let inputs = self.draft_inputs();
            let post = self.poster();
            let _ =
                self.deps
                    .jobs
                    .submit(JobPriority::Interactive, JobLabel("open-path"), move |_| {
                        let result = inputs.classify(&path);
                        let opened = Opened {
                            path,
                            origin,
                            result,
                        };
                        post(Internal::Opened(Box::new(opened)));
                    });
        }
        Ok(())
    }

    pub(super) fn launch_or_show(
        &mut self,
        game: GameId,
        exe: Option<ExecutableId>,
        origin: OpenOrigin,
    ) -> Result<()> {
        if !origin.launches_known() {
            self.emit(Event::ShowGame(game));
            self.notice(Notice::AlreadyInLibrary { game });
            return Ok(());
        }
        let launch_origin = if origin == OpenOrigin::Cli {
            LaunchOrigin::Cli
        } else {
            LaunchOrigin::QuickLaunch
        };
        self.start_launch(game, exe, launch_origin);
        Ok(())
    }

    /// Store a fresh fingerprint (and PE information) for an executable.
    pub(crate) fn update_fingerprint(
        &mut self,
        exe: ExecutableId,
        fp: Fingerprint,
        pe: Option<PeInfo>,
    ) -> Result<()> {
        use crate::storage::ExecutableRepo;
        let Some((game, mut row)) = self.lib.db_mut().executable(exe)? else {
            bail!(LibExeNotFound, exe = exe);
        };
        row.fingerprint = fp;
        if let Some(pe) = pe {
            row.pe = pe;
        }
        self.lib.db_mut().update_executable(&row)?;
        self.lib.refresh_game(game)?;
        self.publish_library();
        Ok(())
    }

    /// Answers for the open-flow prompts.
    pub(crate) fn resume_open(&mut self, then: Continuation, answer: &PromptAnswer) -> Result<()> {
        match (then, answer) {
            (_, PromptAnswer::Cancel) => Ok(()),
            (
                Continuation::OpenChanged {
                    game,
                    exe,
                    fingerprint,
                    pe,
                    origin,
                    ..
                },
                PromptAnswer::ItsAnUpdate,
            ) => {
                self.update_fingerprint(exe, fingerprint, Some(*pe))?;
                self.launch_or_show(game, Some(exe), origin)
            }
            (
                Continuation::OpenChanged { path, origin, .. }
                | Continuation::OpenAttach { path, origin, .. },
                PromptAnswer::SetUpAsNew,
            ) => {
                // The path leaves the old game first (rule 2/3); for InsideKnownRoot and
                // CopyOfKnown it belongs to no game.
                self.set_up_as_new(path, origin)
            }
            (
                Continuation::OpenAttach { path, game, origin },
                PromptAnswer::RunWithGameSettings { .. } | PromptAnswer::UseSameSettings,
            ) => {
                let exe = self.lib.add_executable(game, &path, ExeRole::Alternate)?;
                self.publish_library();
                self.launch_or_show(game, Some(exe), origin)
            }
            (other, a) => {
                tracing::warn!(?a, ?other, "unexpected answer for an open prompt");
                Ok(())
            }
        }
    }
}
