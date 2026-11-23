//! A peer's `AgentRatchetKey` (per-queue encrypted only) and `EREADY` (Agent.hs 7.1.4 `newRatchetKey`, `ereadyMsg`).
use crate::encoding::{Decode, Encode, Version};
use crate::ratchet::{
    self, PqSupport, PrivParams, Ratchet, RatchetVersions, RcvParams, Resynced, ratchet_key_hash,
    supported_e2e_range,
};
use crate::store::Store;

use super::keys::{Keys, choose_keys};
use super::send::answered_key_hash;
use super::state::SyncState;
use crate::agent::E2eParams;
use crate::agent::client::{AgentClient, AgentEvent, ConnId};
use crate::agent::delivery::Handled;
use crate::agent::error::{AgentError, Result};
use crate::agent::message::AMessage;
use crate::agent::records::{Batch, ConnRecord};

/// What `EREADY` needs from before its own decryption (Agent.hs passes `rcPrev`).
pub(crate) struct BeforeDecrypt {
    had_sending_chain: bool,
    last_ext_snd_id: i64,
}

impl BeforeDecrypt {
    pub(crate) fn of(rec: &ConnRecord, ratchet: &Ratchet) -> Self {
        Self {
            had_sending_chain: ratchet.has_sending_chain(),
            last_ext_snd_id: rec.chain.rcv_id,
        }
    }
}

impl AgentClient {
    /// The agent version is not raised here as `updateConnVersion` would; nothing reads it yet.
    pub(crate) fn on_ratchet_key(
        &mut self,
        store: &mut Store,
        id: ConnId,
        _agent_version: Version,
        e2e: E2eParams,
        info: Vec<u8>,
    ) -> Result<Handled> {
        let (mut rec, _) = self.editable(id)?;
        if rec.rcv.is_none() || rec.snd.is_none() {
            return Err(AgentError::Prohibited(
                "AgentRatchetKey: message must be sent to duplex connection",
            ));
        }
        let peer = RcvParams::try_from(ratchet::E2eParams::from_bytes(&e2e.to_bytes()?)?)?;
        let p = peer.params();
        let hash = ratchet_key_hash(&p.k1, &p.k2).to_vec();
        if rec.sync.as_ref().is_some_and(|s| s.knows_key(&hash)) {
            return Ok(Handled::Ack);
        }
        if !supported_e2e_range().contains(p.version) {
            return Err(AgentError::Version);
        }
        match choose_keys(&rec, p.version, answered_key_hash(&info).as_deref())? {
            Keys::Use { own, reply } => self.agree(store, rec, &peer, &own, reply, hash)?,
            Keys::Skip => {
                rec.sync_mut().remember_key(hash);
                self.save(store, rec, None, Batch::default())?;
            }
            Keys::Refuse => return self.refuse_key(store, rec, hash),
        }
        Ok(Handled::Ack)
    }

    /// `initRatchet` and `recreateRatchet`: our reply key first, then the new ratchet and, on the
    /// sending side, `EREADY` made with it, all in one stored change.
    fn agree(
        &mut self,
        store: &mut Store,
        mut rec: ConnRecord,
        peer: &RcvParams,
        own: &PrivParams,
        reply: Option<RcvParams>,
        hash: Vec<u8>,
    ) -> Result<()> {
        let versions = RatchetVersions::new(peer.params().version);
        let Resynced {
            mut ratchet,
            send_eready,
        } = Ratchet::resync(versions, own, peer, PqSupport(rec.pq))?;
        let mut b = Batch::default();
        let mut ids = Vec::new();
        if let Some(params) = &reply {
            ids.push(self.stage_ratchet_key(&mut rec, &mut b, params, Some(hash.clone()))?);
        }
        let sync = rec.sync_mut();
        sync.remember_key(hash);
        sync.state = SyncState::Agreed;
        sync.keys = None;
        if send_eready {
            let eready = AMessage::Eready(rec.chain.rcv_id);
            ids.push(self.stage_agent_msg(&mut rec, &mut ratchet, &mut b, eready)?);
        }
        let conn = rec.id;
        self.save(store, rec, Some(ratchet), b)?;
        self.queue_staged(conn, &ids)?;
        self.emit(AgentEvent::RatchetSync {
            conn,
            state: SyncState::Agreed,
        });
        Ok(())
    }

    fn refuse_key(
        &mut self,
        store: &mut Store,
        mut rec: ConnRecord,
        hash: Vec<u8>,
    ) -> Result<Handled> {
        let sync = rec.sync_mut();
        sync.remember_key(hash);
        sync.state = SyncState::Required;
        let conn = rec.id;
        self.save(store, rec, None, Batch::default())?;
        self.emit(AgentEvent::RatchetSync {
            conn,
            state: SyncState::Required,
        });
        Err(AgentError::Prohibited(
            "RATCHET_SYNC: a ratchet key answered ours after the ratchets agreed",
        ))
    }

    /// `ereadyMsg`: a ratchet made receiving sent no `EREADY` when the keys were agreed, so it
    /// answers now. The caller stores `rec`, `ratchet` and `b` together.
    pub(crate) fn on_eready(
        &mut self,
        rec: &mut ConnRecord,
        ratchet: &mut Ratchet,
        b: &mut Batch,
        before: &BeforeDecrypt,
    ) -> Result<Handled> {
        if !before.had_sending_chain {
            let eready = AMessage::Eready(before.last_ext_snd_id);
            let id = self.stage_agent_msg(rec, ratchet, b, eready)?;
            self.queue_staged(rec.id, &[id])?;
        }
        Ok(Handled::Ack)
    }
}
