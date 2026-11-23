//! What the helper reports (mirrors core's `chat::engine::ChatEvent`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::kind::{FileKind, RequestKind};
use crate::model::{
    CallSignal, ChatFeatures, ChatGroup, ChatMessage, ChatPreview, ChatProfile, ChatRef, Friend,
    FriendRequest, GroupMember, StreamSignal,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c", rename_all = "snake_case")]
pub enum Event {
    Features {
        features: ChatFeatures,
        /// The reactions it carries; `None` = any emoji.
        reactions: Option<Vec<String>>,
    },
    Ready(ChatProfile),
    Profile(ChatProfile),
    Chats {
        friends: Vec<Friend>,
        groups: Vec<ChatGroup>,
        requests: Vec<FriendRequest>,
        chats: Vec<ChatPreview>,
    },
    Invite(String),
    Address(Option<String>),
    Messages(Vec<ChatMessage>),
    Updated(Vec<ChatMessage>),
    History {
        tag: u64,
        chat: ChatRef,
        messages: Vec<ChatMessage>,
    },
    Code {
        friend: i64,
        code: String,
    },
    Verified {
        friend: i64,
        matched: bool,
    },
    Members {
        group: i64,
        members: Vec<GroupMember>,
    },
    Offline(bool),
    Preview {
        chat: ChatRef,
        last: Box<ChatMessage>,
    },
    OpenFile {
        path: PathBuf,
        kind: FileKind,
    },
    Located {
        tag: u64,
        path: Option<PathBuf>,
    },
    GroupLink {
        group: i64,
        link: Option<String>,
    },
    Call(CallSignal),
    Stream(StreamSignal),
    Failed {
        request: RequestKind,
        /// The stable string of core's `ErrorCode`, e.g. "ZN-CHT-005".
        code: String,
        /// The engine's own words, for the log.
        detail: String,
    },
    Exited {
        status: Option<i32>,
        started: bool,
    },
}
