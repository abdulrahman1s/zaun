//! Create a one-time invitation: queue keys and X3DH keys, NEW (with the short link's data), then the links (Agent.hs `newConn`, `newRcvConnSrv`; HANDSHAKE.md §1).
use crate::crypto::{Alg, KeyPair, random_vec};
use crate::ratchet::{PqSupport, generate_rcv_params};
use crate::smp::QueueIds;
use crate::store::Store;

use super::params::link_params;
use super::queues::{link_sender_id, new_rcv_queue, queue_address};
use crate::agent::SmpQueueUri;
use crate::agent::client::{AgentClient, AgentEvent, ConnId};
use crate::agent::error::{AgentError, Result};
use crate::agent::links::{
    ConnReqUriData, ConnShortLink, ConnectionRequestUri, ServiceScheme, ShortLinkScheme,
    shorten_short_link,
};
use crate::agent::records::{Batch, ConnRecord, ConnStatus, NewLink};
use crate::agent::short_link::{
    UserConnLinkData, UserLinkData, encode_sign_fixed_data, encode_sign_user_data,
    encrypt_fixed_data, encrypt_user_data, inv_short_link_kdf,
};
use crate::agent::version::{CURRENT_AGENT, supported_agent_range, supported_client_range};

impl AgentClient {
    /// The links come in `AgentEvent::Invitation` once the server has created the queue.
    pub fn create_invitation(
        &mut self,
        store: &mut Store,
        pq: bool,
        short: bool,
    ) -> Result<ConnId> {
        self.create_invitation_with(store, pq, short, Vec::new())
    }

    /// `user_data` goes into the short link; chat puts the profile there (`ContactShortLinkData`).
    pub fn create_invitation_with(
        &mut self,
        store: &mut Store,
        pq: bool,
        short: bool,
        user_data: Vec<u8>,
    ) -> Result<ConnId> {
        let server = self
            .cfg
            .servers
            .first()
            .cloned()
            .ok_or(AgentError::Prohibited("no SMP server configured"))?;
        let id = ConnId(store.next_id("agent.conn")?);
        let mut rec = ConnRecord::new(id, ConnStatus::Creating, CURRENT_AGENT, pq);
        rec.rcv = Some(new_rcv_queue(server)?);
        rec.x3dh = Some(generate_rcv_params(crate::ratchet::CURRENT_E2E_VERSION, PqSupport(pq))?.0);
        if short {
            rec.new_link = Some(new_link(&rec, user_data)?);
        }
        self.save(store, rec, None, Batch::default())?;
        self.drive(store, id);
        Ok(id)
    }

    pub(crate) fn on_invitation_created(
        &mut self,
        store: &mut Store,
        id: ConnId,
        ids: QueueIds,
    ) -> Result<()> {
        let (mut rec, _) = self.editable(id)?;
        let rq = rec.rcv.as_mut().ok_or(AgentError::Prohibited("no queue"))?;
        rq.rcv_id = ids.rcv_id;
        rq.snd_id = ids.snd_id.clone();
        rq.server_dh = Some(ids.rcv_dh_key);
        let server = rq.server.clone();
        let full = invitation_uri(&rec, ids.snd_id.clone())?.to_uri();
        let short = match rec.new_link.take() {
            Some(l) => {
                let link_id = ids
                    .link_id
                    .ok_or(AgentError::Link("no link id from the server".into()))?;
                if link_sender_id(&l.corr_id) != ids.snd_id {
                    return Err(AgentError::Link("sender id differs from the link's".into()));
                }
                let link = ConnShortLink::Invitation {
                    scheme: ShortLinkScheme::Server,
                    server,
                    link_id,
                    link_key: l.link_key,
                };
                Some(shorten_short_link(&self.cfg.presets, link).to_uri())
            }
            None => None,
        };
        rec.status = ConnStatus::Invited;
        self.save(store, rec, None, Batch::default())?;
        self.conn_mut(id)?.sub.subscribed = true; // NEW subscribed it on this session
        self.emit(AgentEvent::Invitation {
            conn: id,
            full,
            short,
        });
        Ok(())
    }
}

/// `CRInvitationUri` of our queue and X3DH keys; `sender_id` comes from IDS, or from the corrId for a short link.
fn invitation_uri(rec: &ConnRecord, sender_id: Vec<u8>) -> Result<ConnectionRequestUri> {
    let rq = rec.rcv.as_ref().ok_or(AgentError::Prohibited("no queue"))?;
    let x3dh = rec
        .x3dh
        .as_ref()
        .ok_or(AgentError::Prohibited("no X3DH keys"))?;
    let data = ConnReqUriData {
        scheme: ServiceScheme::Simplex,
        agent_v_range: supported_agent_range(),
        smp_queues: vec![SmpQueueUri {
            client_v_range: supported_client_range(),
            address: queue_address(rq, sender_id)?,
        }],
        client_data: None,
    };
    Ok(ConnectionRequestUri::Invitation {
        data,
        e2e: link_params(x3dh)?,
    })
}

/// The data the server stores for an invitation short link (`prepareConnectionLink`, `encryptLinkData`).
fn new_link(rec: &ConnRecord, user_data: Vec<u8>) -> Result<NewLink> {
    let corr_id = random_vec(24)?;
    let root = KeyPair::generate(Alg::Ed25519)?;
    let req = invitation_uri(rec, link_sender_id(&corr_id))?;
    let vr = supported_agent_range();
    let (link_key, fixed) = encode_sign_fixed_data(&root.private, vr, &req, None)?;
    let user_link = UserConnLinkData::Invitation(UserLinkData(user_data));
    let user = encode_sign_user_data(&root.private, vr, user_link)?;
    let key = inv_short_link_kdf(&link_key)?;
    Ok(NewLink {
        corr_id,
        fixed: encrypt_fixed_data(&key, &fixed)?,
        user: encrypt_user_data(&key, &user)?,
        link_key,
    })
}
