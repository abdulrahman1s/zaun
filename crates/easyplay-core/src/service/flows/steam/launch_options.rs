//! A Steam game's launch options: written to Steam's own file on the job pool (ADR-0031 §6a).

use crate::service::prelude::*;
use std::path::PathBuf;

use super::details::app_id;
use super::state::SteamDone;
use crate::library::steam::not_listed;
use crate::steam::launch_options::{self, Written};

impl AppService {
    /// Write the launch options of the Steam game `id`; the page is answered either way.
    pub(super) fn steam_set_launch_options(
        &mut self,
        id: GameId,
        options: String,
        reply_to: QueryId,
    ) -> Result<()> {
        let root = match self.steam_game_root(id) {
            Ok(r) => r,
            Err(e) => {
                self.fail(e);
                return self.steam_details(id, reply_to);
            }
        };
        let app = app_id(id)?;
        let fs = Arc::clone(&self.deps.fs);
        let proc = Arc::clone(&self.deps.proc);
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("steam-launch-options"),
            move |_| {
                let result = launch_options::write(&*fs, &*proc, &root, app, &options);
                let written = SteamDone::LaunchOptions {
                    game: id,
                    reply_to,
                    result,
                };
                post(Internal::Flow(Box::new(FlowDone::Steam(written))));
            },
        );
        Ok(())
    }

    /// The Steam installation folder that lists the game.
    fn steam_game_root(&self, id: GameId) -> Result<PathBuf> {
        self.steam_summary(id)?;
        let app = id.steam_app_id().unwrap_or(0);
        self.lib
            .steam_scan()
            .and_then(|s| s.game(app).map(|g| g.root.clone()))
            .ok_or_else(|| not_listed(app))
    }

    pub(super) fn on_launch_options_written(
        &mut self,
        game: GameId,
        reply_to: QueryId,
        result: Result<Written>,
    ) -> Result<()> {
        match result {
            Ok(Written::Changed { .. }) => {
                tracing::info!(app = game.steam_app_id(), "steam launch options written");
                self.notice(Notice::SteamLaunchOptionsSaved { game });
            }
            Ok(Written::Unchanged) => {}
            Err(e) => {
                let u = self.describe(&e, Some(game));
                self.emit(Event::Failure(Box::new(u)));
            }
        }
        // The page shows the value as Steam's file now has it.
        self.steam_details(game, reply_to)
    }
}
