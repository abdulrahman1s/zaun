//! What a request carries: the requester's `x.contact` (or a legacy `x.info`), and the one we send to an address (Protocol.hs `XContact`).

use crate::agent::short_link::{ContactShortLinkData, encode_short_link_data};
use crate::chat::compress::compress_to_limit;
use crate::chat::ids::XContactId;
use crate::chat::limits::MAX_CONN_INFO_LENGTH_PQ;
use crate::chat::profile::Profile;
use crate::chat::{ChatEvent, ChatMessage, Result, parse_chat_messages};
use crate::engine::error::{Code, fail};
use crate::engine::profile::sent_profile;
use crate::engine::records::UserRecord;

/// The requester's profile and their `contactReqId`; `None` for anything chat does not take as a request.
pub(super) fn parse_request(conn_info: &[u8]) -> Option<(Profile, Option<Vec<u8>>)> {
    let msgs = parse_chat_messages(conn_info).ok()?;
    msgs.into_iter().find_map(|m| {
        if let ChatEvent::Contact {
            profile,
            contact_req_id,
            ..
        } = m.event
        {
            Some((profile, contact_req_id.map(|id| id.0)))
        } else if let ChatEvent::Info { profile, .. } = m.event {
            Some((profile, None))
        } else {
            None
        }
    })
}

/// The conn-info joining an address: `x.contact` with our profile and the id that makes repeats one request.
pub(crate) fn contact_conn_info(user: &UserRecord, req_id: Vec<u8>) -> Result<Vec<u8>> {
    let msg = ChatMessage::without_id(ChatEvent::Contact {
        profile: sent_profile(user),
        member_key: None,
        contact_req_id: Some(XContactId(req_id)),
        welcome_msg_id: None,
        request_msg: None,
    });
    compress_to_limit(&msg.to_json()?, MAX_CONN_INFO_LENGTH_PQ)
}

pub(super) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}"); // writing to a String cannot fail
        s
    })
}

/// The most a short link's user data may hold (13784 bytes of link data).
const MAX_LINK_DATA: usize = 13784;

/// `{"profile":…,"business":false}` as the address shows it; a picture that would not fit is left out, the name stays.
pub(super) fn link_data(user: &UserRecord) -> crate::engine::error::Result<Vec<u8>> {
    let mut profile = sent_profile(user);
    let mut data = encode_short_link_data(&ContactShortLinkData::new(profile.clone()));
    if data.as_ref().map_or(true, |d| d.0.len() > MAX_LINK_DATA) && profile.image.take().is_some() {
        data = encode_short_link_data(&ContactShortLinkData::new(profile));
    }
    data.map(|d| d.0)
        .map_err(|e| fail(Code::RequestFailed, e.to_string()))
}
