//! What a handshake step's answer leads to, by the status that sent it.
use crate::smp::{Response, SmpError};
use crate::store::Store;

use crate::agent::client::{AgentClient, ConnId, OpKind, is_temporary};
use crate::agent::error::{AgentError, Result};
use crate::agent::records::ConnStatus;

impl AgentClient {
    /// Routes the answer of a handshake step. A temporary failure is retried; a refusal stops the step until the next `Up`.
    pub(crate) fn on_step_answer(
        &mut self,
        store: &mut Store,
        id: ConnId,
        kind: OpKind,
        answer: Result<Response, SmpError>,
    ) -> Result<()> {
        let retry_at = self.step_retry_after();
        let c = self.conn_mut(id)?;
        c.step.in_flight = false;
        let r = match answer {
            Err(e) if is_temporary(&e) => {
                c.step.retry_at = retry_at;
                return Ok(());
            }
            Err(e) => Err(e.into()),
            Ok(r) => match kind {
                OpKind::NewQueue => self.on_queue_created(store, id, r),
                OpKind::Lkey => self.on_link_resolved(store, id, r),
                OpKind::Lget => self.on_contact_resolved(store, id, r),
                OpKind::Skey => self.on_secured(store, id),
                OpKind::Request => self.on_request_sent(store, id),
                OpKind::Sub | OpKind::Ack | OpKind::Send(_) | OpKind::Del | OpKind::Switch(_) => {
                    Ok(())
                }
            },
        };
        if r.is_err()
            && let Ok(c) = self.conn_mut(id)
        {
            c.step.retry_at = i64::MAX;
        }
        r
    }

    fn on_queue_created(&mut self, store: &mut Store, id: ConnId, r: Response) -> Result<()> {
        let Response::Ids(ids) = r else {
            return Err(AgentError::Prohibited("NEW answered without IDS"));
        };
        match self.conn(id)?.rec.status {
            ConnStatus::Creating => self.on_invitation_created(store, id, ids),
            ConnStatus::Replying => self.on_reply_created(store, id, ids),
            ConnStatus::Addressing => self.on_address_created(store, id, ids),
            ConnStatus::Applying => self.on_request_queue_created(store, id, ids),
            ConnStatus::Invited
            | ConnStatus::Address
            | ConnStatus::Requesting
            | ConnStatus::Confirmed
            | ConnStatus::Allowing
            | ConnStatus::Accepting
            | ConnStatus::Resolving
            | ConnStatus::Securing
            | ConnStatus::Joined
            | ConnStatus::Active => Ok(()),
        }
    }

    fn on_secured(&mut self, store: &mut Store, id: ConnId) -> Result<()> {
        match self.conn(id)?.rec.status {
            ConnStatus::Securing => self.on_inviter_secured(store, id),
            ConnStatus::Allowing => self.on_reply_secured(store, id),
            ConnStatus::Creating
            | ConnStatus::Addressing
            | ConnStatus::Address
            | ConnStatus::Applying
            | ConnStatus::Requesting
            | ConnStatus::Invited
            | ConnStatus::Confirmed
            | ConnStatus::Accepting
            | ConnStatus::Resolving
            | ConnStatus::Replying
            | ConnStatus::Joined
            | ConnStatus::Active => Ok(()),
        }
    }
}
