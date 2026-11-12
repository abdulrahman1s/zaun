//! The policy explainer the UI calls on its own thread (`Backend::assess`): the planning
//! inputs the service holds, read-only, plus the current host facts from the hub.
//!
//! It never lists a directory or writes; existence checks are `stat`/`realpath` calls of the
//! policy's own paths. See `sandbox::explain::explain_policy`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::app_service::LaunchEnv;
use super::snapshot::SnapshotHub;
use crate::fsx::Fs;
use crate::host::HostFacts;
use crate::ids::GameId;
use crate::model::{LaunchConfig, SandboxPolicy};
use crate::paths::AppPaths;
use crate::runtime::resolver::IsolationInputs;
use crate::sandbox::explain::{PolicyContext, PolicyExplanation, explain_policy};

/// Stand-in game ID for a draft (the explanation does not depend on it).
const DRAFT_GAME: u128 = 0xD8AF_7000_0000_7000_8000_0000_0000_0000;

/// Read-only planning inputs, shared with the UI.
pub struct Explainer {
    pub(crate) fs: Arc<dyn Fs>,
    pub(crate) paths: AppPaths,
    pub(crate) env: LaunchEnv,
}

impl Explainer {
    /// Explain `policy` for `game` (its folder and program from the library snapshot), or
    /// for a draft at `root` running `exe`.
    pub fn explain(
        &self,
        hub: &SnapshotHub,
        game: Option<GameId>,
        root: Option<&Path>,
        exe: Option<&Path>,
        policy: &SandboxPolicy,
    ) -> PolicyExplanation {
        let host = hub.host.load();
        let empty;
        let host: &HostFacts = match host.as_ref() {
            Some(h) => h,
            None => {
                empty = HostFacts::empty(crate::clock::Timestamp::from_millis(0));
                &empty
            }
        };
        let lib = hub.library.load();
        let summary = game.and_then(|g| lib.get(g));
        let root: Option<PathBuf> = summary
            .and_then(|s| s.root.as_deref().map(Path::to_path_buf))
            .or_else(|| root.map(Path::to_path_buf));
        let exe: PathBuf = summary
            .map(|s| s.primary_exe_path.to_path_buf())
            .or_else(|| exe.map(Path::to_path_buf))
            .or_else(|| root.as_ref().map(|r| r.join("game.exe")))
            .unwrap_or_else(|| self.env.home.join("program.exe"));
        let others = lib
            .games
            .iter()
            .filter(|g| Some(g.id) != game)
            .filter_map(|g| g.root.as_deref().map(Path::to_path_buf))
            .collect::<Vec<_>>();
        let launch = LaunchConfig::default();
        let isolation = crate::runtime::resolver::select_isolation(IsolationInputs {
            policy: policy.hardening.runtime_isolation,
            always_isolate: hub.prefs.load().protection.always_isolate_runtime,
            reflink_ok: host.fs.reflink_ok,
        });
        let ctx = PolicyContext {
            game: game.unwrap_or(GameId::from_uuid(uuid::Uuid::from_u128(DRAFT_GAME))),
            root: root.as_deref(),
            exe: &exe,
            home: &self.env.home,
            uid: self.env.uid,
            gid: self.env.gid,
            user_name: &self.env.user_name,
            group_name: &self.env.group_name,
            paths: &self.paths,
            launch: &launch,
            global_env: &self.env.global_env,
            host_env: &self.env.host_env,
            user_dirs: &self.env.user_dirs,
            other_game_roots: &others,
            mounts: &self.env.mounts,
            fs: &*self.fs,
            developer_mode: false,
            seccomp_available: self.env.seccomp_available,
            isolation,
        };
        explain_policy(policy, &ctx, host)
    }
}
