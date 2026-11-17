//! Run a tool in a Steam game's Windows storage as a `Tool` session (ADR-0031 §6).

use std::path::{Path, PathBuf};

use super::prefix_job::PrefixJob;
use crate::library::steam::not_for_steam;
use crate::model::{SteamCompat, SteamFacts, Tool};
use crate::service::prelude::*;

impl AppService {
    pub(super) fn steam_run_in_prefix(&mut self, id: GameId, tool: Tool) -> Result<()> {
        tool.validate()?;
        // Needs the network inside an Offline sandbox; not offered for Steam games yet.
        if matches!(tool, Tool::Winetricks { .. }) {
            return Err(not_for_steam("winetricks"));
        }
        let s = self.steam_summary(id)?;
        let app = super::details::app_id(id)?;
        ensure!(!self.flow.steam.is_running(app), StmGameRunning);
        let Some(facts) = s.steam() else {
            return Ok(());
        };
        let (compat_data, proton) = prefix_layer(facts)?;
        let (program, root) = self.tool_program(&tool, facts, &compat_data);
        let job = PrefixJob {
            id,
            app,
            name: s.name.to_string(),
            tool,
            program,
            root,
            proton,
            compat_data,
            install_id: self.install_id.clone(),
            env: self.deps.launch_env.clone(),
            fs: Arc::clone(&self.deps.fs),
            clock: Arc::clone(&self.deps.clock),
            paths: self.deps.paths.clone(),
            runtimes: Arc::clone(&self.deps.runtimes),
            launcher: Arc::clone(&self.deps.launcher),
            locks: Arc::clone(&self.deps.locks),
            host: Arc::clone(&self.deps.host),
            proc: Arc::clone(&self.deps.proc),
        };
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("steam-prefix-tool"),
            move |_| {
                let result = job.start();
                post(Internal::Flow(Box::new(FlowDone::Tool {
                    game: id,
                    result,
                })));
            },
        );
        Ok(())
    }

    /// The program to run and the folder that is the game folder for it.
    fn tool_program(
        &self,
        tool: &Tool,
        facts: &SteamFacts,
        compat_data: &Path,
    ) -> (PathBuf, Option<PathBuf>) {
        match tool {
            // A program elsewhere (a mod installer in Downloads): its folder, read-only. A
            // program inside the prefix is already reachable through it, like winecfg below.
            Tool::RunExe { path, .. } if !path.starts_with(compat_data) => {
                let root = path
                    .parent()
                    .filter(|d| !self.deps.shared.is_shared_location(d))
                    .map(Path::to_path_buf);
                (path.clone(), root)
            }
            // Windows' own programs come from the prefix: the game's install folder is the
            // (read-only) game folder, so nothing is bound inside the prefix twice.
            _ => (
                facts.install_dir.join("winecfg.exe"),
                Some(facts.install_dir.clone()),
            ),
        }
    }
}

/// The game's `compatdata` folder and the Proton build that last ran it.
fn prefix_layer(facts: &SteamFacts) -> Result<(PathBuf, PathBuf)> {
    match &facts.compat {
        SteamCompat::Proton {
            compat_data,
            prefix: Some(_),
            proton_dir,
            ..
        } => match proton_dir {
            Some(d) => Ok((compat_data.clone(), d.clone())),
            None => Err(err!(StmLayerUnavailable)),
        },
        SteamCompat::Proton { .. } | SteamCompat::NotProton => Err(err!(StmNoPrefix)),
    }
}
