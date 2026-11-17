//! The seam between the service and a chat protocol (ADR-0033): requests in the terms of
//! the Chat page, events back in the types of `model::chat`. The SimpleX engine is the
//! one implementation; tests use [`super::fake::FakeEngine`].

use std::path::PathBuf;
use std::sync::Arc;

use crate::error::{ErrorCode, Result};
use crate::model::{
    ChatGroup, ChatMessage, ChatPreview, ChatProfile, ChatRef, Friend, FriendRequest,
};

/// What the service asks the engine to do. Every answer comes back as [`ChatEvent`]s.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Change the user's name.
    SetProfile(ChatProfile),
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
        /// Where.
        chat: ChatRef,
        /// What.
        text: String,
    },
    /// Mark everything in a conversation read.
    MarkRead(ChatRef),
    /// Messages of a conversation, newest last ([`ChatEvent::History`]).
    History {
        /// Which.
        chat: ChatRef,
        /// Only messages older than this item number (`None` = the latest).
        before: Option<i64>,
        /// At most this many.
        count: u32,
        /// Returned with the answer.
        tag: u64,
    },
    /// Read the conversation list again ([`ChatEvent::Chats`]).
    Refresh,
    /// Make a group with the user as owner.
    NewGroup(String),
    /// Invite a friend into a group.
    AddMember {
        /// The group.
        group: i64,
        /// The friend.
        friend: i64,
    },
    /// Join a group the user was invited to.
    JoinGroup(i64),
    /// Leave a group.
    LeaveGroup(i64),
    /// Remove a friend and the conversation with them.
    RemoveFriend(i64),
}

/// Which request failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestKind {
    /// Starting the engine.
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
    /// A group request.
    Group,
    /// [`Request::RemoveFriend`].
    RemoveFriend,
}

/// What the engine reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatEvent {
    /// Started: the database is open and the relays are being reached.
    Ready(ChatProfile),
    /// The user's profile changed.
    Profile(ChatProfile),
    /// The whole conversation list, read again.
    Chats {
        /// Friends.
        friends: Vec<Friend>,
        /// Groups.
        groups: Vec<ChatGroup>,
        /// Requests waiting.
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
        /// The request's tag.
        tag: u64,
        /// The conversation.
        chat: ChatRef,
        /// Oldest first.
        messages: Vec<ChatMessage>,
    },
    /// A request failed.
    Failed {
        /// Which.
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
        /// Whether it had started.
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
}
