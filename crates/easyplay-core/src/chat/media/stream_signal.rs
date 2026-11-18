//! Stream signalling through the chat engine (ADR-0036). A stream travels as a video call
//! (`/_call invite … {"media":"video"}`) that the friend's app takes at once in watch-only
//! mode; the engine keeps one call per friend, so the codec tells streams and voice calls
//! apart and routes each to its own flow.

/// A stream request for the engine (`chat::Request::Stream`). `friend` is a contact number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamRequest {
    /// Offer the friend the user's stream.
    Invite(i64),
    /// Take the friend's stream with our offer.
    Offer { friend: i64, sdp: String },
    /// Answer the friend's offer (the user shares).
    Answer { friend: i64, sdp: String },
    /// The stream's connection is up.
    Connected(i64),
    /// Stop sharing, or let the friend's stream go.
    End(i64),
}

/// What the engine reports about streams (`chat::ChatEvent::Stream`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamSignal {
    /// The friend shares a game with the user.
    Invited(i64),
    /// The friend took the user's stream with this offer.
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
    /// The stream with the friend is over (they stopped, or a voice call with them took its
    /// place in the engine).
    Ended(i64),
    /// The engine refused a stream request.
    Refused { friend: i64, detail: String },
}

impl StreamSignal {
    /// The friend it is about.
    pub fn friend(&self) -> i64 {
        match self {
            Self::Offer { friend, .. }
            | Self::Answer { friend, .. }
            | Self::Extra { friend, .. }
            | Self::Refused { friend, .. } => *friend,
            Self::Invited(friend) | Self::Ended(friend) => *friend,
        }
    }
}
