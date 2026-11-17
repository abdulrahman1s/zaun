//! Friends (ADR-0033): what the Friends page shows, independent of the protocol that
//! carries it. The engine behind it (`crate::friends`) maps its protocol to these types, so
//! the service and the UI never see a protocol's own vocabulary.

use crate::clock::Timestamp;

/// The user's own profile, as friends see it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FriendProfile {
    /// Display name.
    pub name: String,
}

/// A conversation: with one friend, or a group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ChatRef {
    /// With one friend (the engine's contact number).
    Direct(i64),
    /// A group (the engine's group number).
    Group(i64),
}

/// A friend: someone the user is connected with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Friend {
    /// The engine's contact number.
    pub id: i64,
    /// Their display name.
    pub name: String,
    /// The connection is ready for messages (otherwise still being set up).
    pub ready: bool,
}

/// The user's place in a group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Membership {
    /// Invited, not joined yet.
    Invited,
    /// Joining (accepted, waiting for the host).
    Joining,
    /// A member.
    Member,
    /// Left or removed.
    Left,
}

/// A group conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FriendGroup {
    /// The engine's group number.
    pub id: i64,
    /// The group's name.
    pub name: String,
    /// The user's place in it.
    pub membership: Membership,
    /// Current members, the user included (when known).
    pub members: Option<u32>,
}

/// Someone asked to connect through the user's address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FriendRequest {
    /// The engine's request number.
    pub id: i64,
    /// The name they give.
    pub name: String,
}

/// Who wrote a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageAuthor {
    /// The user.
    Me,
    /// A friend in a direct chat.
    Friend,
    /// A member of a group, by name.
    Member(String),
}

/// What a message is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageBody {
    /// Text the author wrote.
    Text(String),
    /// Something that happened in the conversation (a member joined, encryption is on),
    /// in the engine's words.
    Event(String),
}

/// Where a message the user sent is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Not on a relay yet.
    Sending,
    /// On the relay.
    Sent,
    /// The friend's device has it.
    Delivered,
    /// It could not be sent.
    Failed,
    /// A message the user received.
    Received,
}

/// One message in a conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FriendMessage {
    /// The engine's item number (increases within a conversation).
    pub id: i64,
    /// The conversation.
    pub chat: ChatRef,
    /// Who wrote it.
    pub author: MessageAuthor,
    /// What it is.
    pub body: MessageBody,
    /// When it was written.
    pub at: Timestamp,
    /// Delivery of the user's own messages.
    pub delivery: Delivery,
    /// Not read yet.
    pub unread: bool,
}

/// A conversation as the chat list shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatPreview {
    /// The conversation.
    pub chat: ChatRef,
    /// The friend's or group's name.
    pub name: String,
    /// The latest message, if any.
    pub last: Option<FriendMessage>,
    /// Messages not read yet.
    pub unread: u32,
}

/// Whether Friends runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum FriendsStatus {
    /// Turned off (the default).
    #[default]
    Off,
    /// This build has no chat engine (the helper or its library is missing).
    Unavailable,
    /// Starting (opening the encrypted database, reaching the relays).
    Starting,
    /// Running.
    Ready,
    /// Stopped after repeated failures; the code says why.
    Failed(crate::error::ErrorCode),
}

/// Everything the Friends page shows, published as one snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FriendsSnapshot {
    /// Whether Friends runs.
    pub status: FriendsStatus,
    /// The user's profile (once started).
    pub profile: Option<FriendProfile>,
    /// The user's reusable address, if they made one.
    pub address: Option<String>,
    /// The one-time invitation link made last, until someone uses it.
    pub invite: Option<String>,
    /// Friends.
    pub friends: Vec<Friend>,
    /// Groups.
    pub groups: Vec<FriendGroup>,
    /// Requests through the address, waiting for an answer.
    pub requests: Vec<FriendRequest>,
    /// Conversations, most recent first.
    pub chats: Vec<ChatPreview>,
}

impl FriendsSnapshot {
    /// Unread messages over every conversation (the top bar's dot).
    pub fn unread(&self) -> u32 {
        self.chats.iter().map(|c| c.unread).sum()
    }

    /// The friend with this contact number.
    pub fn friend(&self, id: i64) -> Option<&Friend> {
        self.friends.iter().find(|f| f.id == id)
    }

    /// The group with this number.
    pub fn group(&self, id: i64) -> Option<&FriendGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// The conversation's name, as the list shows it.
    pub fn chat_name(&self, chat: ChatRef) -> Option<&str> {
        match chat {
            ChatRef::Direct(id) => self.friend(id).map(|f| f.name.as_str()),
            ChatRef::Group(id) => self.group(id).map(|g| g.name.as_str()),
        }
    }
}
