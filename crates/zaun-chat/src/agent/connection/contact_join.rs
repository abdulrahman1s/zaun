//! Joining an address: a queue for the answer, then our request sent unsigned to the address queue; an address that publishes ratchet keys gets the ratchet-encrypted form (Agent.hs `joinConnSrv'` for `CRContactUri`, `compatibleContactUri`).
use crate::encoding::{Decode, Encode};
use crate::ratchet::{
    CURRENT_E2E_VERSION, PqSupport, Ratchet, RatchetVersions, compatible_e2e_version,
    generate_rcv_params,
};
use crate::smp::QueueIds;
use crate::store::Store;

use super::invite::invitation_uri;
use super::params::rcv_params;
use super::queues::queue_address;
use super::reply::encrypt_conn_info;
use crate::agent::SmpQueueInfo;
use crate::agent::client::{AgentClient, ConnId};
use crate::agent::delivery::seal_once;
use crate::agent::e2e::E2eParams as WireParams;
use crate::agent::envelope::AgentMsgEnvelope;
use crate::agent::error::{AgentError, Result};
use crate::agent::links::ConnectionRequestUri;
use crate::agent::message::AgentMessage;
use crate::agent::msg_hash;
use crate::agent::records::{Batch, ConnRecord, ConnStatus, ContactRequest};
use crate::agent::version::{supported_agent_range, supported_client_range};

impl AgentClient {
    /// The versions and keys of a request to `req`: its reply queue is next. Returns the ratchet when the address has keys.
    pub(super) fn begin_request(
        &self,
        rec: &mut ConnRecord,
        req: &ConnectionRequestUri,
    ) -> Result<Option<Ratchet>> {
        let ConnectionRequestUri::Contact { data, ratchet_keys } = req else {
            return Err(AgentError::Prohibited("not an address"));
        };
        let agent_v = data
            .agent_v_range
            .highest_common(&supported_agent_range())
            .ok_or(AgentError::Version)?;
        let q = data
            .smp_queues
            .first()
            .ok_or(AgentError::Parse("link without queues"))?;
        let client_v = q
            .client_v_range
            .highest_common(&supported_client_range())
            .ok_or(AgentError::Version)?;
        let (ratchet, ratchet_key_id) = match ratchet_keys {
            Some(keys) => {
                let e2e_v = compatible_e2e_version(&keys.e2e.v_range).ok_or(AgentError::Version)?;
                let rcv = rcv_params(&keys.e2e, e2e_v)?;
                let (r, reply) =
                    Ratchet::join(RatchetVersions::new(e2e_v), &rcv, PqSupport(rec.pq))?;
                rec.e2e_reply = Some(reply.to_bytes()?);
                (Some(r), Some(keys.ratchet_key_id.clone()))
            }
            None => {
                let (x3dh, _) = generate_rcv_params(CURRENT_E2E_VERSION, PqSupport(rec.pq))?;
                rec.x3dh = Some(x3dh);
                (None, None)
            }
        };
        rec.agent_version = agent_v;
        rec.rcv = Some(super::queues::new_rcv_queue(self.reply_server()?)?);
        rec.request = Some(ContactRequest {
            queue: SmpQueueInfo::from_uri(q, client_v),
            agent_version: agent_v,
            ratchet_key_id,
            body: None,
        });
        rec.status = ConnStatus::Applying;
        Ok(ratchet)
    }

    /// The queue for the owner's answer exists: build the request and seal it once, so retries send the same bytes.
    pub(crate) fn on_request_queue_created(
        &mut self,
        store: &mut Store,
        id: ConnId,
        ids: QueueIds,
    ) -> Result<()> {
        let (mut rec, ratchet) = self.editable(id)?;
        let rq = rec.rcv.as_mut().ok_or(AgentError::Prohibited("no queue"))?;
        rq.rcv_id = ids.rcv_id;
        rq.snd_id = ids.snd_id.clone();
        rq.server_dh = Some(ids.rcv_dh_key);
        let reply = SmpQueueInfo {
            client_version: rq.client_version,
            address: queue_address(rq, ids.snd_id.clone())?,
        };
        let mut req = rec
            .request
            .take()
            .ok_or(AgentError::Prohibited("no request"))?;
        let conn_info = rec.conn_info.take().unwrap_or_default();
        let (envelope, ratchet) = request_envelope(&mut rec, ratchet, &req, reply, conn_info)?;
        req.body = Some(seal_once(&req.queue, &envelope.to_bytes()?)?);
        rec.request = Some(req);
        rec.status = ConnStatus::Requesting;
        self.save(store, rec, ratchet, Batch::default())?;
        self.conn_mut(id)?.sub.subscribed = true; // NEW subscribed it on this session
        self.drive(store, id);
        Ok(())
    }

    /// The address took the request: now we wait for the owner to join the queue we made.
    pub(crate) fn on_request_sent(&mut self, store: &mut Store, id: ConnId) -> Result<()> {
        let (mut rec, _) = self.editable(id)?;
        rec.request = None;
        rec.status = ConnStatus::Invited;
        self.save(store, rec, None, Batch::default())
    }
}

/// The request as the address expects it: our invitation, or the ratchet-encrypted reply queue and info.
fn request_envelope(
    rec: &mut ConnRecord,
    ratchet: Option<Ratchet>,
    req: &ContactRequest,
    reply: SmpQueueInfo,
    conn_info: Vec<u8>,
) -> Result<(AgentMsgEnvelope, Option<Ratchet>)> {
    let Some(ratchet_key_id) = req.ratchet_key_id.clone() else {
        let conn_req = invitation_uri(rec, reply.address.sender_id)?;
        let env = AgentMsgEnvelope::Invitation {
            agent_version: req.agent_version,
            conn_req,
            conn_info,
        };
        return Ok((env, None));
    };
    let mut ratchet = ratchet.ok_or(AgentError::Prohibited("no ratchet"))?;
    let plain = AgentMessage::ConnInfoReply {
        queues: vec![reply],
        conn_info,
    }
    .to_bytes()?;
    let e2e = rec
        .e2e_reply
        .take()
        .ok_or(AgentError::Prohibited("no ratchet params"))?;
    rec.chain.snd_id = 1;
    rec.chain.snd_hash = msg_hash(&plain).to_vec();
    let env = AgentMsgEnvelope::ContactRequest {
        agent_version: req.agent_version,
        e2e_snd_params: WireParams::from_bytes(&e2e)?,
        ratchet_key_id,
        enc_conn_info: encrypt_conn_info(&mut ratchet, &plain)?,
    };
    Ok((env, Some(ratchet)))
}
