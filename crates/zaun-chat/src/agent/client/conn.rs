//! A loaded connection: its record and ratchet, and what is in flight for it; only the record and ratchet are stored.
use std::collections::VecDeque;

use crate::ratchet::Ratchet;

use super::ids::AgentMsgId;
use crate::agent::delivery::Retry;
use crate::agent::records::ConnRecord;

#[derive(Debug, Default)]
pub(crate) struct SendState {
    pub in_flight: bool,
    pub retry: Option<Retry>,
    /// Wall-clock ms before which the head is not tried again.
    pub next_at: i64,
}

#[derive(Debug, Default)]
pub(crate) struct StepState {
    /// The NEW, LKEY or SKEY of the current status is waiting for its answer.
    pub in_flight: bool,
    pub retry_at: i64,
}

#[derive(Debug, Default)]
pub(crate) struct SubState {
    /// Subscribed on the current session of the receive queue's server.
    pub subscribed: bool,
    pub in_flight: bool,
}

pub(crate) struct Conn {
    pub rec: ConnRecord,
    pub ratchet: Option<Ratchet>,
    /// Ids of the stored pending sends, oldest first. Eviction: an id leaves when its message is sent, dropped or expired.
    pub sends: VecDeque<AgentMsgId>,
    pub send: SendState,
    pub step: StepState,
    pub sub: SubState,
}

impl Conn {
    pub(crate) fn new(rec: ConnRecord, ratchet: Option<Ratchet>) -> Self {
        Self {
            rec,
            ratchet,
            sends: VecDeque::new(),
            send: SendState::default(),
            step: StepState::default(),
            sub: SubState::default(),
        }
    }
}
