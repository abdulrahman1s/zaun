//! Record keys inside the namespace "chat", and the items table's chat refs.

use crate::engine::agent_port::{AgentMsgId, ConnId};

pub const USER: &[u8] = b"user";
pub const SETTINGS: &[u8] = b"settings";
pub const FRIEND_PREFIX: &[u8] = b"friend/";
pub const CONN_PREFIX: &[u8] = b"conn/";
/// The `seq` counter that numbers friends.
pub const FRIEND_SEQ: &str = "friend";

#[must_use]
pub fn friend(id: i64) -> Vec<u8> {
    format!("friend/{id}").into_bytes()
}

#[must_use]
pub fn conn(conn: &ConnId) -> Vec<u8> {
    format!("conn/{}", conn.0).into_bytes()
}

/// A sent message by its agent id, until its receipt or failure.
#[must_use]
pub fn sent(conn: &ConnId, msg: AgentMsgId) -> Vec<u8> {
    format!("snd/{}/{msg}", conn.0).into_bytes()
}

#[must_use]
pub fn sent_prefix(conn: &ConnId) -> Vec<u8> {
    format!("snd/{}/", conn.0).into_bytes()
}

/// An item by the shared message id both sides know it by.
#[must_use]
pub fn shared(friend: i64, msg_id: &[u8]) -> Vec<u8> {
    format!("smsg/{friend}/{}", hex(msg_id)).into_bytes()
}

#[must_use]
pub fn shared_prefix(friend: i64) -> Vec<u8> {
    format!("smsg/{friend}/").into_bytes()
}

#[must_use]
pub fn member_blocked(group: i64, member: i64) -> Vec<u8> {
    format!("mblock/{group}/{member}").into_bytes()
}

/// The items table's chat of a direct conversation.
#[must_use]
pub fn direct_chat(friend: i64) -> Vec<u8> {
    format!("d/{friend}").into_bytes()
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}"); // writing to a String cannot fail
            s
        })
}
