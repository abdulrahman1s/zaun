//! What a message is.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::bytes;
use super::conn_event::{DecryptError, IntegrityError, SwitchPhase, SyncStatus};

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
    MemberAdded {
        by: Option<String>,
        who: String,
    },
    MemberLeft(String),
    /// A moderator blocked or unblocked a member for everyone (`RGEMemberBlocked`).
    MemberBlocked {
        by: Option<String>,
        who: String,
        blocked: bool,
    },
    MemberRemoved {
        by: Option<String>,
        who: String,
    },
    YouLeft,
    YouRemoved(String),
    GroupDeleted,
    GroupNameChanged(String),
    NameChanged {
        from: String,
        to: String,
    },
    FriendDeleted,
    GroupInvitation {
        group: String,
        sent: bool,
    },
    CodeChanged,
    /// The connection's queue moved (`RCESwitchQueue`, or `SCESwitchQueue` when the item is the
    /// user's). `member`: in a group, who the user's own switch is with ("... for \<name\>").
    AddressSwitch {
        phase: SwitchPhase,
        member: Option<String>,
    },
    /// The connection's ratchet re-sync moved on (`RCERatchetSync`, or `SCERatchetSync` when
    /// the item is the user's). `member` as for `AddressSwitch`.
    RatchetSync {
        status: SyncStatus,
        member: Option<String>,
    },
    BadSignature,
    /// The relay dropped, reordered or replayed messages before this one (`CIRcvIntegrityError`).
    Integrity(IntegrityError),
    /// `count` messages in a row did not decrypt (`CIRcvDecryptionError`); for too many skipped
    /// messages, how many were skipped.
    DecryptionError {
        error: DecryptError,
        count: u32,
    },
    /// A member invited the user to talk directly (`RGEMemberCreatedContact`, "started direct
    /// connection with you"); the invitation waits with the requests.
    DirectInvitation,
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
