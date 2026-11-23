//! What happens to a queued message after an attempt: gone, given up, or tried again later (Agent.hs `runSmpQueueMsgDelivery`).
use crate::store::Store;

use super::retry::{Retry, RetryMode};
use crate::agent::client::{AgentClient, AgentEvent, ConnId, ms};
use crate::agent::error::{AgentError, Result};
use crate::agent::records::{self, Batch, ConnStatus, PendingSend, SendKind};

impl AgentClient {
    pub(super) fn on_sent(&mut self, store: &mut Store, id: ConnId, p: &PendingSend) -> Result<()> {
        let mut b = Batch::default();
        b.delete(records::send_key(id, p.id));
        b.apply(store)?;
        self.pop_send(id);
        match p.kind {
            SendKind::Msg => self.emit(AgentEvent::Sent {
                conn: id,
                msg_id: p.id,
            }),
            SendKind::Confirmation if self.conn(id)?.rec.status == ConnStatus::Accepting => {
                let (mut rec, _) = self.editable(id)?;
                rec.status = ConnStatus::Active;
                self.save(store, rec, None, Batch::default())?;
                self.emit(AgentEvent::Con { conn: id });
            }
            SendKind::Confirmation | SendKind::Receipt | SendKind::Qcont => {}
        }
        Ok(())
    }

    /// Gives up on `p`. The engine hears of its own messages and of a failed handshake; a receipt that cannot go is dropped silently.
    pub(super) fn drop_send(
        &mut self,
        store: &mut Store,
        id: ConnId,
        p: &PendingSend,
        err: AgentError,
    ) -> Result<()> {
        let mut b = Batch::default();
        b.delete(records::send_key(id, p.id));
        b.apply(store)?;
        self.pop_send(id);
        match p.kind {
            SendKind::Msg => self.emit(AgentEvent::Merr {
                conn: id,
                msg_id: p.id,
                err,
            }),
            SendKind::Confirmation => self.emit(AgentEvent::Err { conn: id, err }),
            SendKind::Receipt | SendKind::Qcont => {}
        }
        Ok(())
    }

    /// `messageTimeout` after temporary failures, `quotaExceededTimeout` while the peer's queue is full.
    pub(super) fn expired(&self, id: ConnId, p: &PendingSend, now: i64) -> bool {
        let slow = self
            .conns
            .get(&id)
            .and_then(|c| c.send.retry)
            .is_some_and(|r| r.mode == RetryMode::Slow);
        let d = &self.cfg.delivery;
        let limit = if slow {
            d.quota_timeout
        } else {
            d.message_timeout
        };
        now - p.created_at > ms(limit)
    }

    pub(super) fn pop_send(&mut self, id: ConnId) {
        if let Some(c) = self.conns.get_mut(&id) {
            c.sends.pop_front();
            c.send.retry = None;
            c.send.next_at = 0;
        }
    }

    pub(super) fn schedule_retry(&mut self, id: ConnId, mode: RetryMode) {
        let now = self.now();
        let ri = match mode {
            RetryMode::Fast => self.cfg.delivery.fast,
            RetryMode::Slow => self.cfg.delivery.slow,
        };
        let Some(c) = self.conns.get_mut(&id) else {
            return;
        };
        let mut r = c
            .send
            .retry
            .filter(|r| r.mode == mode)
            .unwrap_or_else(|| Retry::new(mode, &ri));
        c.send.next_at = now + r.next_delay(&ri);
        c.send.retry = Some(r);
    }
}
