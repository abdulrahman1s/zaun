//! Delivery receipts and queue-full notices, both directions (Agent.hs `messagesRcvd`, `ackMessage'`, `A_QCONT`; fact sheet §13).
use crate::store::Store;

use super::receive::Handled;
use crate::agent::client::{AgentClient, AgentEvent, AgentMsgId, ConnId, RcptStatus, Receipt};
use crate::agent::error::{AgentError, Result};
use crate::agent::message::{AMessage, AMessageReceipt, SndQAddr};
use crate::agent::records::{self, Batch, ConnStatus, SendKind, SentMsg, Unacked};

impl AgentClient {
    /// The engine has processed message `msg_id`; with `receipt`, the sender learns it arrived (`A_RCVD`).
    pub fn ack(
        &mut self,
        store: &mut Store,
        conn: ConnId,
        msg_id: AgentMsgId,
        receipt: Option<Vec<u8>>,
    ) -> Result<()> {
        let key = records::unacked_key(conn);
        let u = records::load::<Unacked>(store, &key)?
            .filter(|u| u.msg_id == msg_id)
            .ok_or(AgentError::MsgNotFound)?;
        let mut b = Batch::default();
        b.delete(key);
        b.apply(store)?;
        self.ack_queue(conn, &u.smp_msg_id);
        let Some(rcpt_info) = receipt else {
            return Ok(());
        };
        let r = AMessageReceipt {
            agent_msg_id: u.ext_snd_id,
            msg_hash: u.hash,
            rcpt_info,
        };
        self.enqueue_message(store, conn, AMessage::ARcvd(vec![r]), SendKind::Receipt)?;
        Ok(())
    }

    /// Each receipt names our `sndMsgId`; `Rcvd` once per message, as the record is removed.
    pub(crate) fn on_receipts(
        &mut self,
        store: &mut Store,
        id: ConnId,
        receipts: Vec<AMessageReceipt>,
        b: &mut Batch,
    ) -> Result<Handled> {
        for r in receipts {
            let key = records::sent_key(id, r.agent_msg_id);
            let Some(sent) = records::load::<SentMsg>(store, &key)? else {
                continue;
            };
            b.delete(key);
            let status = if sent.hash == r.msg_hash {
                RcptStatus::Ok
            } else {
                RcptStatus::BadMsgHash
            };
            let receipt = Receipt {
                status,
                info: r.rcpt_info,
            };
            self.emit(AgentEvent::Rcvd {
                conn: id,
                msg_id: sent.msg_id,
                receipt,
            });
        }
        Ok(Handled::Ack)
    }

    /// The server says our queue drained after it was full: tell the sender to continue.
    pub(crate) fn on_quota_notice(&mut self, store: &mut Store, id: ConnId) -> Result<()> {
        let c = self.conn(id)?;
        let Some(rq) = c
            .rec
            .rcv
            .as_ref()
            .filter(|_| c.rec.status == ConnStatus::Active)
        else {
            return Ok(());
        };
        let addr = SndQAddr {
            server: rq.server.clone(),
            sender_id: rq.snd_id.clone(),
        };
        self.enqueue_message(store, id, AMessage::AQcont(addr), SendKind::Qcont)?;
        Ok(())
    }

    /// The peer has room again: the slow retry after QUOTA ends now.
    pub(crate) fn on_qcont(&mut self, id: ConnId) -> Handled {
        if let Some(c) = self.conns.get_mut(&id) {
            c.send.retry = None;
            c.send.next_at = 0;
        }
        self.emit(AgentEvent::Qcont { conn: id });
        Handled::Ack
    }
}
