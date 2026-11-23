//! The messages only a v7 switch sends: QKEY on the old queue, QTEST alone on the new one (Agent.hs `qAddMsg`, `qUseMsg`).
use crate::store::Store;

use super::addr::addr;
use super::state::{Moving, Step, Switch};
use crate::agent::client::{AgentClient, AgentMsgId, ConnId};
use crate::agent::delivery::{agent_envelope, seal};
use crate::agent::error::{AgentError, Result};
use crate::agent::message::AMessage;
use crate::agent::queue_info::SmpQueueInfo;
use crate::agent::queue_uri::SmpQueueAddress;
use crate::agent::records::{Batch, SendKind};

impl AgentClient {
    /// QKEY on the current queue: the new queue with our new e2e key in place of the peer's, and our new sender key.
    pub(super) fn queue_qkey(&mut self, store: &mut Store, id: ConnId, m: Moving) -> Result<()> {
        let (mut rec, ratchet) = self.editable(id)?;
        let mut ratchet = ratchet.ok_or(AgentError::Prohibited("no ratchet"))?;
        let sq = rec
            .snd
            .clone()
            .ok_or(AgentError::Prohibited("no send queue"))?;
        let info = SmpQueueInfo {
            client_version: m.new.client_version,
            address: SmpQueueAddress {
                server: m.new.server.clone(),
                sender_id: m.new.snd_id.clone(),
                dh_public_key: m.new.e2e_priv.public()?,
                queue_mode: m.mode,
            },
        };
        let qkey = AMessage::Qkey(vec![(info, m.new.auth.public()?)]);
        let body = seal(&sq, &agent_envelope(&mut rec, &mut ratchet, qkey)?, false)?;
        rec.switch = Some(Switch::Moving(Moving {
            step: Step::WaitingQuse,
            ..m
        }));
        self.enqueue_built(
            store,
            rec,
            ratchet,
            SendKind::Switch,
            body,
            Batch::default(),
        )?;
        Ok(())
    }

    /// QTEST, built once and stored, so that a retry or a restart sends the same message. From here on
    /// what goes on the old queue goes again on the new one, after QTEST, as 7.1.4 sends it to both.
    pub(super) fn build_qtest(&mut self, store: &mut Store, id: ConnId, m: Moving) -> Result<()> {
        let (mut rec, ratchet) = self.editable(id)?;
        let mut ratchet = ratchet.ok_or(AgentError::Prohibited("no ratchet"))?;
        let qtest = AMessage::Qtest(vec![addr(&m.new)]);
        let body = seal(
            &m.new,
            &agent_envelope(&mut rec, &mut ratchet, qtest)?,
            false,
        )?;
        let keep_after = m.keep_after.or(Some(AgentMsgId(rec.last_id)));
        rec.switch = Some(Switch::Moving(Moving {
            step: Step::Testing { body },
            keep_after,
            ..m
        }));
        self.save(store, rec, Some(ratchet), Batch::default())
    }
}
