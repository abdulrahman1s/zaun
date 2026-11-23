//! Record keys inside the namespace "chat", and the items table's chat refs.

use zaun_chat_wire::model::ChatRef;

use crate::engine::agent_port::{AgentMsgId, ConnId};

pub const USER: &[u8] = b"user";
pub const SETTINGS: &[u8] = b"settings";
pub const FRIEND_PREFIX: &[u8] = b"friend/";
pub const CONN_PREFIX: &[u8] = b"conn/";
/// The user's address.
pub const ADDRESS: &[u8] = b"address";
pub const REQUEST_PREFIX: &[u8] = b"req/";
pub const GROUP_PREFIX: &[u8] = b"group/";
pub const MEMBER_PREFIX: &[u8] = b"member/";
/// The `seq` counter that numbers friends.
pub const FRIEND_SEQ: &str = "friend";
/// The `seq` counter that numbers requests to the address.
pub const REQUEST_SEQ: &str = "request";
/// The `seq` counter that numbers groups.
pub const GROUP_SEQ: &str = "group";
/// The `seq` counter that numbers members, across all groups.
pub const MEMBER_SEQ: &str = "member";

#[must_use]
pub fn group(id: i64) -> Vec<u8> {
    format!("group/{id}").into_bytes()
}

/// A member of `group`. Eviction: with the group (`member_prefix`).
#[must_use]
pub fn member(group: i64, member: i64) -> Vec<u8> {
    format!("member/{group}/{member}").into_bytes()
}

#[must_use]
pub fn member_prefix(group: i64) -> Vec<u8> {
    format!("member/{group}/").into_bytes()
}

#[must_use]
pub fn friend(id: i64) -> Vec<u8> {
    format!("friend/{id}").into_bytes()
}

#[must_use]
pub fn request(id: i64) -> Vec<u8> {
    format!("req/{id}").into_bytes()
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

/// An item by the shared message id both sides know it by. In a group the id is the author's,
/// so the index names the author too (Store/Messages.hs `getGroupMemberCIBySharedMsgId`):
/// `member` is the author's member number, `None` for the user's own items.
#[must_use]
pub fn shared(chat: ChatRef, member: Option<i64>, msg_id: &[u8]) -> Vec<u8> {
    let mut k = shared_prefix(chat);
    if let ChatRef::Group(_) = chat {
        match member {
            Some(m) => k.extend_from_slice(format!("{m}/").as_bytes()),
            None => k.extend_from_slice(b"me/"),
        }
    }
    k.extend_from_slice(hex(msg_id).as_bytes());
    k
}

#[must_use]
pub fn shared_prefix(chat: ChatRef) -> Vec<u8> {
    match chat {
        ChatRef::Direct(f) => format!("smsg/{f}/").into_bytes(),
        ChatRef::Group(g) => format!("smsg/g{g}/").into_bytes(),
    }
}

#[must_use]
pub fn member_blocked(group: i64, member: i64) -> Vec<u8> {
    format!("mblock/{group}/{member}").into_bytes()
}

/// A file item's record. Eviction: with its friend (`file_prefix`).
#[must_use]
pub fn file(friend: i64, item: i64) -> Vec<u8> {
    format!("file/{friend}/{item}").into_bytes()
}

#[must_use]
pub fn file_prefix(friend: i64) -> Vec<u8> {
    format!("file/{friend}/").into_bytes()
}

/// Every file record, for the start's sweep of transfers a restart cut short.
pub const FILE_PREFIX: &[u8] = b"file/";

/// The items table's chat of a conversation.
#[must_use]
pub fn chat(chat: ChatRef) -> Vec<u8> {
    match chat {
        ChatRef::Direct(f) => format!("d/{f}").into_bytes(),
        ChatRef::Group(g) => format!("g/{g}").into_bytes(),
    }
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

/// The members a group file's message went to, in order, until its descriptions go out
/// (Subscriber.hs `SFDONE`: one each). Eviction: when the upload ends, or with the group.
#[must_use]
pub fn file_members(group: i64, item: i64) -> Vec<u8> {
    format!("gfto/{group}/{item}").into_bytes()
}

#[must_use]
pub fn file_members_prefix(group: i64) -> Vec<u8> {
    format!("gfto/{group}/").into_bytes()
}

/// The emoji one member gave an item of a group (`getGroupReactions`; at most three).
/// Eviction: with the item when it is deleted for good, or with the group.
#[must_use]
pub fn group_reaction(group: i64, item: i64, member: i64) -> Vec<u8> {
    format!("grx/{group}/{item}/{member}").into_bytes()
}

#[must_use]
pub fn group_reaction_item_prefix(group: i64, item: i64) -> Vec<u8> {
    format!("grx/{group}/{item}/").into_bytes()
}

#[must_use]
pub fn group_reaction_prefix(group: i64) -> Vec<u8> {
    format!("grx/{group}/").into_bytes()
}
