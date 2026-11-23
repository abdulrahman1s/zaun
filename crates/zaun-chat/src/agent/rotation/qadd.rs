//! The peer moves its receive queue: a new send queue to follow it, by agent v8 SKEY or by v7 QKEY (Agent.hs `qAddMsg`, `newSndQueue`).
use super::addr::{addr, same_addr};
use super::receive::refuse_unsynced;
use super::state::{Moving, Step, Switch};
use crate::agent::client::{AgentClient, AgentMsgId};
use crate::agent::error::{AgentError, Result};
use crate::agent::message::SndQAddr;
use crate::agent::queue_uri::SmpQueueUri;
use crate::agent::records::{ConnRecord, SndQueue};
use crate::agent::version::{fast_rotation, supported_client_range};
use crate::crypto::{Alg, KeyPair};

impl AgentClient {
    pub(super) fn qadd(
        &self,
        rec: &mut ConnRecord,
        queues: Vec<(SmpQueueUri, Option<SndQAddr>)>,
    ) -> Result<()> {
        let (uri, addr) = queues
            .into_iter()
            .next()
            .ok_or(AgentError::Prohibited("QADD: no queue"))?;
        let addr = addr.ok_or(AgentError::Prohibited(
            "adding queue without switching is not supported",
        ))?;
        refuse_unsynced(rec)?;
        let client_v = uri
            .client_v_range
            .highest_common(&supported_client_range())
            .ok_or(AgentError::Version)?;
        let new = new_snd_queue(&uri, client_v)?;
        if used(rec, &new) {
            return Err(AgentError::Prohibited(
                "QADD: queue address is already used in connection",
            ));
        }
        if !rec.snd.as_ref().is_some_and(|old| same_addr(old, &addr)) {
            return Err(AgentError::Prohibited(
                "QADD: replaced queue address is not found in connection",
            ));
        }
        let fast = fast_rotation(rec.agent_version);
        let (step, mut keep_after) = if fast {
            (Step::Securing, Some(AgentMsgId(0)))
        } else {
            (Step::Qkey, None)
        };
        // A second QADD for the same queue replaces the unfinished switch; what it kept still waits.
        let mut kept = None;
        if let Some(Switch::Moving(m)) = &rec.switch {
            keep_after = keep_after.or(m.keep_after);
            kept = m.kept;
        }
        rec.switch = Some(Switch::Moving(Moving {
            new,
            mode: uri.address.queue_mode,
            step,
            keep_after,
            kept,
            started_at: self.now(),
        }));
        Ok(())
    }
}

/// `findQ (qAddress sqInfo) sqs`: the queue we send to, or one a switch holds.
fn used(rec: &ConnRecord, new: &SndQueue) -> bool {
    let a = addr(new);
    let held = rec.switch.as_ref().map(Switch::queue);
    rec.snd.iter().chain(held).any(|q| same_addr(q, &a))
}

/// `newSndQueue` without a key: fresh X25519 sender and e2e keys (`sndAuthAlg`).
fn new_snd_queue(uri: &SmpQueueUri, client_version: crate::encoding::Version) -> Result<SndQueue> {
    let key = || -> Result<_> { Ok(KeyPair::generate(Alg::X25519)?.private) };
    let a = &uri.address;
    Ok(SndQueue {
        server: a.server.clone(),
        snd_id: a.sender_id.clone(),
        auth: key()?,
        e2e_priv: key()?,
        peer_dh: a.dh_public_key.clone(),
        client_version,
        secured: false,
    })
}
