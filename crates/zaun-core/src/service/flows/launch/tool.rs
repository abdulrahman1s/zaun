//! `RunTool`: a tool runs as a `Tool` session in the game's private storage (SVC-005, LOGIC_SPECS §4 `ToolNeedsNetwork`).
//!
//! The runtime is resolved, the session planned (`plan_launch` with `LaunchTarget::Tool`) and
//! handed to a supervisor on the job pool; the session watcher then reports it like a game's.
//! Winetricks on a game without network access first asks for the network for this run.
use super::job_env::JobEnv;
use super::not_installed;
use crate::launch::{LaunchTarget, PlanRequest, plan_launch};
use crate::model::{Game, NetworkAccess, PromptAnswer, PromptKind, Tool};
use crate::runtime::Resolution;
use crate::service::prelude::*;

/// Resolve, plan and start the tool's session (job pool).
fn start_tool(
    env: &JobEnv,
    game: &Game,
    tool: &Tool,
    network: Option<NetworkAccess>,
) -> Result<SessionId> {
    let facts = env.facts();
    let henv = env.host_env();
    let runtime = match env.runtimes.resolve(&game.runtime, Some(game)) {
        Resolution::Ready(rt) => rt,
        Resolution::NeedsProvision(need) => return Err(not_installed(&need)),
        Resolution::Unavailable(r) => {
            return Err(Error::new(r.code()).with("reason", format!("{r:?}")));
        }
    };
    ensure!(
        !env.locks.is_held(&env.paths.prefix_lock(game.id)),
        SesAlreadyRunning,
        game = game.id
    );
    let req = PlanRequest {
        session_id: SessionId::generate(&*env.clock),
        target: LaunchTarget::Tool(tool),
        game: Some(game),
        runtime: Some(&runtime),
        proton_codename: None,
        network_override: network,
        host: &facts,
        env: &henv,
    };
    let planned = plan_launch(&req)?;
    Ok(env.launcher.start(&planned.plan)?.session_id)
}

impl AppService {
    /// `RunTool`; `network` is the consent given through `ToolNeedsNetwork`.
    pub(crate) fn run_tool(
        &mut self,
        id: GameId,
        tool: Tool,
        network: Option<NetworkAccess>,
    ) -> Result<()> {
        tool.validate()?;
        ensure!(!self.is_busy(id), SesAlreadyRunning, game = id);
        let game = self.lib.game(id)?;
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
            return Ok(());
        }
        self.lib.begin_flow(id)?;
        let env = self.job_env(&game);
        let post = self.poster();
        let _ = self
            .deps
            .jobs
            .submit(JobPriority::Interactive, JobLabel("run-tool"), move |_| {
                let result = start_tool(&env, &game, &tool, network);
                post(Internal::Flow(Box::new(FlowDone::Tool {
                    game: id,
                    result,
                })));
            });
        Ok(())
    }

    pub(crate) fn resume_tool(&mut self, then: Continuation, answer: &PromptAnswer) -> Result<()> {
        if let (Continuation::ToolNetwork { game, tool }, PromptAnswer::AllowNetworkOnce) =
            (then, answer)
        {
            self.run_tool(game, tool, Some(NetworkAccess::Full))?;
        }
        Ok(())
    }

    pub(crate) fn on_tool_started(
        &mut self,
        game: GameId,
        result: Result<SessionId>,
    ) -> Result<()> {
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
        Ok(())
    }
}
