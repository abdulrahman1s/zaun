//! The seam between the service and a chat protocol (ADR-0033): requests in the terms of
//! the Chat page, events back in the types of `model::chat`. The SimpleX engine is the
//! one implementation; tests use [`super::fake::FakeEngine`].

use std::path::PathBuf;
use std::sync::Arc;

use crate::error::{ErrorCode, Result};
use crate::model::{
    ChatFeatures, ChatGroup, ChatMessage, ChatPreview, ChatProfile, ChatRef, Friend, FriendRequest,
    GroupMember, Picture,
};

/// What the service asks the engine to do. Every answer comes back as [`ChatEvent`]s.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Change the user's name and the line about them (the avatar pair stays on this
    /// computer: the service keeps it; the picture is kept: [`Request::SetPicture`]).
    SetProfile(ChatProfile),
    /// Change the user's picture, keeping the rest of the profile (`None` = remove it;
    /// answered as [`ChatEvent::Profile`], failures as [`RequestKind::SetProfile`]).
    SetPicture(Option<Picture>),
    /// Change a group's picture, keeping the rest of its profile (owners only; `None` =
    /// remove it; failures as [`RequestKind::Group`]).
    SetGroupPicture {
        group: i64,
        picture: Option<Picture>,
    },
    /// Make a one-time invitation link ([`ChatEvent::Invite`]).
    CreateInvite,
    /// Use a friend's link (an invitation or an address).
    Connect(String),
    /// Accept a request made through the user's address.
    Accept(i64),
    /// Decline it.
    Reject(i64),
    /// Make the user's reusable address ([`ChatEvent::Address`]).
    CreateAddress,
    /// Delete it; links already shared stop working.
    DeleteAddress,
    /// Send text to a conversation.
    Send {
        chat: ChatRef,
        text: String,
        reply_to: Option<i64>,
        /// Members named with @ (name as written, member number); groups only.
        mentions: Vec<(String, i64)>,
    },
    /// Send a file (inside the engine's data folder).
    SendFile {
        chat: ChatRef,
        /// The file (inside `EngineConfig::dir`).
        path: PathBuf,
        kind: FileKind,
        /// A small preview picture (JPEG or PNG bytes) friends see before fetching it.
        preview: Option<Vec<u8>>,
        /// A video's length in seconds.
        duration: Option<u32>,
        /// A caption (empty = none).
        text: String,
    },
    /// Fetch a file a friend offered.
    ReceiveFile { chat: ChatRef, item: i64 },
    /// Stop sending or fetching a file.
    CancelFile { chat: ChatRef, item: i64 },
    /// Where a message's file is on this computer ([`ChatEvent::OpenFile`]).
    OpenFile { chat: ChatRef, item: i64 },
    /// Where a message's file is, for a copy ([`ChatEvent::Located`]); never fails.
    LocateFile { chat: ChatRef, item: i64, tag: u64 },
    /// Make a group's invite link ([`ChatEvent::GroupLink`]).
    CreateGroupLink(i64),
    /// Delete it.
    DeleteGroupLink(i64),
    /// Change the text of one of the user's messages.
    Edit {
        chat: ChatRef,
        item: i64,
        text: String,
    },
    /// Delete one of the user's messages for everyone.
    Delete { chat: ChatRef, item: i64 },
    /// Add or take back a reaction.
    React {
        chat: ChatRef,
        item: i64,
        /// The emoji (display form).
        emoji: String,
        /// Add (or take back).
        on: bool,
    },
    /// Mute or unmute a conversation (a muted group still notifies mentions of the user).
    SetMuted { chat: ChatRef, on: bool },
    /// Block or unblock a friend, without telling them: their conversation leaves the list
    /// and what they send while blocked is deleted on arrival.
    SetBlocked { friend: i64, on: bool },
    /// Hide or show a group member's messages (on this computer only).
    SetMemberBlocked { group: i64, member: i64, on: bool },
    /// Read a friend's security code ([`ChatEvent::Code`]).
    ShowCode(i64),
    /// Compare a friend's security code with the one read out ([`ChatEvent::Verified`]).
    Verify { friend: i64, code: String },
    /// Read a group's members ([`ChatEvent::Members`]).
    Members(i64),
    /// Mark everything in a conversation read.
    MarkRead(ChatRef),
    /// Messages of a conversation, newest last ([`ChatEvent::History`]).
    History {
        chat: ChatRef,
        /// Only messages older than this item number (`None` = the latest).
        before: Option<i64>,
        count: u32,
        tag: u64,
    },
    /// Read the conversation list again ([`ChatEvent::Chats`]).
    Refresh,
    /// Make a group with the user as owner.
    NewGroup(String),
    /// Invite a friend into a group.
    AddMember { group: i64, friend: i64 },
    /// Join a group the user was invited to.
    JoinGroup(i64),
    /// Leave a group.
    LeaveGroup(i64),
    /// Remove a friend and the conversation with them.
    RemoveFriend(i64),
    /// Call signalling (ADR-0035).
    Call(super::media::CallRequest),
    /// Stream signalling (ADR-0036).
    Stream(super::media::StreamRequest),
}

/// Which request failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RequestKind {
    Start,
    /// [`Request::SetProfile`].
    SetProfile,
    /// [`Request::CreateInvite`].
    CreateInvite,
    /// [`Request::Connect`].
    Connect,
    /// [`Request::Accept`] / [`Request::Reject`].
    Answer,
    /// [`Request::CreateAddress`] / [`Request::DeleteAddress`].
    Address,
    /// [`Request::Send`].
    Send,
    /// [`Request::MarkRead`].
    MarkRead,
    /// [`Request::History`].
    History,
    /// [`Request::Refresh`].
    Refresh,
    Group,
    /// [`Request::RemoveFriend`].
    RemoveFriend,
    /// [`Request::SendFile`].
    SendFile,
    /// [`Request::Edit`].
    Edit,
    /// [`Request::Delete`].
    Delete,
    /// [`Request::React`].
    React,
    /// [`Request::SetMuted`].
    Mute,
    /// [`Request::SetBlocked`] / [`Request::SetMemberBlocked`].
    Block,
    /// [`Request::ShowCode`] / [`Request::Verify`].
    Code,
    /// [`Request::Members`].
    Members,
    /// [`Request::ReceiveFile`] / [`Request::CancelFile`] / [`Request::OpenFile`].
    File,
    /// [`Request::CreateGroupLink`] / [`Request::DeleteGroupLink`].
    GroupLink,
    /// [`Request::Call`].
    Call,
}

/// What a sent file is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Image,
    Gif,
    Video,
    File,
}

/// What the engine reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatEvent {
    /// What this engine carries (reported before [`ChatEvent::Ready`]).
    Features {
        features: ChatFeatures,
        /// The reactions it carries (display form); `None` = any emoji.
        reactions: Option<Vec<String>>,
    },
    /// Started: the database is open and the relays are being reached.
    Ready(ChatProfile),
    /// The user's profile changed.
    Profile(ChatProfile),
    /// The whole conversation list, read again.
    Chats {
        friends: Vec<Friend>,
        groups: Vec<ChatGroup>,
        requests: Vec<FriendRequest>,
        /// Conversations, most recent first.
        chats: Vec<ChatPreview>,
    },
    /// A one-time invitation link to share.
    Invite(String),
    /// The user's address (`None` = deleted or never made).
    Address(Option<String>),
    /// New messages (received, or sent by the user), oldest first.
    Messages(Vec<ChatMessage>),
    /// Messages whose delivery or text changed.
    Updated(Vec<ChatMessage>),
    /// The answer to [`Request::History`].
    History {
        tag: u64,
        chat: ChatRef,
        messages: Vec<ChatMessage>,
    },
    /// A friend's security code, after [`Request::ShowCode`].
    Code {
        friend: i64,
        /// The code (groups of five digits).
        code: String,
    },
    /// The answer to [`Request::Verify`].
    Verified {
        friend: i64,
        /// The codes matched (the friend is now verified).
        matched: bool,
    },
    /// A group's members, the user included.
    Members {
        group: i64,
        members: Vec<GroupMember>,
    },
    /// The relays cannot (true) or can again (false) be reached.
    Offline(bool),
    /// The latest message the page shows of a chat whose latest item is not shown.
    Preview {
        chat: ChatRef,
        last: Box<ChatMessage>,
    },
    /// The answer to [`Request::OpenFile`]: the file, inside the engine's data folder.
    OpenFile { path: PathBuf, kind: FileKind },
    /// The answer to [`Request::LocateFile`].
    Located {
        tag: u64,
        /// The file, inside the engine's data folder (`None` = not on this computer).
        path: Option<PathBuf>,
    },
    /// A group's invite link (`None` = none, or deleted).
    GroupLink {
        group: i64,
        /// The link (short form when the relay offers one).
        link: Option<String>,
    },
    /// A call signal from a friend (ADR-0035).
    Call(super::media::CallSignal),
    /// A stream signal from a friend (ADR-0036).
    Stream(super::media::StreamSignal),
    /// A request failed.
    Failed {
        request: RequestKind,
        /// Why, for the user.
        code: ErrorCode,
        /// The engine's own words, for the log.
        detail: String,
    },
    /// The engine stopped. `started` = it had reported [`ChatEvent::Ready`].
    Exited {
        /// The helper's exit status, if it exited normally.
        status: Option<i32>,
        started: bool,
    },
}

/// Where events go: called on the engine's own thread.
pub type EventSink = Arc<dyn Fn(ChatEvent) + Send + Sync>;

/// What an engine needs to start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineConfig {
    /// The engine's data folder (`AppPaths::chat_dir`).
    pub dir: PathBuf,
    /// The database key (`chat::key`).
    pub key: String,
    /// The display name for a first start.
    pub name: String,
    /// Relay addresses; empty = the protocol's preset relays.
    pub relays: Vec<String>,
}

/// A running engine. Dropping it stops the engine.
pub trait Engine: Send {
    /// Queue a request (never blocks; requests made while starting wait for the start).
    fn request(&self, request: Request);
    /// Stop now and wait (bounded) until the engine has closed its database: for the end of
    /// the process, when nothing would be left to finish a stop in the background.
    fn shutdown(&mut self) {}
}

/// Starts engines.
pub trait EngineFactory: Send + Sync {
    /// Whether this build can run the engine at all (helper and library present).
    fn available(&self) -> bool;
    /// Start an engine; events go to `sink` until it is dropped or exits.
    fn start(&self, config: EngineConfig, sink: EventSink) -> Result<Box<dyn Engine>>;
    /// What an engine started with `relays` would carry, known before it starts (the page
    /// and Settings show the same controls whether Chat is on or off). Nothing by default.
    fn features(&self, relays: &[String]) -> ChatFeatures {
        let _ = relays;
        ChatFeatures::default()
    }
    /// How long to really wait before restarting after a back-off of `wait`. The wait itself
    /// by default; test engines shorten it so a restart test doesn't sleep through seconds.
    fn restart_wait(&self, wait: std::time::Duration) -> std::time::Duration {
        wait
    }
}
