//! "Close Steam": ask the running client to close itself (ADR-0031 §6a).
use crate::service::prelude::*;
use crate::steam::client::{SteamRequest, client_running};

impl AppService {
    pub(super) fn steam_close(&mut self, id: GameId) -> Result<()> {
        // `-shutdown` with no client running could start one: only while one runs.
        if !client_running(&*self.deps.proc) {
            tracing::debug!("close steam: no client runs");
            return Ok(());
        }
        self.steam_ask(Some(id), SteamRequest::Shutdown)
    }
}
