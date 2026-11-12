//! RunTool and CancelLaunch (SVC-005, LOGIC_SPECS §3 "Cancellation", §4 `ToolNeedsNetwork`).
//!
//! A tool runs as a `Tool` session in the game's private storage: the runtime is resolved,
//! the session planned (`launch::plan_launch` with `LaunchTarget::Tool`) and handed to a
//! supervisor on the job pool. From then on the session watcher reports it like a game's.
//! Winetricks on a game without network access first asks for the network for this run.

use std::sync::Arc;

use super::super::app_service::{AppService, Internal};
use super::super::command::StopMode;
use super::super::event::{Event, Notice};
use super::super::prompts::Continuation;
use super::FlowDone;
use crate::error::{Error, ErrorCode};
use crate::ids::{GameId, SessionId};
use crate::jobs::{JobLabel, JobPriority};
use crate::launch::{LaunchState, LaunchTarget, PlanRequest, plan_launch};
use crate::model::{NetworkAccess, PromptAnswer, PromptKind, Tool};
use crate::runtime::Resolution;

impl AppService {
    fn busy(&self, id: GameId) -> bool {
        self.sessions.values().any(|s| s.game_id == Some(id)) || self.launching.contains_key(&id)
    }

    /// `RunTool`; `network` is the consent given through `ToolNeedsNetwork`.
    pub(crate) fn run_tool(&mut self, id: GameId, tool: Tool, network: Option<NetworkAccess>) {
        if let Err(e) = tool.validate() {
            return self.fail(e.into());
        }
        if self.busy(id) {
            return self.fail(Error::new(ErrorCode::SesAlreadyRunning).with("game", id));
        }
        let game = match self.lib.game(id) {
            Ok(g) => g,
            Err(e) => return self.fail(e),
        };
        if network.is_none()
            && matches!(tool, Tool::Winetricks { .. })
            && game.policy.network == NetworkAccess::None
        {
            let req = self.prompts.park(
                &*self.deps.clock,
                PromptKind::ToolNeedsNetwork {
                    game: id,
                    tool: tool.clone(),
                    prefix_tainted: game.is_prefix_tainted(),
                },
                Continuation::ToolNetwork { game: id, tool },
            );
            self.emit(Event::Prompt(req));
            return;
        }
        if let Err(e) = self.lib.begin_flow(id) {
            return self.fail(e);
        }
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
        let developer = crate::sandbox::validate::effective_developer_mode(
            self.prefs.advanced.developer_mode,
            &game.policy,
            &game.launch,
        );
        let _ = self
            .deps
            .jobs
            .submit(JobPriority::Interactive, JobLabel("run-tool"), move |_| {
                let result = (|| -> crate::error::Result<SessionId> {
                    let facts = host.facts();
                    let mut henv = env.host_env(&paths, &install_id, &other_roots, &*fs);
                    henv.developer_mode = developer;
                    let runtime = match runtimes.resolve(&game.runtime, Some(&game)) {
                        Resolution::Ready(rt) => rt,
                        Resolution::NeedsProvision(need) => {
                            return Err(super::not_installed(&need));
                        }
                        Resolution::Unavailable(r) => {
                            return Err(Error::new(r.code()).with("reason", format!("{r:?}")));
                        }
                    };
                    if locks.is_held(&paths.prefix_lock(game.id)) {
                        return Err(Error::new(ErrorCode::SesAlreadyRunning).with("game", game.id));
                    }
                    let req = PlanRequest {
                        session_id: SessionId::generate(&*clock),
                        target: LaunchTarget::Tool(&tool),
                        game: Some(&game),
                        runtime: Some(&runtime),
                        proton_codename: None,
                        network_override: network,
                        host: &facts,
                        env: &henv,
                    };
                    let planned = plan_launch(&req)?;
                    Ok(launcher.start(&planned.plan)?.session_id)
                })();
                post(Internal::Flow(Box::new(FlowDone::Tool {
                    game: id,
                    result,
                })));
            });
    }

    pub(crate) fn resume_tool(&mut self, then: Continuation, answer: &PromptAnswer) {
        if let (Continuation::ToolNetwork { game, tool }, PromptAnswer::AllowNetworkOnce) =
            (then, answer)
        {
            self.run_tool(game, tool, Some(NetworkAccess::Full));
        }
    }

    pub(crate) fn on_tool_started(
        &mut self,
        game: GameId,
        result: crate::error::Result<SessionId>,
    ) {
        self.lib.end_flow(game);
        match result {
            Ok(session) => {
                self.watch(session);
                self.poke_watcher();
                self.notice(Notice::ToolStarted { game });
            }
            Err(e) => {
                tracing::info!(game = %game, code = e.code.as_str(), "tool failed: {e}");
                let u = self.describe(&e, Some(game));
                self.emit(Event::Failure(Box::new(u)));
            }
        }
    }

    /// `CancelLaunch`: before the hand-off the launch is dropped (a session that starts
    /// anyway is stopped at once); from `Starting` on it is `StopSession`.
    pub(crate) fn cancel_launch(&mut self, id: GameId) {
        if let Some(launch) = self.launching.get(&id).copied() {
            if self.flow.cancelled.insert(launch) {
                self.set_activity(id, crate::model::Activity::Idle);
                self.publish_library();
                self.emit(Event::LaunchProgress {
                    game: id,
                    launch,
                    state: LaunchState::Cancelled,
                });
                self.notice(Notice::LaunchCancelled { game: id });
            }
            return;
        }
        if let Some(s) = self
            .sessions
            .values()
            .find(|s| s.game_id == Some(id))
            .map(|s| s.id)
        {
            self.stop_session(s, StopMode::Graceful);
        }
    }

    /// A launch job finished after its launch was cancelled: release it, and stop a
    /// session that started anyway. Returns whether `launch` was cancelled.
    pub(crate) fn settle_cancelled(
        &mut self,
        game: GameId,
        launch: crate::ids::LaunchId,
        result: Option<&SessionId>,
    ) -> bool {
        if !self.flow.cancelled.remove(&launch) {
            return false;
        }
        self.lib.end_flow(game);
        self.launching.remove(&game);
        if let Some(session) = result {
            use crate::storage::SessionRepo;
            if let Ok(Some(row)) = self.lib.db_mut().session(*session)
                && let Err(e) = self.deps.launcher.request_stop(&row, StopMode::Force)
            {
                tracing::warn!(error = %e, "stopping a cancelled launch failed");
            }
            self.watch(*session);
            self.poke_watcher();
        }
        self.set_activity(game, crate::model::Activity::Idle);
        self.publish_library();
        true
    }
}
