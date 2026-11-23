//! Starting a re-sync ourselves: the engine does when a connection needs one, where the apps wait for the user (Agent.hs `synchronizeRatchet'`, `enqueueRatchetKeyMsgs`).
use crate::ratchet::{CURRENT_E2E_VERSION, PqSupport, new_resync_params};
use crate::store::Store;

use super::state::SyncState;
use crate::agent::client::{AgentClient, ConnId};
use crate::agent::error::{AgentError, Result};
use crate::agent::records::Batch;

impl AgentClient {
    /// Allowed in `Allowed` and `Required` (`ratchetSyncAllowed`), or always with `force`.
    pub(crate) fn synchronize_ratchet(
        &mut self,
        store: &mut Store,
        conn: ConnId,
        force: bool,
    ) -> Result<()> {
        let (mut rec, _) = self.editable(conn)?;
        if rec.rcv.is_none() || rec.snd.is_none() {
            return Err(AgentError::Prohibited("synchronizeRatchet: not duplex"));
        }
        let allowed = matches!(rec.sync_state(), SyncState::Allowed | SyncState::Required);
        if !(allowed || force) {
            return Err(AgentError::Prohibited("synchronizeRatchet: not allowed"));
        }
        let (own, params) = new_resync_params(CURRENT_E2E_VERSION, PqSupport(rec.pq))?;
        let mut b = Batch::default();
        let id = self.stage_ratchet_key(&mut rec, &mut b, &params, None)?;
        let sync = rec.sync_mut();
        sync.state = SyncState::Started;
        sync.keys = Some(own);
        self.save(store, rec, None, b)?;
        self.queue_staged(conn, &[id])?;
        self.kick(store, conn);
        Ok(())
    }
}
