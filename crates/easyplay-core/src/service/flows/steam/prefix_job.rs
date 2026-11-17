//! The prefix-tool job: plan the `Tool` session and start it on the job pool (ADR-0031 §6).

use std::path::PathBuf;

use super::tool_game::{steam_tool_plan, tool_game};
use crate::clock::Clock;
use crate::fsx::Fs;
use crate::host::HostProbe;
use crate::launch::{LaunchTarget, PlanRequest, PrefixLocks, plan_launch_in_prefix};
use crate::model::Tool;
use crate::paths::AppPaths;
use crate::procfs::ProcSource;
use crate::runtime::{Resolution, RuntimeService};
use crate::service::app_service::LaunchEnv;
use crate::service::prelude::*;
use crate::session::SessionLauncher;
use crate::steam::client::running_games;

/// Everything the job needs, collected on the service thread.
pub(super) struct PrefixJob {
    pub(super) id: GameId,
    pub(super) app: u32,
    pub(super) name: String,
    pub(super) tool: Tool,
    pub(super) program: PathBuf,
    pub(super) root: Option<PathBuf>,
    pub(super) proton: PathBuf,
    pub(super) compat_data: PathBuf,
    pub(super) install_id: String,
    pub(super) env: LaunchEnv,
    pub(super) fs: Arc<dyn Fs>,
    pub(super) clock: Arc<dyn Clock>,
    pub(super) paths: AppPaths,
    pub(super) runtimes: Arc<dyn RuntimeService>,
    pub(super) launcher: Arc<dyn SessionLauncher>,
    pub(super) locks: Arc<dyn PrefixLocks>,
    pub(super) host: Arc<dyn HostProbe>,
    pub(super) proc: Arc<dyn ProcSource>,
}

impl PrefixJob {
    /// Plan the tool session and start it (on the job pool).
    pub(super) fn start(&self) -> Result<SessionId> {
        // Authoritative check: Steam may have started the game meanwhile.
        ensure!(
            !running_games(&*self.proc).contains_key(&self.app),
            StmGameRunning
        );
        ensure!(
            !self.locks.is_held(&self.paths.prefix_lock(self.id)),
            SesAlreadyRunning,
            game = self.id
        );
        // EasyPlay's own folder for this game: the private home and the lock.
        self.fs
            .create_dir_all(&self.paths.home_dir(self.id), 0o700)?;
        let game = tool_game(
            self.id,
            &self.name,
            &self.program,
            self.root.as_deref(),
            &self.proton,
            self.clock.now(),
        );
        let runtime = match self.runtimes.resolve(&game.runtime, Some(&game)) {
            Resolution::Ready(rt) => rt,
            Resolution::NeedsProvision(need) => return Err(super::super::not_installed(&need)),
            Resolution::Unavailable(r) => bail!(StmLayerUnavailable, reason = format!("{r:?}")),
        };
        let facts = self.host.facts();
        let other_roots: Vec<PathBuf> = Vec::new();
        let henv = self
            .env
            .host_env(&self.paths, &self.install_id, &other_roots, &*self.fs);
        let req = PlanRequest {
            session_id: SessionId::generate(&*self.clock),
            target: LaunchTarget::Tool(&self.tool),
            game: Some(&game),
            runtime: Some(&runtime),
            proton_codename: None,
            network_override: None,
            host: &facts,
            env: &henv,
        };
        let mut planned = plan_launch_in_prefix(&req, &self.compat_data)?;
        steam_tool_plan(&mut planned.plan);
        Ok(self.launcher.start(&planned.plan)?.session_id)
    }
}
