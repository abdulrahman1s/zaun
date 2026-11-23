//! The user's address (Phase 3): making and deleting it, the requests that come through it, accepting and
//! rejecting them (Commands.hs `APICreateMyAddress`, `APIDeleteMyAddress`, `APIAcceptContact`,
//! `APIRejectContact`; Subscriber.hs `processContactConnMessage` REQ).

mod answer;
mod create;
mod delete;
mod info;
mod joined;
mod load;
mod request;

use std::collections::BTreeMap;

pub(crate) use info::contact_conn_info;

use crate::engine::agent_port::ConnId;
use crate::engine::records::{AddressRecord, RequestRecord};

/// Requests waiting for an answer at most; the oldest is rejected to make room for a new one.
pub const MAX_REQUESTS: usize = 50;

/// The address and the requests waiting for an answer.
#[derive(Debug, Default)]
pub struct AddressState {
    pub record: Option<AddressRecord>,
    /// The address being made: one at a time.
    pub creating: Option<ConnId>,
    /// Eviction: an answer removes one; `MAX_REQUESTS` rejects the oldest; deleting the address clears them.
    pub requests: BTreeMap<i64, RequestRecord>,
}
