//! Stream signalling (ADR-0036).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c", rename_all = "snake_case")]
pub enum StreamRequest {
    Invite(i64),
    Offer { friend: i64, sdp: String },
    Answer { friend: i64, sdp: String },
    Connected(i64),
    End(i64),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c", rename_all = "snake_case")]
pub enum StreamSignal {
    Invited(i64),
    Offer {
        friend: i64,
        sdp: String,
        candidates: Vec<String>,
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
    Ended(i64),
    Refused {
        friend: i64,
        detail: String,
    },
}
