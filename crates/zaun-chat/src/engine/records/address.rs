//! The user's address and the requests that came through it (Types.hs `UserContactLink`, `UserContactRequest`).

use serde::{Deserialize, Serialize};

use crate::engine::agent_port::ConnId;

/// Requests accepted, newest last, so a person who asks again is not listed twice while they are a friend.
/// Eviction: one leaves with its friend, and the oldest past `MAX_ACCEPTED`.
pub const MAX_ACCEPTED: usize = 256;

/// A request that became a friend: its `contactReqId` (hex) and the friend.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Accepted {
    pub req_id: String,
    pub friend: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddressRecord {
    pub conn: ConnId,
    pub full: String,
    pub short: Option<String>,
    #[serde(default)]
    pub accepted: Vec<Accepted>,
}

impl AddressRecord {
    /// The short link when the relay keeps one: a QR code that scans at the page's size.
    #[must_use]
    pub fn shared(&self) -> String {
        self.short.clone().unwrap_or_else(|| self.full.clone())
    }

    /// `live` says which friends still exist: a contact that was deleted no longer counts (`ct.deleted = 0`).
    pub fn accept(&mut self, req_id: String, friend: i64, live: impl Fn(i64) -> bool) {
        self.accepted.retain(|a| live(a.friend));
        self.accepted.push(Accepted { req_id, friend });
        let excess = self.accepted.len().saturating_sub(MAX_ACCEPTED);
        self.accepted.drain(..excess);
    }

    #[must_use]
    pub fn accepted_friend(&self, req_id: &str) -> Option<i64> {
        self.accepted
            .iter()
            .find(|a| a.req_id == req_id)
            .map(|a| a.friend)
    }
}

/// Someone asked to connect: their profile as sent in `x.contact`, and the agent's name for the request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestRecord {
    pub id: i64,
    pub invitation: Vec<u8>,
    pub name: String,
    pub about: String,
    pub image: Option<String>,
    /// `contactReqId`, hex: the same person asking again is one request.
    pub req_id: Option<String>,
    /// Milliseconds since the epoch.
    pub since: i64,
}
