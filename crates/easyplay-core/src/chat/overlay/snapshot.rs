//! What the overlay draws (ADR-0037 §5): a small copy of the Chat snapshot in words of its
//! own, built by the service ([`super::build`]) and sent whole. Bounded, so a line stays small:
//! [`MAX_CHATS`] conversations, [`MAX_MESSAGES`] messages of the open one, [`MAX_TOASTS`]
//! toasts, [`MAX_PEOPLE`] people in the call, and each picture sent once ([`WirePicture`]).
use serde::{Deserialize, Serialize};

use super::notice::Notice;
use crate::model::ChatRef;

/// Recent conversations listed.
pub const MAX_CHATS: usize = 8;
/// Messages of the open conversation.
pub const MAX_MESSAGES: usize = 30;
/// Toasts held (the newest; the overlay shows each once).
pub const MAX_TOASTS: usize = 3;
/// A toast's text is cut here (characters).
pub const TOAST_CHARS: usize = 140;
/// People drawn in the call widget (then "and N more").
pub const MAX_PEOPLE: usize = 8;

/// A conversation on the wire: `{"direct":3}` or `{"group":7}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatKey {
    Direct(i64),
    Group(i64),
}

impl ChatKey {
    pub fn is_group(self) -> bool {
        matches!(self, Self::Group(_))
    }
}

impl From<ChatRef> for ChatKey {
    fn from(c: ChatRef) -> Self {
        match c {
            ChatRef::Direct(id) => Self::Direct(id),
            ChatRef::Group(id) => Self::Group(id),
        }
    }
}

impl From<ChatKey> for ChatRef {
    fn from(c: ChatKey) -> Self {
        match c {
            ChatKey::Direct(id) => Self::Direct(id),
            ChatKey::Group(id) => Self::Group(id),
        }
    }
}

/// The physical corner of the screen the call widget sits in (`chat.overlay_corner`); it
/// never mirrors in Arabic (the game's picture does not).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Corner {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Whether a friend is around.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Online,
    Away,
    Busy,
    #[default]
    Offline,
}

/// The settings and appearance the overlay needs to draw.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayLook {
    /// Show new messages as toasts while closed (`chat.overlay_toasts`).
    pub toasts: bool,
    /// Show the call widget while closed (`chat.overlay_voice_widget`).
    pub voice_widget: bool,
    pub corner: Corner,
    /// The key the user bound, as hints show it (`chat.overlay_key`; empty = none).
    pub key: String,
    /// `general.language` (`system` or a code) and the app's own locale (`LANG`), which
    /// the overlay resolves the way the window does.
    pub language: String,
    pub locale: String,
    /// `appearance.accent`.
    pub accent: String,
    /// `appearance.theme` is high contrast: opaque black plates.
    pub high_contrast: bool,
    pub reduced_motion: bool,
}

/// A profile picture sent once: its key (`model::Picture::key`) and its bytes in base64
/// (a JPEG or PNG the service already checked, at most 32 KiB).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WirePicture {
    pub key: u64,
    pub data: String,
}

/// Everything the overlay draws.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlaySnapshot {
    /// The user's name and avatar (`None` before Chat has started).
    pub me: Option<Person>,
    /// The running game's title (the panel's top line).
    pub game: Option<String>,
    /// Recent conversations, most recent first.
    pub chats: Vec<OverlayChat>,
    /// The conversation shown in the panel.
    pub open: Option<Conversation>,
    pub call: Option<OverlayCall>,
    /// A call ringing for the user.
    pub ringing: Option<OverlayRing>,
    /// The newest message toasts, oldest first; each has an ID so it is shown once.
    pub toasts: Vec<Toast>,
    pub look: OverlayLook,
    /// Pictures the overlay was not sent yet; the others are referenced by key.
    pub pictures: Vec<WirePicture>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Person {
    pub name: String,
    /// The avatar's paper-and-ink pair (0..10; `None` = from the name).
    pub paper: Option<u8>,
    pub picture: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayChat {
    pub chat: ChatKey,
    pub who: Person,
    /// A friend's status (groups have none).
    pub status: Option<Status>,
    pub unread: u32,
    pub muted: bool,
    /// The latest message.
    pub last: Option<OverlayMessage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    pub chat: ChatKey,
    pub who: Person,
    pub status: Option<Status>,
    /// A group's members and how many of them are online.
    pub members: Option<(u32, u32)>,
    /// Oldest first.
    pub messages: Vec<OverlayMessage>,
}

/// What a message is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyKind {
    Text,
    /// Something that happened in the conversation ([`OverlayMessage::notice`]).
    Notice,
    Voice,
    Gif,
    Picture,
    Video,
    File,
    /// A call line ([`OverlayMessage::call`]).
    Call,
    Stream,
    Deleted,
}

/// How a call went.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallLine {
    Ended { minutes: u32 },
    Missed,
    Declined,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayMessage {
    pub id: i64,
    /// The author (a friend, a member, or the user when `me`).
    pub author: Person,
    pub me: bool,
    pub kind: BodyKind,
    /// The text of a text message, a caption, or a file's or game's name.
    pub text: String,
    pub notice: Option<Notice>,
    pub call: Option<CallLine>,
    /// Unix milliseconds.
    pub at: i64,
    /// Not read when it arrived (the first one gets the "New" line).
    pub unread: bool,
    /// Names written after @ that point at a member.
    pub mentions: Vec<String>,
}

/// How a call gets through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Link {
    Ringing,
    Connecting,
    #[default]
    Connected,
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallPerson {
    pub who: Person,
    pub me: bool,
    pub speaking: bool,
    pub muted: bool,
    /// Voice level 0..=100.
    pub level: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayCall {
    pub chat: ChatKey,
    /// The conversation's name ("Voice in …" for groups).
    pub name: String,
    /// Connected since (Unix milliseconds); `None` while ringing out or connecting.
    pub since: Option<i64>,
    pub link: Link,
    /// Everyone in it, the user last; at most [`MAX_PEOPLE`] + 1 (the count of the rest
    /// is [`OverlayCall::more`]).
    pub people: Vec<CallPerson>,
    pub more: u32,
    pub muted: bool,
    pub deafened: bool,
    /// The user shares their game in this call's conversation.
    pub sharing: bool,
    /// Sharing a game is possible in this build.
    pub can_share: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayRing {
    pub chat: ChatKey,
    pub who: Person,
    /// Unix milliseconds.
    pub since: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Toast {
    pub id: u64,
    pub chat: ChatKey,
    pub who: Person,
    /// The group's name, for a member's message.
    pub group: Option<String>,
    pub kind: BodyKind,
    pub text: String,
}
