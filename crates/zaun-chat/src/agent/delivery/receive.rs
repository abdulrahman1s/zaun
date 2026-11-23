//! A message on one of our queues: the server layer, the per-queue box, then the envelope by kind; handshake and agent messages are acked here (Agent.hs `processSMPTransmissions`, `smpConfirmation`).
use crate::encoding::Decode;
use crate::smp::{RcvMsg, decrypt_message};
use crate::store::Store;

use super::seal::open;
use crate::agent::client::{AgentClient, AgentEvent, ConnId, OpKind};
use crate::agent::connection::{Confirmation, ack_request};
use crate::agent::envelope::AgentMsgEnvelope;
use crate::agent::error::{AgentError, Result};

/// Whether the agent acks a message itself, or the engine will through `ack`.
pub(crate) enum Handled {
    Ack,
    Hold,
}

impl AgentClient {
    pub(crate) fn on_queue_msg(
        &mut self,
        store: &mut Store,
        id: ConnId,
        smp_msg_id: &[u8],
        body: &[u8],
    ) {
        match self.receive(store, id, smp_msg_id, body) {
            Ok(Handled::Hold) => {}
            Ok(Handled::Ack) => self.ack_queue(id, smp_msg_id),
            // Like the Haskell agent: report it, and ack so that it is not delivered forever.
            Err(err) => {
                self.emit(AgentEvent::Err { conn: id, err });
                self.ack_queue(id, smp_msg_id);
            }
        }
        self.kick(store, id);
    }

    fn receive(
        &mut self,
        store: &mut Store,
        id: ConnId,
        smp_msg_id: &[u8],
        body: &[u8],
    ) -> Result<Handled> {
        let rq = self
            .conn(id)?
            .rec
            .rcv
            .clone()
            .ok_or(AgentError::Prohibited("no queue"))?;
        let (ts, body) = match decrypt_message(&rq.server_secret()?, smp_msg_id, body)? {
            RcvMsg::Quota { .. } => {
                self.on_quota_notice(store, id)?;
                return Ok(Handled::Ack);
            }
            RcvMsg::Message { ts, body, .. } => (ts, body),
        };
        if let Some(h) = self.redelivered(store, id, smp_msg_id)? {
            return Ok(h);
        }
        let opened = open(&rq, &body)?;
        match AgentMsgEnvelope::from_bytes(&opened.envelope)? {
            AgentMsgEnvelope::Confirmation {
                agent_version,
                e2e_encryption,
                enc_conn_info,
            } if opened.sender_e2e.is_some() => {
                let conf = Confirmation {
                    agent_version,
                    e2e: e2e_encryption,
                    enc_conn_info,
                };
                self.on_confirmation(store, id, &opened, conf)?;
                Ok(Handled::Ack)
            }
            AgentMsgEnvelope::Message {
                enc_agent_message, ..
            } if opened.sender_e2e.is_none() => {
                self.on_agent_message(store, id, smp_msg_id, ts.0, &enc_agent_message)
            }
            // TODO(phase 3): requests to a contact address (`REQ`).
            AgentMsgEnvelope::Invitation { .. } | AgentMsgEnvelope::ContactRequest { .. } => {
                Err(AgentError::Unsupported("contact requests"))
            }
            // TODO(phase 5): ratchet re-synchronisation (`EREADY`).
            AgentMsgEnvelope::RatchetKey { .. } => Err(AgentError::Unsupported("ratchet re-sync")),
            AgentMsgEnvelope::Confirmation { .. } | AgentMsgEnvelope::Message { .. } => {
                Err(AgentError::Prohibited("envelope does not match its header"))
            }
        }
    }

    pub(crate) fn ack_queue(&mut self, id: ConnId, smp_msg_id: &[u8]) {
        let Some(rq) = self.conns.get(&id).and_then(|c| c.rec.rcv.clone()) else {
            return;
        };
        // An ACK that is lost makes the server send the message again; it is then recognised and acked.
        self.request(id, OpKind::Ack, &rq.server, ack_request(&rq, smp_msg_id));
    }
}
