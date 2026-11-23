//! What the join tests share: links that parse, conn-info as a peer sends it, and what the engine sent.

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{ChatNotice, MessageBody};

use crate::chat::group::{GroupMemberRole, MemberInfo};
use crate::chat::ids::MemberId;
use crate::chat::profile::Profile;
use crate::chat::version::SUPPORTED_CHAT_VRANGE;
use crate::chat::{ChatEvent, ChatMessage, parse_chat_elements};
use crate::engine::agent_port::ConnId;
use crate::engine::tests::{Call, T, messages};

const SMP: &str = "smp%3A%2F%2FAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE%3D%40fake.test%2F-FWjHkx7I4TOSuF3q4l2S4jD2BgprZR";
const SMP_TAIL: &str =
    "%23%2F%3Fv%3D1-4%26dh%3DMCowBQYDK2VuAyEAQWIOFJ7ZTMqTFfTsHuyRYNDwszkpHQfRxObZXAWAIRU%253D";
const E2E: &str = "v%3D3%26x3dh%3DMEIwBQYDK2VvAzkAmKuSYeQ_m0SixPDS8Wq8VBaTS1cW-Lp0n0h4Diu-kUpR-qXx4SDJ32YGEFoGFGSbGPry5Ychr6U%3D%2CMEIwBQYDK2VvAzkAmKuSYeQ_m0SixPDS8Wq8VBaTS1cW-Lp0n0h4Diu-kUpR-qXx4SDJ32YGEFoGFGSbGPry5Ychr6U%3D";

/// A one-time invitation link that parses, the `n`th of several distinct ones.
pub fn link(n: u8) -> String {
    let last = char::from(b'A' + n % 26);
    format!("simplex:/invitation#/?v=6-8&smp={SMP}{last}{SMP_TAIL}&e2e={E2E}")
}

pub fn info(name: &str, role: GroupMemberRole) -> MemberInfo {
    MemberInfo {
        member_id: MemberId::mint().unwrap(),
        member_role: role,
        v: Some(SUPPORTED_CHAT_VRANGE),
        profile: Profile::new(name, ""),
        member_key: None,
    }
}

/// Conn-info as a 7.1.4 peer sends it: no message id.
pub fn conn_info(event: ChatEvent) -> Vec<u8> {
    ChatMessage::without_id(event).to_json().unwrap()
}

pub fn events(body: &[u8]) -> Vec<ChatEvent> {
    parse_chat_elements(body)
        .into_iter()
        .map(|p| p.unwrap().message.event)
        .collect()
}

/// Every event sent on `conn`, batches opened.
pub fn sent_on(t: &T, conn: ConnId) -> Vec<ChatEvent> {
    t.e.agent
        .sent()
        .into_iter()
        .filter(|(c, _)| *c == conn)
        .flat_map(|(_, b)| events(&b))
        .collect()
}

/// What the user allowed group connections with (friends' handshakes left out).
pub fn allowed(t: &T) -> Vec<(ConnId, ChatEvent)> {
    t.e.agent
        .calls
        .iter()
        .filter_map(|c| match c {
            Call::Allow { conn, conn_info } if t.e.state.member_of(conn).is_some() => {
                Some((*conn, events(conn_info).remove(0)))
            }
            _ => None,
        })
        .collect()
}

pub fn joined(t: &T) -> Vec<(String, ChatEvent)> {
    t.e.agent
        .calls
        .iter()
        .filter_map(|c| match c {
            Call::Join { link, conn_info } => Some((link.clone(), events(conn_info).remove(0))),
            _ => None,
        })
        .collect()
}

pub fn invitations_made(t: &T) -> usize {
    t.e.agent
        .calls
        .iter()
        .filter(|c| matches!(c, Call::CreateInvitation))
        .count()
}

pub fn notices(out: &[Event]) -> Vec<ChatNotice> {
    messages(out)
        .into_iter()
        .filter_map(|m| match m.body {
            MessageBody::Notice(n) => Some(n),
            _ => None,
        })
        .collect()
}

/// The connection the last `create_invitation` or `join` got from the fake agent.
pub fn last_conn(t: &T) -> ConnId {
    t.e.state
        .conns
        .keys()
        .filter(|c| c.0 < 1_000_000)
        .max()
        .copied()
        .unwrap()
}

/// The one element of `v`.
pub fn only<T: std::fmt::Debug>(v: Vec<T>) -> T {
    assert_eq!(v.len(), 1, "{v:?}");
    v.into_iter().next().unwrap()
}
