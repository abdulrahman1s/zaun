//! What the agent tells the engine, named after the agent's ACommand events (Agent/Protocol.hs `ACommand`, AGPL-3.0-only).
use std::sync::Arc;

use super::ids::{AgentMsgId, ConnId};
use crate::agent::error::AgentError;
use crate::agent::integrity::MsgIntegrity;
use crate::smp::SmpServer;

/// `MsgReceiptStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RcptStatus {
    Ok,
    /// The peer received a message whose hash differs from what we sent.
    BadMsgHash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub status: RcptStatus,
    /// Chat's receipt body; empty for an ordinary delivery receipt.
    pub info: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    /// `create_invitation` is done: the link to give the peer.
    Invitation {
        conn: ConnId,
        full: String,
        short: Option<String>,
    },
    /// The joining peer's confirmation: answer with `allow` and our own connection info.
    Conf {
        conn: ConnId,
        conn_info: Vec<u8>,
    },
    /// The inviter's connection info, received by the joining side just before `Con`.
    Info {
        conn: ConnId,
        conn_info: Vec<u8>,
    },
    /// The connection is duplex; `send` may be used.
    Con {
        conn: ConnId,
    },
    /// A chat message; `ack` it when it is processed, or it comes again after a restart.
    Msg {
        conn: ConnId,
        msg_id: AgentMsgId,
        body: Vec<u8>,
        integrity: MsgIntegrity,
        /// Server time of the message, in seconds since the epoch.
        broker_ts: i64,
    },
    /// The server accepted a message from `send`.
    Sent {
        conn: ConnId,
        msg_id: AgentMsgId,
    },
    /// A temporary failure; the message is still being sent (QUOTA).
    Mwarn {
        conn: ConnId,
        msg_id: AgentMsgId,
        err: AgentError,
    },
    /// The message will not be sent.
    Merr {
        conn: ConnId,
        msg_id: AgentMsgId,
        err: AgentError,
    },
    /// The peer confirmed receiving our message `msg_id`.
    Rcvd {
        conn: ConnId,
        msg_id: AgentMsgId,
        receipt: Receipt,
    },
    /// The peer's queue has room again after QUOTA.
    Qcont {
        conn: ConnId,
    },
    Up {
        server: Arc<SmpServer>,
    },
    Down {
        server: Arc<SmpServer>,
    },
    /// Another client subscribed to this connection's queue (`END`).
    End {
        conn: ConnId,
    },
    Err {
        conn: ConnId,
        err: AgentError,
    },
    /// `create_address` is done: the links to give out.
    Address {
        conn: ConnId,
        full: String,
        short: Option<String>,
    },
    /// Someone used the address `conn`: answer with `accept_contact` or `reject_contact` on `inv_id`.
    Req {
        conn: ConnId,
        inv_id: Vec<u8>,
        conn_info: Vec<u8>,
    },
}
