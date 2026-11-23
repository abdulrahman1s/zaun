//! The rotation messages a sender receives; a refusal is reported and acked, keeping the ratchet step that opened it (Agent.hs `qAddMsg`, `qKeyMsg`, `qUseMsg`, `qEndMsg`).
use crate::store::Store;

use super::addr::same_addr;
use super::state::{Step, Switch};
use crate::agent::client::{AgentClient, AgentEvent, ConnId};
use crate::agent::delivery::Handled;
use crate::agent::error::{AgentError, Result};
use crate::agent::message::SndQAddr;
use crate::agent::queue_info::SmpQueueInfo;
use crate::agent::queue_uri::SmpQueueUri;
use crate::agent::records::{Batch, ConnRecord};
use crate::agent::resync::SyncState;
use crate::crypto::PublicKey;

impl AgentClient {
    pub(crate) fn on_qadd(
        &mut self,
        _store: &mut Store,
        rec: &mut ConnRecord,
        _b: &mut Batch,
        queues: Vec<(SmpQueueUri, Option<SndQAddr>)>,
    ) -> Result<Handled> {
        let r = self.qadd(rec, queues);
        Ok(self.reported(rec.id, r))
    }

    /// We never move our own receive queue, so no queue of ours waits for the peer's key.
    pub(crate) fn on_qkey(
        &mut self,
        _store: &mut Store,
        rec: &mut ConnRecord,
        _b: &mut Batch,
        _queues: Vec<(SmpQueueInfo, PublicKey)>,
    ) -> Result<Handled> {
        let r = refuse_unsynced(rec).and(Err(AgentError::Prohibited(
            "QKEY: queue address not found in connection",
        )));
        Ok(self.reported(rec.id, r))
    }

    pub(crate) fn on_quse(
        &mut self,
        _store: &mut Store,
        rec: &mut ConnRecord,
        _b: &mut Batch,
        queues: Vec<(SndQAddr, bool)>,
    ) -> Result<Handled> {
        let r = quse(rec, queues);
        Ok(self.reported(rec.id, r))
    }

    /// The peer's test message on a queue it moved to us needs nothing (`QTEST _ -> ackDel`).
    pub(crate) fn on_qtest(
        &mut self,
        _store: &mut Store,
        _rec: &mut ConnRecord,
        _b: &mut Batch,
        _queues: Vec<SndQAddr>,
    ) -> Result<Handled> {
        Ok(Handled::Ack)
    }

    /// `qEndMsg` acts only when a second receive queue replaces the named one; we never make one.
    pub(crate) fn on_qend(
        &mut self,
        _store: &mut Store,
        _rec: &mut ConnRecord,
        _b: &mut Batch,
        _queues: Vec<SndQAddr>,
    ) -> Result<Handled> {
        Ok(Handled::Ack)
    }

    fn reported(&mut self, conn: ConnId, r: Result<()>) -> Handled {
        if let Err(err) = r {
            self.emit(AgentEvent::Err { conn, err });
        }
        Handled::Ack
    }
}

/// `ratchetSyncSendProhibited`: no rotation while a re-sync is required, started or agreed.
pub(super) fn refuse_unsynced(rec: &ConnRecord) -> Result<()> {
    let prohibited = rec.sync.as_ref().is_some_and(|s| match s.state {
        SyncState::Required | SyncState::Started | SyncState::Agreed => true,
        SyncState::Ok | SyncState::Allowed => false,
    });
    if prohibited {
        return Err(AgentError::Prohibited("ratchet is not synchronized"));
    }
    Ok(())
}

/// `qUseMsg`: the peer has secured the new queue with KEY, so QTEST may go to it.
fn quse(rec: &mut ConnRecord, queues: Vec<(SndQAddr, bool)>) -> Result<()> {
    refuse_unsynced(rec)?;
    let not_found = AgentError::Prohibited("QUSE: switched queue address not found in connection");
    let (addr, _primary) = queues.into_iter().next().ok_or(not_found.clone())?;
    let Some(Switch::Moving(m)) = rec.switch.as_mut() else {
        return Err(not_found);
    };
    if !same_addr(&m.new, &addr) {
        return Err(not_found);
    }
    match m.step {
        // Both are `SSSendingQKEY` in Haskell, which sets it when QADD arrives.
        Step::Qkey | Step::WaitingQuse => {}
        Step::Securing | Step::Confirming | Step::Qtest | Step::Testing { .. } | Step::Ready => {
            return Err(AgentError::Prohibited("QUSE: unexpected switch status"));
        }
    }
    m.new.secured = true;
    m.step = Step::Qtest;
    Ok(())
}
