//! A connection as stored: handshake status, queues, message chain and what an unfinished step needs (Agent/Store.hs `ConnData`, AGPL-3.0-only).
use crate::crypto::PrivateKey;
use crate::encoding::Version;
use crate::ratchet::PrivParams;

use super::queues::{RcvQueue, SndQueue};
use super::request::ContactRequest;
use super::status::ConnStatus;
use crate::agent::client::ConnId;
use crate::agent::queue_info::SmpQueueInfo;
use crate::agent::resync::RatchetSync;
use crate::agent::rotation::Switch;

/// The ids and hashes that chain each side's agent messages (`APrivHeader`, `checkMsgIntegrity`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Chain {
    /// Our last `sndMsgId`; the confirmation is 1.
    pub snd_id: i64,
    pub snd_hash: Vec<u8>,
    /// The peer's last `sndMsgId` we received.
    pub rcv_id: i64,
    pub rcv_hash: Vec<u8>,
}

/// The short link of an invitation: NEW carries it, so it is kept until the server answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewLink {
    /// The sender id is derived from it, so NEW is re-sent with the same one.
    pub corr_id: Vec<u8>,
    pub fixed: Vec<u8>,
    pub user: Vec<u8>,
    pub link_key: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnRecord {
    pub id: ConnId,
    pub status: ConnStatus,
    pub agent_version: Version,
    pub pq: bool,
    pub rcv: Option<RcvQueue>,
    pub snd: Option<SndQueue>,
    /// Inviter: the X3DH private keys, until the confirmation arrives.
    pub x3dh: Option<PrivParams>,
    pub chain: Chain,
    /// The last internal message id used, sent or received.
    pub last_id: i64,
    /// Our connection info, until the confirmation that carries it is queued.
    pub conn_info: Option<Vec<u8>>,
    /// Joiner: the short link, until LKEY resolves it.
    pub link: Option<String>,
    /// Joiner: the sender key LKEY secures the queue with; it becomes the send queue's key.
    pub lkey: Option<PrivateKey>,
    /// Joiner: our ratchet params for the confirmation, as encoded in it.
    pub e2e_reply: Option<Vec<u8>>,
    /// Inviter: the joiner's reply queue, from its confirmation until `allow`.
    pub peer_reply: Option<SmpQueueInfo>,
    pub new_link: Option<NewLink>,
    /// Requester: the request to an address, until the address has taken it.
    pub request: Option<ContactRequest>,
    /// Address owner: the `clientData` of its link, until the server answers. Only a group's link
    /// has one (`{"groupLinkId":…}`), and it makes the short link a `/g#` one.
    pub client_data: Option<String>,
    /// Sender: the peer is moving its queue (`QADD`) and we follow.
    pub switch: Option<Switch>,
    /// The ratchet-sync state and what a re-sync keeps; `None` is `RSOk`.
    pub sync: Option<RatchetSync>,
}

impl ConnRecord {
    #[must_use]
    pub fn new(id: ConnId, status: ConnStatus, agent_version: Version, pq: bool) -> Self {
        Self {
            id,
            status,
            agent_version,
            pq,
            rcv: None,
            snd: None,
            x3dh: None,
            chain: Chain::default(),
            last_id: 0,
            conn_info: None,
            link: None,
            lkey: None,
            e2e_reply: None,
            peer_reply: None,
            new_link: None,
            request: None,
            client_data: None,
            switch: None,
            sync: None,
        }
    }
}
