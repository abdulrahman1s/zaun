//! The move: the new queue becomes the one we send to and what waits goes to it re-sealed, then at v8 QEND (Agent.hs `ICQSndSecure`, the `AM_QTEST_` outcome).
use crate::store::Store;

use super::addr::addr;
use super::reseal::reseal;
use super::state::{Moving, Switch};
use crate::agent::client::{AgentClient, AgentEvent, AgentMsgId, ConnId};
use crate::agent::delivery::{agent_envelope, seal};
use crate::agent::error::{AgentError, Result};
use crate::agent::message::AMessage;
use crate::agent::records::{self, Batch, PendingSend, SendKind};
use crate::agent::version::fast_rotation;

impl AgentClient {
    /// Runs with no send in flight, so every waiting message is in the store, sealed for the old queue.
    pub(super) fn move_to_new(&mut self, store: &mut Store, id: ConnId, m: Moving) -> Result<()> {
        let (mut rec, ratchet) = self.editable(id)?;
        let old = rec
            .snd
            .clone()
            .ok_or(AgentError::Prohibited("no send queue"))?;
        let ids = records::pending_ids(store, id)?;
        let mut b = Batch::default();
        for &msg in &ids {
            let key = records::send_key(id, msg);
            let Some(mut p) = records::load::<PendingSend>(store, &key)? else {
                continue;
            };
            p.body = reseal(&old, &m.new, &p.body)?;
            b.pending(id, &p)?;
        }
        let moved = ids.iter().any(|&msg| m.kept.is_none_or(|k| msg > k));
        let new = m.new;
        rec.snd = Some(new.clone());
        let c = self.conn_mut(id)?;
        c.sends = ids.into();
        c.send.retry = None;
        c.send.next_at = 0;
        if !fast_rotation(rec.agent_version) {
            // QTEST went already: the old queue is dropped (`AM_QTEST_`).
            rec.switch = None;
            return self.save(store, rec, None, b);
        }
        let mut ratchet = ratchet.ok_or(AgentError::Prohibited("no ratchet"))?;
        let env = agent_envelope(&mut rec, &mut ratchet, AMessage::Qend(vec![addr(&old)]))?;
        // A message that never went on the old queue goes only on the new one; QEND after it on the
        // old queue would skip it in the peer's chain.
        rec.switch = if moved {
            None
        } else {
            Some(Switch::EndingOld {
                body: seal(&old, &env, false)?,
                old,
                // `enqueue_built` numbers QEND on the new queue next.
                last: AgentMsgId(rec.last_id + 1),
                started_at: self.now(),
            })
        };
        let body = seal(&new, &env, false)?;
        self.enqueue_built(store, rec, ratchet, SendKind::Quiet, body, b)?;
        Ok(())
    }

    /// The new queue cannot be used, or the switch did not finish in time: we stay on the old queue, as
    /// 7.1.4 does with a failed `ICQSndSecure`, and drop the messages kept for the new one.
    pub(super) fn give_up(&mut self, store: &mut Store, id: ConnId, err: AgentError) -> Result<()> {
        let (mut rec, _) = self.editable(id)?;
        let mut b = Batch::default();
        let m = match rec.switch.take() {
            Some(Switch::Moving(m)) => m,
            // Only QEND's copy for the old queue was left, which is dropped silently (`AM_QEND_`).
            Some(Switch::EndingOld { .. }) => return self.save(store, rec, None, b),
            None => return Ok(()),
        };
        if let Some(kept) = m.kept {
            for msg in records::pending_ids(store, id)? {
                if msg <= kept {
                    b.delete(records::send_key(id, msg));
                }
            }
        }
        self.save(store, rec, None, b)?;
        self.emit(AgentEvent::Err { conn: id, err });
        Ok(())
    }
}
