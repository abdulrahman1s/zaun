//! Call and stream requests as the wire's.

use zaun_chat_wire::model as wm;

use crate::chat::media::{CallRequest, StreamRequest};

pub(super) fn call(r: CallRequest) -> wm::CallRequest {
    match r {
        CallRequest::Invite(f) => wm::CallRequest::Invite(f),
        CallRequest::Offer { friend, sdp } => wm::CallRequest::Offer { friend, sdp },
        CallRequest::Answer { friend, sdp } => wm::CallRequest::Answer { friend, sdp },
        CallRequest::Extra { friend, candidates } => wm::CallRequest::Extra { friend, candidates },
        CallRequest::Connected(f) => wm::CallRequest::Connected(f),
        CallRequest::Reject(f) => wm::CallRequest::Reject(f),
        CallRequest::Decline(f) => wm::CallRequest::Decline(f),
        CallRequest::End(f) => wm::CallRequest::End(f),
    }
}

pub(super) fn stream(r: StreamRequest) -> wm::StreamRequest {
    match r {
        StreamRequest::Invite(f) => wm::StreamRequest::Invite(f),
        StreamRequest::Offer { friend, sdp } => wm::StreamRequest::Offer { friend, sdp },
        StreamRequest::Answer { friend, sdp } => wm::StreamRequest::Answer { friend, sdp },
        StreamRequest::Connected(f) => wm::StreamRequest::Connected(f),
        StreamRequest::End(f) => wm::StreamRequest::End(f),
    }
}
