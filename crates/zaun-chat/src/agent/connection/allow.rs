//! Allow the joiner after `Conf`: secure its reply queue with SKEY, then queue our confirmation; `Con` when the server takes it (Agent.hs `allowConnection'`, `ICAllowSecure`, `connectReplyQueues`; HANDSHAKE.md §4).
use crate::encoding::Encode;
use crate::store::Store;

use super::queues::new_snd_queue;
use super::reply::encrypt_conn_info;
use crate::agent::client::{AgentClient, ConnId};
use crate::agent::envelope::AgentMsgEnvelope;
use crate::agent::error::{AgentError, Result};
use crate::agent::message::AgentMessage;
use crate::agent::msg_hash;
use crate::agent::records::{Batch, ConnStatus, SendKind};
use crate::agent::version::CURRENT_CLIENT;

impl AgentClient {
    /// `conn_info` is chat's `x.info` with our profile.
    pub fn allow(&mut self, store: &mut Store, conn: ConnId, conn_info: Vec<u8>) -> Result<()> {
        let (mut rec, _) = self.editable(conn)?;
        if rec.status != ConnStatus::Confirmed {
            return Err(AgentError::Prohibited("allow without a confirmation"));
        }
        let reply = rec
            .peer_reply
            .take()
            .ok_or(AgentError::Prohibited("confirmation without a reply queue"))?;
        let v = reply.client_version.min(CURRENT_CLIENT);
        rec.snd = Some(new_snd_queue(&reply, v, None)?);
        rec.conn_info = Some(conn_info);
        rec.status = ConnStatus::Allowing;
        self.save(store, rec, None, Batch::default())?;
        self.drive(store, conn);
        Ok(())
    }

    /// The reply queue is ours to send to: confirm with `AgentConnInfo`, message 1 of our chain.
    pub(crate) fn on_reply_secured(&mut self, store: &mut Store, id: ConnId) -> Result<()> {
        let (mut rec, ratchet) = self.editable(id)?;
        let mut ratchet = ratchet.ok_or(AgentError::Prohibited("no ratchet"))?;
        let sq = rec
            .snd
            .as_mut()
            .ok_or(AgentError::Prohibited("no send queue"))?;
        sq.secured = true;
        let sq = sq.clone();
        let plain = AgentMessage::ConnInfo(rec.conn_info.take().unwrap_or_default()).to_bytes()?;
        let env = AgentMsgEnvelope::Confirmation {
            agent_version: rec.agent_version,
            e2e_encryption: None,
            enc_conn_info: encrypt_conn_info(&mut ratchet, &plain)?,
        };
        rec.chain.snd_id = 1;
        rec.chain.snd_hash = msg_hash(&plain).to_vec();
        rec.status = ConnStatus::Accepting;
        let body = crate::agent::delivery::seal(&sq, &env.to_bytes()?, true)?;
        self.enqueue_built(
            store,
            rec,
            ratchet,
            SendKind::Confirmation,
            body,
            Batch::default(),
        )?;
        Ok(())
    }
}
