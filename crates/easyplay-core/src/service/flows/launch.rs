//! Launch and StopSession (SVC-005, LOGIC_SPECS §3, §6).
//!
//! The orchestrator (`launch::launch`) runs on the job pool: preflight reads the disk and the
//! hand-off waits for the supervisor's row. Its states come back as internal messages and
//! are forwarded as `LaunchProgress`. From `Starting` on, the session belongs to its
//! supervisor; the session watcher reports `Running` and the end, after which the game's
//! statistics are re-read.
//!
//! Provision and Prepare sessions and the audit gate run inside the launch job through
//! `launch::LocalTrustedSteps` (built by `flows::runtime`).

use std::sync::Arc;

use super::super::app_service::{AppService, Internal};
use super::super::command::{LaunchOrigin, StopMode};
use super::super::event::{Event, Notice};
use super::super::prompts::Continuation;
use super::super::session_watch::SessionPoll;
use crate::error::{Error, ErrorCode};
use crate::identity::fingerprint;
use crate::ids::{ExecutableId, GameId, LaunchId, SessionId};
use crate::jobs::{JobLabel, JobPriority};
use crate::launch::{
    self, LaunchError, LaunchPrompt, LaunchRequest, LaunchState, OrchestratorDeps,
};
use crate::model::{
    Activity, EndReason, LaunchStateTag, PromptAnswer, PromptKind, RuntimeRef, SessionState,
};
use crate::runtime::ProvisionNeed;
use crate::session::StopOutcome;
use crate::storage::GamePatch;

/// The launch job's result.
pub(crate) struct LaunchDone {
    pub game: GameId,
    pub launch: LaunchId,
    pub exe: Option<ExecutableId>,
    pub origin: LaunchOrigin,
    pub result: Result<SessionId, LaunchError>,
}

/// The error of a launch that needs a Provision or Prepare session.
pub fn not_installed(need: &ProvisionNeed) -> Error {
    let mut e = Error::new(ErrorCode::RtNotInstalled).with("protonpath", need.protonpath());
    if let ProvisionNeed::Slr { variant, build } = need {
        e = e.with("variant", variant.as_str()).with("build", build);
    }
    e
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

fn activity_of(s: &crate::model::Session) -> Activity {
    match s.state {
        SessionState::Starting => Activity::Launching(LaunchStateTag::Starting),
        SessionState::Running => Activity::Running {
            since: s.started_at,
        },
        SessionState::Lingering => Activity::Lingering,
        SessionState::Stopping => Activity::Stopping,
        SessionState::Ended => Activity::Idle,
    }
}

impl AppService {
    fn launch_failed(&mut self, game: GameId, launch: LaunchId, err: &Error) {
        tracing::info!(game = %game, code = err.code.as_str(), "launch failed: {err}");
        let u = self.describe(err, Some(game));
        self.set_activity(game, Activity::Idle);
        self.publish_library();
        self.emit(Event::LaunchProgress {
            game,
            launch,
            state: LaunchState::Failed(Box::new(u)),
        });
    }

    pub(crate) fn start_launch(
        &mut self,
        id: GameId,
        exe: Option<ExecutableId>,
        origin: LaunchOrigin,
    ) {
        let launch = LaunchId::generate(&*self.deps.clock);
        if self.sessions.values().any(|s| s.game_id == Some(id)) || self.launching.contains_key(&id)
        {
            return self.launch_failed(
                id,
                launch,
                &Error::new(ErrorCode::SesAlreadyRunning).with("game", id),
            );
        }
        if let Err(e) = self.lib.begin_flow(id) {
            return self.launch_failed(id, launch, &e);
        }
        let game = match self.lib.game(id) {
            Ok(g) => g,
            Err(e) => {
                self.lib.end_flow(id);
                return self.launch_failed(id, launch, &e);
            }
        };
        self.launching.insert(id, launch);
        self.set_activity(id, Activity::Launching(LaunchStateTag::Validating));
        self.publish_library();

        let fs = Arc::clone(&self.deps.fs);
        let clock = Arc::clone(&self.deps.clock);
        let paths = self.deps.paths.clone();
        let env = self.deps.launch_env.clone();
        let install_id = self.install_id.clone();
        let other_roots: Vec<_> = self.lib.exe_index().other_roots(Some(id));
        let runtimes = Arc::clone(&self.deps.runtimes);
        let launcher = Arc::clone(&self.deps.launcher);
        let locks = Arc::clone(&self.deps.locks);
        let host = Arc::clone(&self.deps.host);
        let post = self.poster();
        let steps = self.trusted_steps();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("launch"),
            move |cancel| {
                let facts = host.facts();
                let henv = env.host_env(&paths, &install_id, &other_roots, &*fs);
                let deps = OrchestratorDeps {
                    host: &facts,
                    env: &henv,
                    runtimes: &*runtimes,
                    launcher: &*launcher,
                    locks: &*locks,
                    fs: Arc::clone(&fs),
                    clock: &*clock,
                    steps: Some(&steps),
                    cancel: Some(cancel),
                };
                let session_id = SessionId::generate(&*clock);
                let req = LaunchRequest {
                    game: &game,
                    exe,
                    extra_args: &[],
                    session_id,
                    network_consent: false,
                };
                let mut on_state = |s: &LaunchState| {
                    post(Internal::LaunchState {
                        game: id,
                        launch,
                        state: s.clone(),
                    });
                };
                let result = launch::launch(&req, &deps, &mut on_state).map(|o| {
                    for n in &o.notices {
                        tracing::warn!(code = n.code.as_str(), "{n}");
                    }
                    o.handle.session_id
                });
                post(Internal::Runtimes(runtimes.catalog()));
                post(Internal::LaunchDone(Box::new(LaunchDone {
                    game: id,
                    launch,
                    exe,
                    origin,
                    result,
                })));
            },
        );
    }

    pub(crate) fn on_launch_state(&mut self, game: GameId, launch: LaunchId, state: LaunchState) {
        if let Some(tag) = tag_of(&state) {
            self.set_activity(game, Activity::Launching(tag));
            self.publish_library();
        }
        self.emit(Event::LaunchProgress {
            game,
            launch,
            state,
        });
    }

    pub(crate) fn on_launch_done(&mut self, d: LaunchDone) {
        self.lib.end_flow(d.game);
        self.launching.remove(&d.game);
        match d.result {
            Ok(session) => {
                self.launches.insert(session, (d.game, d.launch));
                self.watch(session);
                self.poke_watcher();
            }
            Err(LaunchError::NeedsProvision(need)) => {
                self.launch_failed(d.game, d.launch, &not_installed(&need));
            }
            Err(LaunchError::Prompt(LaunchPrompt::ExeChanged { size, mtime_ns })) => {
                self.set_activity(d.game, Activity::Idle);
                self.publish_library();
                let Ok(game) = self.lib.game(d.game) else {
                    return;
                };
                let Some(exe) = (match d.exe {
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
                        game: d.game,
                        exe: exe.id,
                        old: exe.fingerprint,
                        new,
                    },
                    Continuation::LaunchExeChanged {
                        game: d.game,
                        exe: d.exe,
                        origin: d.origin,
                    },
                );
                self.emit(Event::Prompt(req));
            }
            Err(LaunchError::Prompt(LaunchPrompt::GrantTargetChanged(changes))) => {
                // Accepting a moved grant target needs the permissions editor (UI-2xx).
                let e = Error::new(ErrorCode::SbxGrantTargetChanged).with(
                    "detail",
                    format!("{} granted folder(s) moved", changes.len()),
                );
                self.launch_failed(d.game, d.launch, &e);
            }
            Err(LaunchError::Failed(e))
                if matches!(
                    e.code,
                    ErrorCode::RtNoLongerInstalled | ErrorCode::RtUnresolvable
                ) =>
            {
                self.set_activity(d.game, Activity::Idle);
                self.publish_library();
                let reference = self
                    .lib
                    .game(d.game)
                    .map(|g| g.runtime)
                    .unwrap_or(RuntimeRef::Default);
                let req = self.prompts.park(
                    &*self.deps.clock,
                    PromptKind::RuntimeUnavailable {
                        game: d.game,
                        reference,
                    },
                    Continuation::LaunchRuntime {
                        game: d.game,
                        exe: d.exe,
                        origin: d.origin,
                    },
                );
                self.emit(Event::Prompt(req));
            }
            Err(LaunchError::Failed(e)) => self.launch_failed(d.game, d.launch, &e),
        }
    }

    /// Answers for the launch prompts.
    pub(crate) fn resume_launch(&mut self, then: Continuation, answer: &PromptAnswer) {
        match (then, answer) {
            (_, PromptAnswer::Cancel) => {}
            (Continuation::LaunchExeChanged { game, exe, origin }, PromptAnswer::ItsAnUpdate) => {
                let refreshed = self.lib.game(game).ok().and_then(|g| {
                    let e = match exe {
                        Some(id) => g.executables.iter().find(|e| e.id == id),
                        None => g.primary(),
                    }?;
                    fingerprint::compute(&*self.deps.fs, &e.canonical_path)
                        .ok()
                        .map(|fp| (e.id, fp))
                });
                match refreshed {
                    Some((exe_id, fp)) => match self.update_fingerprint(exe_id, fp, None) {
                        Ok(()) => self.start_launch(game, exe, origin),
                        Err(e) => self.fail(e),
                    },
                    None => self.fail(Error::new(ErrorCode::LibExeNotFound).with("game", game)),
                }
            }
            (Continuation::LaunchExeChanged { .. }, PromptAnswer::SetUpAsNew) => {
                self.fail(Error::not_available("LIB-007 detach"));
            }
            (Continuation::LaunchRuntime { game, exe, origin }, a) => {
                let reference = match a {
                    PromptAnswer::UseDefaultRuntime => RuntimeRef::Default,
                    PromptAnswer::ChooseRuntime(r) => r.clone(),
                    _ => return,
                };
                let patch = GamePatch {
                    runtime: Some(reference),
                    ..GamePatch::default()
                };
                match self.lib.update(game, patch) {
                    Ok(_) => {
                        self.publish_library();
                        self.start_launch(game, exe, origin);
                    }
                    Err(e) => self.fail(e),
                }
            }
            (other, a) => tracing::warn!(?a, ?other, "unexpected answer for a launch prompt"),
        }
    }

    pub(crate) fn stop_session(&mut self, session: SessionId, mode: StopMode) {
        let row = match self.sessions.get(&session).cloned() {
            Some(s) => s,
            None => {
                use crate::storage::SessionRepo;
                match self.lib.db_mut().session(session) {
                    Ok(Some(s)) if s.is_open() => s,
                    Ok(_) => return, // already ended: idempotent
                    Err(e) => return self.fail(e.into()),
                }
            }
        };
        match self.deps.launcher.request_stop(&row, mode) {
            Ok(StopOutcome::Signalled) => {
                self.notice(Notice::StopRequested { session });
                self.poke_watcher();
            }
            Ok(StopOutcome::SupervisorGone) => self.recover_sessions(),
            Err(e) => self.fail(e),
        }
    }

    /// Close rows whose supervisor died (PROCESS_LIFECYCLE §9) and re-read the games.
    pub(crate) fn recover_sessions(&mut self) {
        let recovered = crate::session::recover(
            self.lib.db_mut(),
            &*self.deps.proc,
            &*self.deps.fs,
            &self.deps.paths,
            &*self.deps.clock,
            &crate::session::RecoveryOptions::default(),
        );
        match recovered {
            Ok(r) if !r.is_empty() => {
                for s in &r {
                    tracing::info!(session = %s.id, reason = s.reason.as_str(), "recovered");
                }
                self.poke_watcher();
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "recovery failed"),
        }
    }

    /// A watcher report: update the registry, activities and statistics.
    pub(crate) fn on_sessions(&mut self, p: SessionPoll) {
        let mut games_changed = Vec::new();
        // Supervisors that died leave open rows: recover them.
        let dead = p.open.iter().any(|s| {
            crate::session::stop::supervisor_identity(s)
                .is_some_and(|id| !self.deps.proc.is_alive(id).unwrap_or(true))
        });
        let previous: Vec<SessionId> = self.sessions.keys().copied().collect();
        self.sessions = p.open.iter().map(|s| (s.id, s.clone())).collect();
        for s in &p.open {
            if let Some(g) = s.game_id {
                let a = activity_of(s);
                if self.lib.overlays().activity_of(g) != a {
                    self.set_activity(g, a);
                    games_changed.push(g);
                }
                if s.state == SessionState::Running
                    && let Some((game, launch)) = self.launches.get(&s.id).copied()
                {
                    self.emit(Event::LaunchProgress {
                        game,
                        launch,
                        state: LaunchState::Running { session: s.id },
                    });
                }
            }
        }
        // Sessions that left the open set without an end row (recovered elsewhere) and
        // the tracked ones that ended.
        for id in previous {
            if !self.sessions.contains_key(&id)
                && !p.ended.iter().any(|s| s.id == id)
                && let Some((g, _)) = self.launches.remove(&id)
            {
                self.set_activity(g, Activity::Idle);
                games_changed.push(g);
            }
        }
        if !p.ended.is_empty() {
            self.after_session_ended();
        }
        for s in &p.ended {
            let Some(g) = s.game_id else { continue };
            self.set_activity(g, Activity::Idle);
            games_changed.push(g);
            if let Some((game, launch)) = self.launches.remove(&s.id) {
                let state = match s.end_reason {
                    Some(EndReason::FailedToStart { code }) => {
                        let u = self.describe(&Error::new(code).with("session", s.id), Some(game));
                        LaunchState::Failed(Box::new(u))
                    }
                    Some(reason) => LaunchState::Finished { reason },
                    None => LaunchState::Finished {
                        reason: EndReason::Interrupted,
                    },
                };
                self.emit(Event::LaunchProgress {
                    game,
                    launch,
                    state,
                });
            }
            if let Some(opts) = self.remove_after_stop.remove(&g) {
                self.remove_game(g, opts);
            }
        }
        for g in &games_changed {
            if let Err(e) = self.lib.refresh_game(*g) {
                tracing::debug!(error = %e, "game refresh after session change failed");
            }
        }
        self.publish_sessions();
        if !games_changed.is_empty() {
            self.publish_library();
        }
        if dead {
            self.recover_sessions();
        }
    }
}
