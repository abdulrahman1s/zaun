//! Call signalling through the chat engine (ADR-0035), in protocol-neutral words: what the
//! call flow asks the engine to send, and what arrives from the friend. The session strings
//! are plain SDP; candidates are `candidate:…` lines.

/// A call request for the engine (`chat::Request::Call`). `friend` is a contact number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallRequest {
    /// Ring the friend (a voice call).
    Invite(i64),
    /// Accept the friend's call with our offer.
    Offer { friend: i64, sdp: String },
    /// Answer the friend's offer (the user placed the call).
    Answer { friend: i64, sdp: String },
    /// Candidates found after the description.
    Extra {
        friend: i64,
        candidates: Vec<String>,
    },
    /// The media connected: the call's line in the conversation starts counting.
    Connected(i64),
    /// Let the ringing call go without telling the caller (a friend just blocked): on this
    /// computer only; the caller stops at its own timeout.
    Reject(i64),
    /// Decline the ringing call: it ends at once for the caller too, and both lines read
    /// "Declined".
    Decline(i64),
    /// Hang up, cancel ringing out, or let a ringing call go as missed.
    End(i64),
}

/// What the engine reports about calls (`chat::ChatEvent::Call`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallSignal {
    /// The friend calls.
    Invited {
        friend: i64,
        /// A voice call this app can take part in. `false` for a call from another app
        /// (one that encrypts its media itself, or a video call): it is let go as missed.
        usable: bool,
    },
    /// The friend accepted the user's call with this offer.
    Offer {
        friend: i64,
        sdp: String,
        candidates: Vec<String>,
    },
    /// The friend answered the user's offer.
    Answer {
        friend: i64,
        sdp: String,
        candidates: Vec<String>,
    },
    /// More of the friend's candidates.
    Extra {
        friend: i64,
        candidates: Vec<String>,
    },
    /// An offer or answer this app cannot use (not plain SDP: another app's).
    Unusable(i64),
    /// The friend hung up (or stopped calling).
    Ended(i64),
    /// The friend declined the user's call while it rang.
    Declined(i64),
    /// The engine refused to send the user's invitation, offer or answer (the friend does
    /// not allow calls, the call is already gone).
    Refused { friend: i64, detail: String },
}

impl CallSignal {
    /// The friend it is about.
    pub fn friend(&self) -> i64 {
        match self {
            Self::Invited { friend, .. }
            | Self::Offer { friend, .. }
            | Self::Answer { friend, .. }
            | Self::Extra { friend, .. }
            | Self::Refused { friend, .. } => *friend,
            Self::Unusable(friend) | Self::Ended(friend) | Self::Declined(friend) => *friend,
        }
    }
}
