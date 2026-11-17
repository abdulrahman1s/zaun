//! A Steam job finished: hand its result to the action that started it (ADR-0031).

use super::state::SteamDone;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn on_steam_done(&mut self, done: SteamDone) -> Result<()> {
        match done {
            SteamDone::Scanned { scan, announce } => self.on_steam_scanned(*scan, announce),
            SteamDone::Running(r) => {
                self.on_steam_running(&r);
                Ok(())
            }
            SteamDone::Asked {
                game,
                request,
                result,
            } => {
                self.on_steam_asked(game, request, result);
                Ok(())
            }
            SteamDone::LaunchOptions {
                game,
                reply_to,
                result,
            } => self.on_launch_options_written(game, reply_to, result),
        }
    }
}
