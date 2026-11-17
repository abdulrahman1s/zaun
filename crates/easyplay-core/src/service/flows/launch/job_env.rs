//! What a launch-like job takes from the service onto the pool (SVC-005).
use crate::clock::Clock;
use crate::host::{HostFacts, HostProbe};
use crate::launch::{HostEnv, PrefixLocks};
use crate::model::Game;
use crate::paths::AppPaths;
use crate::runtime::RuntimeService;
use crate::service::app_service::LaunchEnv;
use crate::service::prelude::*;
use crate::session::SessionLauncher;

pub(super) struct JobEnv {
    pub fs: Arc<dyn Fs>,
    pub clock: Arc<dyn Clock>,
    pub paths: AppPaths,
    pub runtimes: Arc<dyn RuntimeService>,
    pub launcher: Arc<dyn SessionLauncher>,
    pub locks: Arc<dyn PrefixLocks>,
    env: LaunchEnv,
    install_id: String,
    other_roots: Vec<PathBuf>,
    host: Arc<dyn HostProbe>,
    developer: bool,
}

impl JobEnv {
    pub fn facts(&self) -> Arc<HostFacts> {
        self.host.facts()
    }

    pub fn host_env(&self) -> HostEnv<'_> {
        let mut env =
            self.env
                .host_env(&self.paths, &self.install_id, &self.other_roots, &*self.fs);
        env.developer_mode = self.developer;
        env
    }
}

impl AppService {
    pub(super) fn job_env(&self, game: &Game) -> JobEnv {
        let developer = crate::sandbox::validate::effective_developer_mode(
            self.prefs.advanced.developer_mode,
            &game.policy,
            &game.launch,
        );
        JobEnv {
            fs: Arc::clone(&self.deps.fs),
            clock: Arc::clone(&self.deps.clock),
            paths: self.deps.paths.clone(),
            runtimes: Arc::clone(&self.deps.runtimes),
            launcher: Arc::clone(&self.deps.launcher),
            locks: Arc::clone(&self.deps.locks),
            env: self.deps.launch_env.clone(),
            install_id: self.install_id.clone(),
            other_roots: self.lib.exe_index().other_roots(Some(game.id)),
            host: Arc::clone(&self.deps.host),
            developer,
        }
    }
}
