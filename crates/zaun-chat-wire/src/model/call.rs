//! Call signalling (ADR-0035): SDP and `candidate:…` lines, the codec's business.

use serde::{Deserialize, Serialize};

/// The 256-bit key of a call's frame encryption.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallKey(pub [u8; 32]);

impl std::fmt::Debug for CallKey {
    // Never the key: logs print signals.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CallKey(..)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c", rename_all = "snake_case")]
pub enum CallRequest {
    Invite(i64),
    Offer {
        friend: i64,
        sdp: String,
    },
    Answer {
        friend: i64,
        sdp: String,
    },
    Extra {
        friend: i64,
        candidates: Vec<String>,
    },
    Connected(i64),
    Reject(i64),
    Decline(i64),
    End(i64),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c", rename_all = "snake_case")]
pub enum CallSignal {
    Invited {
        friend: i64,
        usable: bool,
        key: Option<CallKey>,
    },
    Offer {
        friend: i64,
        sdp: String,
        candidates: Vec<String>,
        key: Option<CallKey>,
    },
    Answer {
        friend: i64,
        sdp: String,
        candidates: Vec<String>,
    },
    Extra {
        friend: i64,
        candidates: Vec<String>,
    },
    Unusable(i64),
    Ended(i64),
    Declined(i64),
    Refused {
        friend: i64,
        detail: String,
    },
}
