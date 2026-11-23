//! Which request failed, and what a sent file is (mirrors core's `chat::engine`).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestKind {
    Start,
    SetProfile,
    CreateInvite,
    Connect,
    Answer,
    Address,
    Send,
    MarkRead,
    History,
    Refresh,
    Group,
    RemoveFriend,
    SendFile,
    Edit,
    Delete,
    React,
    Mute,
    Block,
    Code,
    Members,
    File,
    GroupLink,
    Call,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Image,
    Gif,
    Video,
    File,
}
