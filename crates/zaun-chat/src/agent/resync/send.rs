//! Our `AgentRatchetKey` and `EREADY`, staged into the change that makes them so that they are stored with the new ratchet (Agent.hs `storeRatchetKey`, `enqueueRatchetKeyMsgs`, `enqueueMessages'`).
use crate::encoding::{Decode, Encode};
use crate::ratchet::{Ratchet, RcvParams, e2e_enc_agent_msg_length};

use crate::agent::client::{AgentClient, AgentMsgId, ConnId};
use crate::agent::delivery::seal;
use crate::agent::error::{AgentError, Result};
use crate::agent::message::{AMessage, APrivHeader, AgentMessage, RatchetInfo};
use crate::agent::msg_hash;
use crate::agent::records::{Batch, ConnRecord, PendingSend, SendKind};
use crate::agent::version::CURRENT_AGENT;
use crate::agent::{AgentMsgEnvelope, E2eParams};

/// `AgentRatchetInfo`: the initiator's names no key (`R0`), a reply names the key it answers.
pub(crate) fn ratchet_info(answered: Option<Vec<u8>>) -> Result<Vec<u8>> {
    let info = RatchetInfo {
        answered_key_hash: answered,
    };
    Ok(AgentMessage::RatchetInfo(info).to_bytes()?)
}

/// The key an `AgentRatchetKey` answers; a 7.0 peer sends a bare `R`, which answers none.
pub(crate) fn answered_key_hash(info: &[u8]) -> Option<Vec<u8>> {
    if let Ok(AgentMessage::RatchetInfo(i)) = AgentMessage::from_bytes(info) {
        i.answered_key_hash
    } else {
        None
    }
}

/// The envelope at our highest agent version, with params at the version they were made for.
pub(crate) fn ratchet_key_envelope(params: &RcvParams, info: Vec<u8>) -> Result<AgentMsgEnvelope> {
    let e2e = E2eParams::from_bytes(&params.params().to_bytes()?)?;
    Ok(AgentMsgEnvelope::RatchetKey {
        agent_version: CURRENT_AGENT,
        e2e_encryption: e2e,
        info,
    })
}

impl AgentClient {
    /// `storeRatchetKey`: sealed per queue only. It takes a `sndMsgId`, and the next message's
    /// previous hash is that of its info.
    pub(crate) fn stage_ratchet_key(
        &self,
        rec: &mut ConnRecord,
        b: &mut Batch,
        params: &RcvParams,
        answered: Option<Vec<u8>>,
    ) -> Result<AgentMsgId> {
        let sq = rec
            .snd
            .as_ref()
            .ok_or(AgentError::Prohibited("no send queue"))?;
        let info = ratchet_info(answered)?;
        let hash = msg_hash(&info).to_vec();
        let body = seal(sq, &ratchet_key_envelope(params, info)?.to_bytes()?, false)?;
        rec.chain.snd_id += 1;
        rec.chain.snd_hash = hash;
        self.stage(rec, b, body)
    }

    /// An agent message ratchet-encrypted as the next in our chain, without the send check:
    /// `EREADY` goes while the state is still `Agreed`.
    pub(crate) fn stage_agent_msg(
        &self,
        rec: &mut ConnRecord,
        ratchet: &mut Ratchet,
        b: &mut Batch,
        message: AMessage,
    ) -> Result<AgentMsgId> {
        let sq = rec
            .snd
            .as_ref()
            .ok_or(AgentError::Prohibited("no send queue"))?;
        let snd_msg_id = rec.chain.snd_id + 1;
        let header = APrivHeader {
            snd_msg_id,
            prev_msg_hash: rec.chain.snd_hash.clone(),
        };
        let plain = AgentMessage::Message { header, message }.to_bytes()?;
        let len = e2e_enc_agent_msg_length(ratchet.pq_support());
        let env = AgentMsgEnvelope::Message {
            agent_version: rec.agent_version,
            enc_agent_message: ratchet.encrypt(&plain, len, None)?,
        };
        let body = seal(sq, &env.to_bytes()?, false)?;
        rec.chain.snd_id = snd_msg_id;
        rec.chain.snd_hash = msg_hash(&plain).to_vec();
        self.stage(rec, b, body)
    }

    fn stage(&self, rec: &mut ConnRecord, b: &mut Batch, body: Vec<u8>) -> Result<AgentMsgId> {
        rec.last_id += 1;
        let id = AgentMsgId(rec.last_id);
        let p = PendingSend {
            id,
            kind: SendKind::Resync,
            body,
            created_at: self.now(),
        };
        b.pending(rec.id, &p)?;
        Ok(id)
    }

    /// After the staged change is stored: the worker takes the messages in order.
    pub(crate) fn queue_staged(&mut self, conn: ConnId, ids: &[AgentMsgId]) -> Result<()> {
        self.conn_mut(conn)?.sends.extend(ids.iter().copied());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_initiators_info_answers_no_key_and_a_reply_names_the_key() {
        assert_eq!(ratchet_info(None).unwrap(), b"R0");
        let h = vec![7; 32];
        let mut want = b"R1".to_vec();
        want.push(32);
        want.extend_from_slice(&h);
        assert_eq!(ratchet_info(Some(h.clone())).unwrap(), want);
        assert_eq!(answered_key_hash(&want), Some(h));
    }

    #[test]
    fn a_7_0_peers_bare_info_answers_no_key() {
        assert_eq!(answered_key_hash(b"R"), None);
        assert_eq!(answered_key_hash(b"R0"), None);
        assert_eq!(answered_key_hash(b"junk"), None);
    }
}
