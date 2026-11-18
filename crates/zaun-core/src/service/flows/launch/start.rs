//! `Launch`: take the game's flow, then run the orchestrator on the pool (SVC-005).
//!
//! Failure is reported through `LaunchProgress::Failed`, so `start_launch` returns nothing.
use super::job_env::JobEnv;
use super::progress::LaunchDone;
use crate::jobs::CancelToken;
use crate::launch::{self, LaunchRequest, LaunchState, LocalTrustedSteps, OrchestratorDeps};
use crate::model::{Activity, Game, LaunchStateTag};
use crate::service::prelude::*;

struct LaunchJob {
    env: JobEnv,
    steps: LocalTrustedSteps,
    game: Game,
    exe: Option<ExecutableId>,
    origin: LaunchOrigin,
    launch: LaunchId,
}

impl LaunchJob {
    fn run(&self, cancel: &CancelToken, post: &dyn Fn(Internal)) {
        let facts = self.env.facts();
        let henv = self.env.host_env();
        let deps = OrchestratorDeps {
            host: &facts,
            env: &henv,
            runtimes: &*self.env.runtimes,
            launcher: &*self.env.launcher,
            locks: &*self.env.locks,
            fs: Arc::clone(&self.env.fs),
            clock: &*self.env.clock,
            steps: Some(&self.steps),
            cancel: Some(cancel),
        };
        let req = LaunchRequest {
            game: &self.game,
            exe: self.exe,
            extra_args: &[],
            session_id: SessionId::generate(&*self.env.clock),
            network_consent: false,
        };
        let (game, launch) = (self.game.id, self.launch);
        let mut on_state = |s: &LaunchState| {
            post(Internal::LaunchState {
                game,
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
        post(Internal::Runtimes(self.env.runtimes.catalog()));
        post(Internal::LaunchDone(Box::new(LaunchDone {
            game,
            launch,
            exe: self.exe,
            origin: self.origin,
            result,
        })));
    }
}

impl AppService {
    pub(crate) fn start_launch(
        &mut self,
        id: GameId,
        exe: Option<ExecutableId>,
        origin: LaunchOrigin,
    ) {
        let launch = LaunchId::generate(&*self.deps.clock);
        let game = match self.begin_launch(id) {
            Ok(g) => g,
            Err(e) => return self.launch_failed(id, launch, &e),
        };
        self.launching.insert(id, launch);
        self.set_activity(id, Activity::Launching(LaunchStateTag::Validating));
        self.publish_library();

        let job = LaunchJob {
            env: self.job_env(&game),
            steps: self.trusted_steps(),
            game,
            exe,
            origin,
            launch,
        };
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("launch"),
            move |cancel| job.run(cancel, &post),
        );
    }

    /// Refuse a game that is busy, then take its flow and load it.
    fn begin_launch(&mut self, id: GameId) -> Result<Game> {
        ensure!(!self.is_busy(id), SesAlreadyRunning, game = id);
        self.lib.begin_flow(id)?;
        self.lib.game(id).inspect_err(|_| self.lib.end_flow(id))
    }
}
