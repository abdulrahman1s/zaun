//! The Steam page's record of a game, answered through its query (ADR-0031 §6).
use crate::library::steam::{is_listed, not_listed};
use crate::model::{GameOrigin, GameSummary, SteamCompat};
use crate::service::event::SteamDetail;
use crate::service::prelude::*;
use crate::steam::client::{SteamRequest, client_running};

/// The Steam app ID behind `id`, or `ZN-STM-002`.
pub(super) fn app_id(id: GameId) -> Result<u32> {
    id.steam_app_id().ok_or_else(|| err!(StmGameGone))
}

impl AppService {
    /// The listed summary of a Steam game, or `ZN-STM-002`.
    pub(super) fn steam_summary(&self, id: GameId) -> Result<GameSummary> {
        let snap = self.lib.snapshot();
        if !is_listed(&snap, id) {
            return Err(not_listed(id.steam_app_id().unwrap_or(0)));
        }
        snap.get(id).cloned().ok_or_else(|| err!(StmGameGone))
    }

    pub(super) fn steam_details(&mut self, id: GameId, reply_to: QueryId) -> Result<()> {
        let result = match self.steam_detail(id) {
            Ok(d) => QueryResult::Steam(Box::new(d)),
            Err(e) => QueryResult::Failed(Box::new(self.describe(&e, Some(id)))),
        };
        self.emit(Event::QueryResult {
            id: reply_to,
            result,
        });
        Ok(())
    }

    fn steam_detail(&mut self, id: GameId) -> Result<SteamDetail> {
        let s = self.steam_summary(id)?;
        let app = id.steam_app_id().unwrap_or(0);
        let GameOrigin::Steam(facts) = s.origin.clone() else {
            return Err(not_listed(app));
        };
        let notes = self.lib.steam_row(app)?.notes;
        let running = self.flow.steam.is_running(app);
        let client_found = self
            .steam_command_for(Some(id), SteamRequest::Run(app))
            .is_ok();
        let prefix_tools = !running
            && matches!(
                &facts.compat,
                SteamCompat::Proton {
                    prefix: Some(_),
                    proton_dir: Some(_),
                    ..
                }
            );
        let launch_options = self
            .lib
            .steam_scan()
            .and_then(|s| s.game(app).map(|g| g.root.clone()))
            .and_then(|root| crate::steam::launch_options::read(&*self.deps.fs, &root, app));
        let steam_open = client_running(&*self.deps.proc);
        Ok(SteamDetail {
            game: id,
            facts,
            notes,
            running,
            client_found,
            prefix_tools,
            launch_options,
            steam_open,
        })
    }
}
