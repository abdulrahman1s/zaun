//! What the engine needs from the agent layer, mirroring simplexmq's Agent API (Agent.hs, AGPL-3.0-only).

use serde::{Deserialize, Serialize};

use crate::agent::links::ConnectionLink;
use crate::store::Store;

/// The agent's connection number (`agent::client::ConnId`), never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ConnId(pub i64);

/// simplexmq's `AgentMsgId`: per connection, increasing.
pub type AgentMsgId = i64;

/// Why an agent call or delivery failed, as far as the engine tells them apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentFailure {
    /// A relay did not answer (network, timeout, host): `ZN-CHT-004`.
    Network(String),
    /// The link was refused or could not be used: `ZN-CHT-005` for a connection.
    BadLink(String),
    Other(String),
}

impl AgentFailure {
    pub fn detail(&self) -> &str {
        match self {
            Self::Network(d) | Self::BadLink(d) | Self::Other(d) => d,
        }
    }
}

/// The agent's events (`ACommand` events in Agent/Protocol.hs) the engine acts on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentEvent {
    /// The link of a `create_invitation`: the full one and the short one when the relay stores it.
    Invitation {
        conn: ConnId,
        full: String,
        short: Option<String>,
    },
    /// `CONF`: the joining side's conn-info arrived on our invitation; answer with `allow`.
    Conf {
        conn: ConnId,
        conn_info: Vec<u8>,
    },
    /// `INFO`: the inviting side's conn-info arrived on a link we joined.
    Info {
        conn: ConnId,
        conn_info: Vec<u8>,
    },
    /// `CON`: both directions work.
    Con {
        conn: ConnId,
    },
    /// `MSG`: must be acknowledged with `ack`, whatever the engine makes of it.
    Msg {
        conn: ConnId,
        msg_id: AgentMsgId,
        body: Vec<u8>,
    },
    Sent {
        conn: ConnId,
        msg_id: AgentMsgId,
    },
    Merr {
        conn: ConnId,
        msg_id: AgentMsgId,
        err: AgentFailure,
    },
    /// `RCVD`: the peer's receipt for a message we sent.
    Rcvd {
        conn: ConnId,
        msg_id: AgentMsgId,
    },
    /// A relay host connected (`UP`) or went away (`DOWN`).
    Up {
        server: String,
    },
    Down {
        server: String,
    },
    Err {
        conn: ConnId,
        err: AgentFailure,
    },
    /// The links of a `create_address`: the full one and the short one when the relay stores it.
    Address {
        conn: ConnId,
        full: String,
        short: Option<String>,
    },
    /// `REQ`: someone used the address `conn`; `invitation` names the request to
    /// `accept_request`/`reject_request`, `conn_info` is their `x.contact`.
    Req {
        conn: ConnId,
        invitation: Vec<u8>,
        conn_info: Vec<u8>,
        pq: bool,
    },
}

/// The agent as the engine drives it: owned by the engine thread, never blocking. Every call
/// may queue events; the engine drains them after each call.
pub trait AgentPort {
    /// What SMP connection threads send to the engine thread, handed back through `on_smp`.
    type Smp: Send + 'static;

    /// `user_data` goes into the short link (the chat layer's `ContactShortLinkData`).
    fn create_invitation(
        &mut self,
        store: &mut Store,
        pq: bool,
        short: bool,
        user_data: Vec<u8>,
    ) -> Result<ConnId, AgentFailure>;
    fn join(
        &mut self,
        store: &mut Store,
        link: &ConnectionLink,
        conn_info: Vec<u8>,
        pq: bool,
    ) -> Result<ConnId, AgentFailure>;
    fn allow(
        &mut self,
        store: &mut Store,
        conn: &ConnId,
        conn_info: Vec<u8>,
    ) -> Result<(), AgentFailure>;
    fn send(
        &mut self,
        store: &mut Store,
        conn: &ConnId,
        body: Vec<u8>,
    ) -> Result<AgentMsgId, AgentFailure>;
    /// `receipt`: `Some` asks for an `A_RCVD` (chat passes an empty one, `ackMessage … (Just "")`).
    fn ack(
        &mut self,
        store: &mut Store,
        conn: &ConnId,
        msg_id: AgentMsgId,
        receipt: Option<Vec<u8>>,
    ) -> Result<(), AgentFailure>;
    /// Deletes the connection once what was sent on it is delivered (`deleteConnectionsAsync`
    /// with `waitDelivery`), so an `x.direct.del` sent just before still goes out.
    fn delete(&mut self, store: &mut Store, conn: &ConnId) -> Result<(), AgentFailure>;
    fn subscribe_all(&mut self, store: &mut Store) -> Result<(), AgentFailure>;
    fn on_smp(&mut self, store: &mut Store, event: Self::Smp);
    /// `now_ms`: the engine clock's wall time, the same clock `next_wake` is in.
    fn on_tick(&mut self, store: &mut Store, now_ms: i64);
    /// Wall-clock ms at which `on_tick` next has work; `None` when nothing waits.
    fn next_wake(&self) -> Option<i64>;
    /// Closes the network; what the agent stored stays.
    fn close(&mut self);
    fn drain_events(&mut self) -> Vec<AgentEvent>;

    /// A contact address (`createConnection … SCMContact`); answered by `AgentEvent::Address`.
    fn create_address(
        &mut self,
        store: &mut Store,
        short: bool,
        user_data: Vec<u8>,
    ) -> Result<ConnId, AgentFailure>;

    /// `acceptContact`: a new connection that answers the request, our `x.info` in `conn_info`.
    fn accept_request(
        &mut self,
        store: &mut Store,
        invitation: &[u8],
        conn_info: Vec<u8>,
        pq: bool,
    ) -> Result<ConnId, AgentFailure>;

    /// `rejectContact`: the request is forgotten; nothing is sent.
    fn reject_request(&mut self, store: &mut Store, invitation: &[u8]) -> Result<(), AgentFailure>;

    /// The ratchet's associated data of `conn` (`getConnectionRatchetAdHash` hashes it):
    /// what the security code is made from.
    fn ratchet_ad(&mut self, store: &mut Store, conn: &ConnId) -> Result<Vec<u8>, AgentFailure>;
}
