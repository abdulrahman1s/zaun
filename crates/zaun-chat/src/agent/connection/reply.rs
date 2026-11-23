//! The joiner after SKEY: create the reply queue, then queue the confirmation that names it (Agent.hs `secureConfirmQueue`, `mkAgentConfirmation`; HANDSHAKE.md §2.4–§2.9).
use crate::encoding::{Decode, Encode};
use crate::ratchet::Ratchet;
use crate::smp::QueueIds;
use crate::store::Store;

use super::queues::{new_rcv_queue, queue_address};
use crate::agent::client::{AgentClient, ConnId};
use crate::agent::e2e::E2eParams as WireParams;
use crate::agent::envelope::AgentMsgEnvelope;
use crate::agent::error::{AgentError, Result};
use crate::agent::message::AgentMessage;
use crate::agent::records::{Batch, ConnStatus, SendKind};
use crate::agent::{SmpQueueInfo, msg_hash};

impl AgentClient {
    /// SKEY on the inviter's queue succeeded.
    pub(crate) fn on_inviter_secured(&mut self, store: &mut Store, id: ConnId) -> Result<()> {
        let (mut rec, _) = self.editable(id)?;
        if let Some(sq) = rec.snd.as_mut() {
            sq.secured = true;
        }
        rec.rcv = Some(new_rcv_queue(self.reply_server()?)?);
        rec.status = ConnStatus::Replying;
        self.save(store, rec, None, Batch::default())?;
        self.drive(store, id);
        Ok(())
    }

    /// The reply queue exists: confirm with `AgentConnInfoReply`, ratchet-encrypted as message 1 (HANDSHAKE.md §2.6–§2.9).
    pub(crate) fn on_reply_created(
        &mut self,
        store: &mut Store,
        id: ConnId,
        ids: QueueIds,
    ) -> Result<()> {
        let (mut rec, ratchet) = self.editable(id)?;
        let mut ratchet = ratchet.ok_or(AgentError::Prohibited("no ratchet"))?;
        let rq = rec
            .rcv
            .as_mut()
            .ok_or(AgentError::Prohibited("no reply queue"))?;
        let sq = rec
            .snd
            .clone()
            .ok_or(AgentError::Prohibited("no send queue"))?;
        rq.rcv_id = ids.rcv_id;
        rq.snd_id = ids.snd_id.clone();
        rq.server_dh = Some(ids.rcv_dh_key);
        let reply = SmpQueueInfo {
            client_version: sq.client_version,
            address: queue_address(rq, ids.snd_id)?,
        };
        let conn_info = rec.conn_info.take().unwrap_or_default();
        let plain = AgentMessage::ConnInfoReply {
            queues: vec![reply],
            conn_info,
        }
        .to_bytes()?;
        let e2e = rec
            .e2e_reply
            .take()
            .ok_or(AgentError::Prohibited("no ratchet params"))?;
        let env = AgentMsgEnvelope::Confirmation {
            agent_version: rec.agent_version,
            e2e_encryption: Some(WireParams::from_bytes(&e2e)?),
            enc_conn_info: encrypt_conn_info(&mut ratchet, &plain)?,
        };
        rec.chain.snd_id = 1;
        rec.chain.snd_hash = msg_hash(&plain).to_vec();
        rec.status = ConnStatus::Joined;
        let body = crate::agent::delivery::seal(&sq, &env.to_bytes()?, true)?;
        self.enqueue_built(
            store,
            rec,
            ratchet,
            SendKind::Confirmation,
            body,
            Batch::default(),
        )?;
        self.conn_mut(id)?.sub.subscribed = true; // NEW subscribed it on this session
        self.drive(store, id);
        Ok(())
    }
}

/// The ratchet-encrypted connection info of a confirmation, padded for the connection's PQ support.
pub(crate) fn encrypt_conn_info(ratchet: &mut Ratchet, plain: &[u8]) -> Result<Vec<u8>> {
    let pq = ratchet.pq_support();
    let len = crate::ratchet::e2e_enc_conn_info_length(pq);
    Ok(ratchet.encrypt(plain, len, Some(pq.to_enc()))?)
}
