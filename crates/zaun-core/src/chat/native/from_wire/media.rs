//! The wire's call and stream signals as core's.

use zaun_chat_wire::model as wm;

use crate::chat::media::{CallKey, CallSignal, StreamSignal};

fn key(k: wm::CallKey) -> CallKey {
    CallKey(k.0)
}

pub(super) fn call(s: wm::CallSignal) -> CallSignal {
    match s {
        wm::CallSignal::Invited {
            friend,
            usable,
            key: k,
        } => CallSignal::Invited {
            friend,
            usable,
            key: k.map(key),
        },
        wm::CallSignal::Offer {
            friend,
            sdp,
            candidates,
            key: k,
        } => CallSignal::Offer {
            friend,
            sdp,
            candidates,
            key: k.map(key),
        },
        wm::CallSignal::Answer {
            friend,
            sdp,
            candidates,
        } => CallSignal::Answer {
            friend,
            sdp,
            candidates,
        },
        wm::CallSignal::Extra { friend, candidates } => CallSignal::Extra { friend, candidates },
        wm::CallSignal::Unusable(f) => CallSignal::Unusable(f),
        wm::CallSignal::Ended(f) => CallSignal::Ended(f),
        wm::CallSignal::Declined(f) => CallSignal::Declined(f),
        wm::CallSignal::Refused { friend, detail } => CallSignal::Refused { friend, detail },
    }
}

pub(super) fn stream(s: wm::StreamSignal) -> StreamSignal {
    match s {
        wm::StreamSignal::Invited(f) => StreamSignal::Invited(f),
        wm::StreamSignal::Offer {
            friend,
            sdp,
            candidates,
        } => StreamSignal::Offer {
            friend,
            sdp,
            candidates,
        },
        wm::StreamSignal::Answer {
            friend,
            sdp,
            candidates,
        } => StreamSignal::Answer {
            friend,
            sdp,
            candidates,
        },
        wm::StreamSignal::Extra { friend, candidates } => {
            StreamSignal::Extra { friend, candidates }
        }
        wm::StreamSignal::Ended(f) => StreamSignal::Ended(f),
        wm::StreamSignal::Refused { friend, detail } => StreamSignal::Refused { friend, detail },
    }
}
