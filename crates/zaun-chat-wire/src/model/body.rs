//! What a message is.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::bytes;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c", rename_all = "snake_case")]
pub enum MessageBody {
    Text(String),
    Notice(ChatNotice),
    Voice { secs: u32, wave: Vec<u8> },
    Gif(String),
    Image(Media),
    Video(Media),
    File(Media),
    Call(CallRecord),
    Stream(StreamRecord),
    Deleted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c", rename_all = "snake_case")]
pub enum ChatNotice {
    Connected,
    GroupCreated,
    YouJoined,
    MemberJoined(String),
    MemberAdded { by: Option<String>, who: String },
    MemberLeft(String),
    MemberRemoved { by: Option<String>, who: String },
    YouLeft,
    YouRemoved(String),
    GroupDeleted,
    GroupNameChanged(String),
    NameChanged { from: String, to: String },
    FriendDeleted,
    GroupInvitation { group: String, sent: bool },
    CodeChanged,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Media {
    pub name: String,
    pub size: u64,
    #[serde(with = "bytes::opt", default)]
    pub preview: Option<Vec<u8>>,
    pub duration: Option<u32>,
    pub caption: String,
    pub local: Option<PathBuf>,
    pub state: FileState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c", rename_all = "snake_case")]
pub enum FileState {
    Offered,
    Receiving { percent: u8 },
    Received(PathBuf),
    Sending { percent: u8 },
    Sent,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallKind {
    Ended,
    Missed,
    Declined,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallRecord {
    pub kind: CallKind,
    pub minutes: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamRecord {
    pub title: String,
    pub live: bool,
    pub watching: u32,
    pub minutes: u32,
}
