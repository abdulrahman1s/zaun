//! New queue keys and the SMP requests a connection's queues need (Agent.hs `newRcvQueue`, `newSndQueue`; Client.hs `createSMPQueue`, `secureSndQueue`, `subscribeQueues`).
use crate::crypto::{Alg, KeyPair, PrivateKey, sha3_384};
use crate::encoding::Version;
use crate::smp::{
    Command, ContactLink, MessagingLink, NewQueue, QueueLinkData, QueueReqData, Request, SmpServer,
    SubscriptionMode,
};

use crate::agent::error::{AgentError, Result};
use crate::agent::records::{NewLink, RcvQueue, SndQueue};
use crate::agent::short_link::contact_short_link_kdf;
use crate::agent::version::CURRENT_CLIENT;
use crate::agent::{QueueMode, SmpQueueAddress, SmpQueueInfo};

pub(crate) fn x25519() -> Result<PrivateKey> {
    Ok(KeyPair::generate(Alg::X25519)?.private)
}

/// Keys for a queue NEW will create on `server`; the ids come with the answer.
pub(crate) fn new_rcv_queue(server: SmpServer) -> Result<RcvQueue> {
    Ok(RcvQueue {
        server,
        rcv_id: Vec::new(),
        snd_id: Vec::new(),
        auth: KeyPair::generate(Alg::Ed25519)?.private,
        dh_priv: x25519()?,
        server_dh: None,
        e2e_priv: x25519()?,
        peer_e2e: None,
        client_version: CURRENT_CLIENT,
    })
}

/// The peer's queue from its link or reply info. `auth` is the LKEY key when the short link already secured it.
pub(crate) fn new_snd_queue(
    info: &SmpQueueInfo,
    client_version: Version,
    auth: Option<PrivateKey>,
) -> Result<SndQueue> {
    let a = &info.address;
    if !crate::agent::sender_can_secure(a.queue_mode) {
        return Err(AgentError::Unsupported(
            "queues the sender cannot secure (agent < 7)",
        ));
    }
    Ok(SndQueue {
        server: a.server.clone(),
        snd_id: a.sender_id.clone(),
        secured: auth.is_some(),
        auth: auth.map_or_else(x25519, Ok)?,
        e2e_priv: x25519()?,
        peer_dh: a.dh_public_key.clone(),
        client_version,
    })
}

/// The address the peer sends to, for our link or reply-queue info.
pub(crate) fn queue_address(rq: &RcvQueue, sender_id: Vec<u8>) -> Result<SmpQueueAddress> {
    address(rq, sender_id, QueueMode::Messaging)
}

/// The address of a contact queue: senders cannot secure it, so they send unsigned.
pub(crate) fn contact_address(rq: &RcvQueue, sender_id: Vec<u8>) -> Result<SmpQueueAddress> {
    address(rq, sender_id, QueueMode::Contact)
}

fn address(rq: &RcvQueue, sender_id: Vec<u8>, mode: QueueMode) -> Result<SmpQueueAddress> {
    Ok(SmpQueueAddress {
        server: rq.server.clone(),
        sender_id,
        dh_public_key: rq.e2e_priv.public()?,
        queue_mode: Some(mode),
    })
}

/// `take 24 (sha3_384 corrId)`: a short link's sender id, fixed before the queue exists.
pub(crate) fn link_sender_id(corr_id: &[u8]) -> Vec<u8> {
    sha3_384(corr_id)[..24].to_vec()
}

/// NEW, signed by the queue's recipient key; with a short link, its data and the corrId it depends on.
/// `contact` makes the queue an address's: its link id comes from the link key.
pub(crate) fn new_request(
    rq: &RcvQueue,
    link: Option<&NewLink>,
    contact: bool,
    basic_auth: Option<String>,
) -> Result<Request> {
    let mut q = NewQueue::messaging(
        rq.auth.public()?,
        rq.dh_priv.public()?,
        SubscriptionMode::Subscribe,
    );
    q.basic_auth = basic_auth;
    let data = link.map(|l| QueueLinkData {
        fixed: l.fixed.clone(),
        user: l.user.clone(),
    });
    q.queue_req_data = match (link, data, contact) {
        (Some(l), Some(data), false) => Some(QueueReqData::Messaging(Some(MessagingLink {
            sender_id: link_sender_id(&l.corr_id),
            data,
        }))),
        (Some(l), Some(data), true) => Some(QueueReqData::Contact(Some(ContactLink {
            link_id: contact_short_link_kdf(&l.link_key)?.0,
            sender_id: link_sender_id(&l.corr_id),
            data,
        }))),
        (_, _, true) => Some(QueueReqData::Contact(None)),
        (_, _, false) => q.queue_req_data.take(),
    };
    let mut r = Request::new(Command::New(Box::new(q)), Vec::new(), Some(rq.auth.clone()));
    r.corr_id = link.map(|l| l.corr_id.clone());
    Ok(r)
}

pub(crate) fn skey_request(sq: &SndQueue) -> Result<Request> {
    let key = sq.auth.public()?;
    Ok(Request::new(
        Command::Skey(key),
        sq.snd_id.clone(),
        Some(sq.auth.clone()),
    ))
}

pub(crate) fn sub_request(rq: &RcvQueue) -> Request {
    Request::new(Command::Sub, rq.rcv_id.clone(), Some(rq.auth.clone()))
}

pub(crate) fn ack_request(rq: &RcvQueue, msg_id: &[u8]) -> Request {
    let cmd = Command::Ack {
        msg_id: msg_id.to_vec(),
    };
    Request::new(cmd, rq.rcv_id.clone(), Some(rq.auth.clone()))
}

pub(crate) fn del_request(rq: &RcvQueue) -> Request {
    Request::new(Command::Del, rq.rcv_id.clone(), Some(rq.auth.clone()))
}
