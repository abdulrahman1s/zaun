//! The call in progress with each friend (Call.hs `Call`, `CallState`); nothing of it survives a restart.

use std::collections::BTreeMap;

use zaun_chat_wire::model::CallKey;

use crate::crypto::{PrivateKey, PublicKey};

/// `callInvitationTTL`: a call not connected by then is let go (Chat.hs).
pub const RING_TTL_MS: i64 = 180_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Voice,
    /// A shared game: video without the apps' frame encryption (ADR-0036); it has no line.
    Stream,
}

/// Call.hs `CallState`, by tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    InviteSent,
    InviteReceived,
    OfferSent,
    OfferReceived,
    Negotiated,
}

#[derive(Debug)]
pub struct Call {
    /// The 16 raw bytes of `callId`.
    pub id: Vec<u8>,
    pub kind: Kind,
    pub phase: Phase,
    /// The user placed it.
    pub outgoing: bool,
    pub version: u16,
    /// The caller's key to agree the media key with the callee's.
    pub private: Option<PrivateKey>,
    /// The callee's own key, sent in its offer.
    pub public: Option<PublicKey>,
    pub key: Option<CallKey>,
    pub started: i64,
    pub connected: Option<i64>,
    /// Wall ms at which it is let go unless connected by then.
    pub deadline: Option<i64>,
}

/// One call per friend. Eviction: it ends, is replaced by the next, or expires (`RING_TTL_MS`).
#[derive(Debug, Default)]
pub struct CallState {
    pub calls: BTreeMap<i64, Call>,
}

impl CallState {
    /// The earliest wall ms at which a call expires.
    #[must_use]
    pub fn next_deadline(&self) -> Option<i64> {
        self.calls.values().filter_map(|c| c.deadline).min()
    }

    /// The friends whose calls expire by `now`.
    #[must_use]
    pub fn expired(&self, now: i64) -> Vec<i64> {
        self.calls
            .iter()
            .filter(|(_, c)| c.deadline.is_some_and(|d| d <= now))
            .map(|(f, _)| *f)
            .collect()
    }
}
