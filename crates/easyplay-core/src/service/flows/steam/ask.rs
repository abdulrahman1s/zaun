//! Requests handed to the Steam client: Play, Update, Open in Steam (ADR-0031).

use super::state::{STEAM_START_WINDOW, SteamDone};
use crate::host::CommandSpec;
use crate::library::steam::not_listed;
use crate::model::{Activity, LaunchStateTag};
use crate::service::prelude::*;
use crate::steam::client::{SteamRequest, command};

impl AppService {
    /// The command that hands `request` to the Steam installation listing `id` (any
    /// installation for the downloads page).
    pub(super) fn steam_command_for(
        &self,
        id: Option<GameId>,
        request: SteamRequest,
    ) -> Result<CommandSpec> {
        let scan = self.lib.steam_scan().ok_or_else(|| err!(StmNotFound))?;
        let (kind, root) = match id.and_then(|i| i.steam_app_id()) {
            Some(app) => {
                let g = scan.game(app).ok_or_else(|| not_listed(app))?;
                (g.facts.install, g.root.clone())
            }
            None => {
                let r = scan.roots.first().ok_or_else(|| err!(StmNotFound))?;
                (r.kind, r.path.clone())
            }
        };
        let env = &self.deps.launch_env.host_env;
        command(&*self.deps.fs, kind, &root, env, request)
            .ok_or_else(|| err!(StmClientUnavailable, install = kind.as_str()))
    }

    /// Hand `request` to Steam on the job pool.
    pub(crate) fn steam_ask(&mut self, game: Option<GameId>, request: SteamRequest) -> Result<()> {
        if let Some(id) = game {
            self.steam_summary(id)?;
        }
        let spec = self.steam_command_for(game, request)?;
        let Some(handoff) = self.flow.steam.handoff.clone() else {
            bail!(StmClientUnavailable, detail = "no hand-off");
        };
        let post = self.poster();
        let _ = self
            .deps
            .jobs
            .submit(JobPriority::Interactive, JobLabel("steam-ask"), move |_| {
                let result = handoff.start_detached(&spec);
                let asked = SteamDone::Asked {
                    game,
                    request,
                    result,
                };
                post(Internal::Flow(Box::new(FlowDone::Steam(asked))));
            });
        Ok(())
    }

    pub(super) fn on_steam_asked(
        &mut self,
        game: Option<GameId>,
        request: SteamRequest,
        result: Result<()>,
    ) {
        if let Err(e) = result {
            let u = self.describe(&e, game);
            self.emit(Event::Failure(Box::new(u)));
            return;
        }
        if let SteamRequest::Run(app) = request {
            let until = self.deps.clock.mono().saturating_add(STEAM_START_WINDOW);
            self.flow.steam.pending.insert(app, until);
            if !self.flow.steam.is_running(app) {
                let starting = Activity::Launching(LaunchStateTag::Starting);
                self.lib
                    .steam_set_running(&[(GameId::steam(app), starting)]);
                self.publish_library();
            }
            self.steam_watch();
        }
        self.notice(Notice::SteamAsked { game, request });
    }
}
