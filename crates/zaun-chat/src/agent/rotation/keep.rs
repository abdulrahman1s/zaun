//! What a sent message means to a switch: before the move it is kept for the new queue, as 7.1.4 delivers to both queues (AgentStore.hs `copyPendingSndDeliveries`, Agent.hs `isActiveSndQ`); QEND on the new queue ends it (the `AM_QEND_` outcome).
use crate::store::Store;

use super::state::Switch;
use crate::agent::client::{AgentClient, ConnId};
use crate::agent::error::Result;
use crate::agent::records::{self, Batch, PendingSend, SendKind};

impl AgentClient {
    /// True when the switch has stored what follows the send of `p`, so `on_sent` must not delete it.
    pub(crate) fn switch_sent(
        &mut self,
        store: &mut Store,
        id: ConnId,
        p: &PendingSend,
    ) -> Result<bool> {
        match &self.conn(id)?.rec.switch {
            Some(Switch::Moving(m)) if keeps(p, m.keep_after) => self.keep(store, id, p),
            Some(Switch::EndingOld { last, .. }) if *last == p.id => self.ended(store, id, p),
            Some(Switch::Moving(_) | Switch::EndingOld { .. }) | None => Ok(false),
        }
    }

    /// Stored again as quiet, so that the engine hears of it once; the peer's ratchet drops whichever
    /// copy arrives second.
    fn keep(&mut self, store: &mut Store, id: ConnId, p: &PendingSend) -> Result<bool> {
        let (mut rec, _) = self.editable(id)?;
        let Some(Switch::Moving(m)) = rec.switch.as_mut() else {
            return Ok(false);
        };
        m.kept = m.kept.max(Some(p.id));
        let mut b = Batch::default();
        b.pending(
            id,
            &PendingSend {
                kind: SendKind::Quiet,
                ..p.clone()
            },
        )?;
        self.save(store, rec, None, b)?;
        Ok(true)
    }

    /// QEND reached the new queue first: the old queue's copy is not needed any more.
    fn ended(&mut self, store: &mut Store, id: ConnId, p: &PendingSend) -> Result<bool> {
        let (mut rec, _) = self.editable(id)?;
        rec.switch = None;
        let mut b = Batch::default();
        b.delete(records::send_key(id, p.id));
        self.save(store, rec, None, b)?;
        Ok(true)
    }
}

fn keeps(p: &PendingSend, keep_after: Option<crate::agent::client::AgentMsgId>) -> bool {
    let kind = match p.kind {
        // A re-sync's key or EREADY is kept like a message; the peer drops a second copy.
        SendKind::Msg | SendKind::Receipt | SendKind::Qcont | SendKind::Resync => true,
        // QKEY needs no second copy, and a handshake confirmation is never sent on an active connection.
        SendKind::Confirmation | SendKind::Switch | SendKind::Quiet => false,
    };
    kind && keep_after.is_some_and(|after| p.id > after)
}
