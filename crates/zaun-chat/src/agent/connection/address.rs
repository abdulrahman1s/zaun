//! Create a contact address: a contact-mode queue with its short link, made in one NEW (Agent.hs `prepareConnectionLink'`, `createConnectionForLink'`, `encryptContactLinkData`; fact sheet §10).
use crate::crypto::{Alg, KeyPair, random_vec, sha256};
use crate::smp::QueueIds;
use crate::store::Store;

use super::queues::{contact_address, link_sender_id, new_rcv_queue};
use crate::agent::SmpQueueUri;
use crate::agent::client::{AgentClient, AgentEvent, ConnId};
use crate::agent::error::{AgentError, Result};
use crate::agent::links::{
    ConnReqUriData, ConnShortLink, ConnectionRequestUri, ContactConnType, ServiceScheme,
    ShortLinkScheme, shorten_short_link,
};
use crate::agent::records::{Batch, ConnRecord, ConnStatus, NewLink, RcvQueue};
use crate::agent::short_link::{
    UserConnLinkData, UserContactData, UserLinkData, contact_short_link_kdf,
    encode_sign_fixed_data, encode_sign_user_data, encrypt_fixed_data, encrypt_user_data,
};
use crate::agent::version::{CURRENT_AGENT, supported_agent_range, supported_client_range};

impl AgentClient {
    /// The links come in `AgentEvent::Address`. `user_data` goes into the short link (chat's `ContactShortLinkData`).
    /// Like the apps' default address it takes plain requests: no ratchet keys are published.
    pub fn create_address(
        &mut self,
        store: &mut Store,
        short: bool,
        user_data: Vec<u8>,
    ) -> Result<ConnId> {
        self.create_address_with(store, short, user_data, None)
    }

    /// `client_data` is the link's `clientData`; a group's link has one and is a `/g#` link.
    pub fn create_address_with(
        &mut self,
        store: &mut Store,
        short: bool,
        user_data: Vec<u8>,
        client_data: Option<String>,
    ) -> Result<ConnId> {
        let server = self
            .cfg
            .servers
            .first()
            .cloned()
            .ok_or(AgentError::Prohibited("no SMP server configured"))?;
        let id = ConnId(store.next_id("agent.conn")?);
        let mut rec = ConnRecord::new(id, ConnStatus::Addressing, CURRENT_AGENT, true);
        let rq = new_rcv_queue(server)?;
        if short {
            rec.new_link = Some(address_link(&rq, user_data, client_data.clone())?);
        }
        rec.client_data = client_data;
        rec.rcv = Some(rq);
        self.save(store, rec, None, Batch::default())?;
        self.drive(store, id);
        Ok(id)
    }

    pub(crate) fn on_address_created(
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
        let rq = rq.clone();
        let full = contact_uri(&rq, ids.snd_id.clone(), rec.client_data.clone())?.to_uri();
        let short = match rec.new_link.take() {
            Some(l) => {
                if link_sender_id(&l.corr_id) != ids.snd_id {
                    return Err(AgentError::Link("sender id differs from the link's".into()));
                }
                let link = ConnShortLink::Contact {
                    scheme: ShortLinkScheme::Server,
                    contact_type: if rec.client_data.is_some() {
                        ContactConnType::Group
                    } else {
                        ContactConnType::Contact
                    },
                    server: rq.server,
                    link_key: l.link_key,
                };
                Some(shorten_short_link(&self.cfg.presets, link).to_uri())
            }
            None => None,
        };
        rec.client_data = None;
        rec.status = ConnStatus::Address;
        self.save(store, rec, None, Batch::default())?;
        self.conn_mut(id)?.sub.subscribed = true; // NEW subscribed it on this session
        self.emit(AgentEvent::Address {
            conn: id,
            full,
            short,
        });
        Ok(())
    }
}

/// `CRContactUri` of our queue; `sender_id` comes from IDS, or from the corrId for a short link.
fn contact_uri(
    rq: &RcvQueue,
    sender_id: Vec<u8>,
    client_data: Option<String>,
) -> Result<ConnectionRequestUri> {
    let data = ConnReqUriData {
        scheme: ServiceScheme::Simplex,
        agent_v_range: supported_agent_range(),
        smp_queues: vec![SmpQueueUri {
            client_v_range: supported_client_range(),
            address: contact_address(rq, sender_id)?,
        }],
        client_data,
    };
    Ok(ConnectionRequestUri::Contact {
        data,
        ratchet_keys: None,
    })
}

/// The data the server stores for an address: signed by a root key whose hash is the link's entity id.
fn address_link(rq: &RcvQueue, user_data: Vec<u8>, client_data: Option<String>) -> Result<NewLink> {
    let corr_id = random_vec(24)?;
    let root = KeyPair::generate(Alg::Ed25519)?;
    let req = contact_uri(rq, link_sender_id(&corr_id), client_data)?;
    let vr = supported_agent_range();
    let entity_id = sha256(root.public.as_bytes()).to_vec();
    let (link_key, fixed) = encode_sign_fixed_data(&root.private, vr, &req, Some(entity_id))?;
    let user_link = UserConnLinkData::Contact(UserContactData {
        direct: true,
        owners: Vec::new(),
        relays: Vec::new(),
        user_data: UserLinkData(user_data),
        ratchet_keys: None,
    });
    let user = encode_sign_user_data(&root.private, vr, user_link)?;
    let (_, key) = contact_short_link_kdf(&link_key)?;
    Ok(NewLink {
        corr_id,
        fixed: encrypt_fixed_data(&key, &fixed)?,
        user: encrypt_user_data(&key, &user)?,
        link_key,
    })
}
