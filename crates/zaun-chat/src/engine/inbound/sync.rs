//! A connection's ratchet-sync state changed (`RSYNC`); a friend's new ratchet has a new security code (Subscriber.hs `RSYNC`, `ratchetSyncEventItem`, `securityCodeChanged`).

use crate::agent::resync::SyncState;
use crate::engine::agent_port::{AgentPort, ConnId};
use crate::engine::error::Result;
use crate::engine::machine::Engine;

impl<A: AgentPort> Engine<A> {
    /// Chat's decryption-error and "encryption re-negotiated" items have no notice in Zaun, and
    /// members are never verified here, so only a verified friend's `Agreed` shows.
    pub(crate) fn on_ratchet_sync(&mut self, conn: &ConnId, state: SyncState) -> Result<()> {
        self.log.line(format_args!(
            "connection {}: ratchet sync {state:?}",
            conn.0
        ));
        match state {
            SyncState::Agreed => {}
            SyncState::Required => {
                self.repair(conn);
                return Ok(());
            }
            SyncState::Ok | SyncState::Allowed | SyncState::Started => return Ok(()),
        }
        let Some(friend) = self.state.friend_of(conn) else {
            return Ok(());
        };
        self.security_code_changed(friend)
    }

    /// The apps offer "fix connection" here and refuse to send until it is pressed; Zaun has no
    /// such button, so it starts the re-sync itself. The agent reports `Required` once per
    /// failure, and `Started` waits for the peer, so this cannot loop.
    fn repair(&mut self, conn: &ConnId) {
        if let Err(e) = self.agent.synchronize_ratchet(&mut self.store, conn) {
            self.log.line(format_args!(
                "connection {}: could not start a re-sync: {}",
                conn.0,
                e.detail()
            ));
        }
    }
}
