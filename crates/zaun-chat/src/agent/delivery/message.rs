//! A ratchet-encrypted agent message: decrypt, check the chain, then by kind; only `A_MSG` waits for the engine's ack (Agent.hs `agentClientMsg`, `A_DUPLICATE`, `checkMsgIntegrity`).
use crate::encoding::Decode;
use crate::ratchet::{Ratchet, RatchetError};
use crate::store::Store;

use super::receive::Handled;
use crate::agent::client::{AgentClient, AgentEvent, AgentMsgId, ConnId};
use crate::agent::error::{AgentError, Result};
use crate::agent::message::{AMessage, AgentMessage};
use crate::agent::records::{self, Batch, ConnRecord, Unacked};
use crate::agent::{check_msg_integrity, msg_hash};

/// `rcvExpireCount` and `rcvExpireInterval`: an unacked message stops being handed out again after both.
const RCV_EXPIRE_COUNT: u16 = 8;
const RCV_EXPIRE_MS: i64 = 24 * 3600 * 1000;

impl AgentClient {
    pub(crate) fn on_agent_message(
        &mut self,
        store: &mut Store,
        id: ConnId,
        smp_msg_id: &[u8],
        broker_ts: i64,
        enc: &[u8],
    ) -> Result<Handled> {
        let (mut rec, ratchet) = self.editable(id)?;
        let mut ratchet = ratchet.ok_or(AgentError::Prohibited("message before the ratchet"))?;
        let Some(plain) = self.decrypt(store, &rec, &mut ratchet, enc)? else {
            return Ok(Handled::Ack);
        };
        let AgentMessage::Message { header, message } = AgentMessage::from_bytes(&plain)? else {
            return Err(AgentError::Prohibited("expected AgentMessage"));
        };
        let c = &rec.chain;
        let integrity = check_msg_integrity(
            c.rcv_id,
            header.snd_msg_id,
            &c.rcv_hash,
            &header.prev_msg_hash,
        );
        let hash = msg_hash(&plain).to_vec();
        rec.chain.rcv_id = header.snd_msg_id;
        rec.chain.rcv_hash.clone_from(&hash);
        let mut b = Batch::default();
        let handled = match message {
            AMessage::AMsg(body) => {
                rec.last_id += 1;
                let u = Unacked {
                    msg_id: AgentMsgId(rec.last_id),
                    smp_msg_id: smp_msg_id.to_vec(),
                    ext_snd_id: header.snd_msg_id,
                    hash,
                    body,
                    integrity,
                    broker_ts,
                    attempts: 0,
                    first_at: self.now(),
                };
                b.unacked(id, &u)?;
                self.save(store, rec, Some(ratchet), b)?;
                self.emit_msg(id, u);
                return Ok(Handled::Hold);
            }
            AMessage::ARcvd(receipts) => self.on_receipts(store, id, receipts, &mut b)?,
            AMessage::AQcont(_) => self.on_qcont(id),
            AMessage::Hello => Handled::Ack,
            // TODO(phase 5): queue rotation (QADD…QEND) and ratchet re-sync (EREADY).
            AMessage::Qadd(_)
            | AMessage::Qkey(_)
            | AMessage::Quse(_)
            | AMessage::Qtest(_)
            | AMessage::Qend(_)
            | AMessage::Eready(_) => Handled::Ack,
        };
        self.save(store, rec, Some(ratchet), b)?;
        Ok(handled)
    }

    /// The ratchet is stored after every attempt. `None` is a message processed before whose ACK was lost (`A_DUPLICATE`).
    fn decrypt(
        &mut self,
        store: &mut Store,
        rec: &ConnRecord,
        ratchet: &mut Ratchet,
        enc: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        let r = ratchet.decrypt(enc);
        if r.is_err() {
            self.save(store, rec.clone(), Some(ratchet.clone()), Batch::default())?;
        }
        match r {
            Ok(p) => Ok(Some(p)),
            Err(RatchetError::DuplicateMessage | RatchetError::EarlierMessage(_)) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// The server sends an unacked message again after a reconnect or restart; hand out the stored copy, since the ratchet has moved past it.
    pub(crate) fn redelivered(
        &mut self,
        store: &mut Store,
        id: ConnId,
        smp_msg_id: &[u8],
    ) -> Result<Option<Handled>> {
        let key = records::unacked_key(id);
        let Some(mut u) = records::load::<Unacked>(store, &key)? else {
            return Ok(None);
        };
        if u.smp_msg_id != smp_msg_id {
            return Ok(None);
        }
        if u.attempts >= RCV_EXPIRE_COUNT && self.now() - u.first_at >= RCV_EXPIRE_MS {
            let mut b = Batch::default();
            b.delete(key);
            b.apply(store)?;
            return Ok(Some(Handled::Ack));
        }
        u.attempts = u.attempts.saturating_add(1);
        let mut b = Batch::default();
        b.unacked(id, &u)?;
        b.apply(store)?;
        self.emit_msg(id, u);
        Ok(Some(Handled::Hold))
    }

    fn emit_msg(&mut self, conn: ConnId, u: Unacked) {
        self.emit(AgentEvent::Msg {
            conn,
            msg_id: u.msg_id,
            body: u.body,
            integrity: u.integrity,
            broker_ts: u.broker_ts,
        });
    }
}
