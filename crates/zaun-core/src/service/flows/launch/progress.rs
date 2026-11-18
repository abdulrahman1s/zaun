//! Launch progress and the end of the launch job: forward states, then hand off, fail or ask (SVC-005).
use crate::launch::{LaunchError, LaunchPrompt, LaunchState};
use crate::model::{Activity, LaunchStateTag, PromptKind, RuntimeRef};
use crate::service::prelude::*;

use super::failed::not_installed;

/// The launch job's result.
pub(crate) struct LaunchDone {
    pub game: GameId,
    pub launch: LaunchId,
    pub exe: Option<ExecutableId>,
    pub origin: LaunchOrigin,
    pub result: Result<SessionId, LaunchError>,
}

/// What a parked launch needs to start again.
#[derive(Clone, Copy)]
struct Target {
    game: GameId,
    launch: LaunchId,
    exe: Option<ExecutableId>,
    origin: LaunchOrigin,
}

fn tag_of(s: &LaunchState) -> Option<LaunchStateTag> {
    Some(match s {
        LaunchState::Validating => LaunchStateTag::Validating,
        LaunchState::ResolvingRuntime => LaunchStateTag::ResolvingRuntime,
        LaunchState::Provisioning { .. } => LaunchStateTag::Provisioning,
        LaunchState::Preparing { .. } => LaunchStateTag::Preparing,
        LaunchState::AuditingRuntime => LaunchStateTag::AuditingRuntime,
        LaunchState::Planning => LaunchStateTag::Planning,
        LaunchState::Spawning => LaunchStateTag::Spawning,
        LaunchState::Starting { .. } => LaunchStateTag::Starting,
        _ => return None,
    })
}

impl AppService {
    pub(crate) fn on_launch_state(
        &mut self,
        game: GameId,
        launch: LaunchId,
        state: LaunchState,
    ) -> Result<()> {
        if self.flow.cancelled.contains(&launch) {
            return Ok(());
        }
        if let Some(tag) = tag_of(&state) {
            self.set_activity(game, Activity::Launching(tag));
            self.publish_library();
        }
        self.emit(Event::LaunchProgress {
            game,
            launch,
            state,
        });
        Ok(())
    }

    pub(crate) fn on_launch_done(&mut self, d: LaunchDone) -> Result<()> {
        let LaunchDone {
            game,
            launch,
            exe,
            origin,
            result,
        } = d;
        if self.settle_cancelled(game, launch, result.as_ref().ok()) {
            return Ok(());
        }
        self.lib.end_flow(game);
        self.launching.remove(&game);
        let at = Target {
            game,
            launch,
            exe,
            origin,
        };
        match result {
            Ok(session) => {
                self.launches.insert(session, (game, launch));
                self.watch(session);
                self.poke_watcher();
            }
            Err(LaunchError::NeedsProvision(need)) => {
                self.launch_failed(game, launch, &not_installed(&need));
            }
            Err(LaunchError::Prompt(LaunchPrompt::ExeChanged { size, mtime_ns })) => {
                self.park_exe_changed(at, size, mtime_ns);
            }
            Err(LaunchError::Prompt(LaunchPrompt::GrantTargetChanged(changes))) => {
                // Accepting a moved grant target needs the permissions editor (UI-2xx).
                let detail = format!("{} granted folder(s) moved", changes.len());
                self.launch_failed(game, launch, &err!(SbxGrantTargetChanged, detail = detail));
            }
            Err(LaunchError::Failed(e))
                if matches!(
                    e.code,
                    ErrorCode::RtNoLongerInstalled | ErrorCode::RtUnresolvable
                ) =>
            {
                self.park_runtime_unavailable(at);
            }
            Err(LaunchError::Failed(e)) => self.launch_failed(game, launch, &e),
        }
        Ok(())
    }

    fn park_exe_changed(&mut self, at: Target, size: u64, mtime_ns: i64) {
        self.launch_parked(at.game, at.launch);
        let Ok(game) = self.lib.game(at.game) else {
            return;
        };
        let Some(exe) = (match at.exe {
            Some(id) => game.executables.iter().find(|e| e.id == id),
            None => game.primary(),
        }) else {
            return;
        };
        let mut new = exe.fingerprint;
        new.size = size;
        new.mtime_ns = mtime_ns;
        let req = self.prompts.park(
            &*self.deps.clock,
            PromptKind::ExeChanged {
                game: at.game,
                exe: exe.id,
                old: exe.fingerprint,
                new,
            },
            Continuation::LaunchExeChanged {
                game: at.game,
                exe: at.exe,
                origin: at.origin,
            },
        );
        self.emit(Event::Prompt(req));
    }

    fn park_runtime_unavailable(&mut self, at: Target) {
        self.launch_parked(at.game, at.launch);
        let reference = self
            .lib
            .game(at.game)
            .map(|g| g.runtime)
            .unwrap_or(RuntimeRef::Default);
        let req = self.prompts.park(
            &*self.deps.clock,
            PromptKind::RuntimeUnavailable {
                game: at.game,
                reference,
            },
            Continuation::LaunchRuntime {
                game: at.game,
                exe: at.exe,
                origin: at.origin,
            },
        );
        self.emit(Event::Prompt(req));
    }
}
