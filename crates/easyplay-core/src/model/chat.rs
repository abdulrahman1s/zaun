//! Chat (ADR-0033): what the Chat page shows, independent of the protocol that
//! carries it. The engine behind it (`crate::chat`) maps its protocol to these types, so
//! the service and the UI never see a protocol's own vocabulary.

use crate::clock::Timestamp;

/// The user's own profile, as friends see it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChatProfile {
    /// Display name.
    pub name: String,
    /// A line about themselves (empty = none).
    pub about: String,
    /// The avatar's paper-and-ink pair (0..10; `None` = from the name).
    pub paper: Option<u8>,
    /// What the user tells friends about being around.
    pub status: MyStatus,
}

/// The status the user chooses (chat.html "you" menu).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MyStatus {
    /// Online.
    #[default]
    Online,
    /// Away.
    Away,
    /// Do not disturb.
    Busy,
    /// Shown as offline.
    Invisible,
}

/// Whether a friend is around, as their app tells it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Presence {
    /// Online.
    Online,
    /// Away.
    Away,
    /// Do not disturb.
    Busy,
    /// Offline (or not telling).
    #[default]
    Offline,
}

/// The game someone is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Playing {
    /// The game's title (its poster is made from it).
    pub title: String,
    /// Minutes in this session.
    pub minutes: u32,
    /// They share it (watch-only stream) right now.
    pub live: bool,
}

/// What this Chat can do. The preview turns everything on; the real engine only what it
/// carries (ADR-0033 phases). The page does not draw a control whose feature is off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct ChatFeatures {
    /// Online / away / "Playing …" between friends.
    pub presence: bool,
    /// Voice calls (direct and group voice).
    pub calls: bool,
    /// Sharing a game and watching a friend's.
    pub share: bool,
    /// Voice messages.
    pub voice: bool,
    /// GIFs.
    pub gifs: bool,
    /// Pictures and files.
    pub files: bool,
    /// Emoji reactions.
    pub reactions: bool,
    /// Replies that quote a message.
    pub replies: bool,
    /// Editing and deleting your own messages.
    pub edit: bool,
    /// Muting a conversation.
    pub mute: bool,
    /// Blocking a friend.
    pub block: bool,
    /// Comparing security codes.
    pub verify: bool,
    /// @mentions in groups.
    pub mentions: bool,
    /// Previews for links the user sends (`chat.link_previews`).
    pub link_previews: bool,
}

impl ChatFeatures {
    /// Everything (the preview).
    pub fn all() -> Self {
        Self {
            presence: true,
            calls: true,
            share: true,
            voice: true,
            gifs: true,
            files: true,
            reactions: true,
            replies: true,
            edit: true,
            mute: true,
            block: true,
            verify: true,
            mentions: true,
            link_previews: true,
        }
    }
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Friend {
    /// The engine's contact number.
    pub id: i64,
    /// Their display name.
    pub name: String,
    /// The connection is ready for messages (otherwise still being set up).
    pub ready: bool,
    /// Whether they are around (`Offline` when unknown).
    pub presence: Presence,
    /// The game they are in.
    pub playing: Option<Playing>,
    /// "Away for 20 min", "Last seen 3 hours ago": when they were last around.
    pub last_seen: Option<Timestamp>,
    /// Their line about themselves.
    pub about: String,
    /// Their avatar pair, if they chose one.
    pub paper: Option<u8>,
    /// Friends since.
    pub since: Option<Timestamp>,
    /// The security code was compared and matched.
    pub verified: bool,
    /// Blocked by the user (silently).
    pub blocked: bool,
    /// Their conversation is muted.
    pub muted: bool,
    /// Games the app has seen them in, most recent first ("Also plays").
    pub seen: Vec<String>,
}

/// The user's place in a group.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Membership {
    /// Invited, not joined yet.
    Invited,
    /// Joining (accepted, waiting for the host).
    Joining,
    /// A member.
    #[default]
    Member,
    /// Left or removed.
    Left,
}

/// A group conversation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChatGroup {
    /// The engine's group number.
    pub id: i64,
    /// The group's name.
    pub name: String,
    /// The user's place in it.
    pub membership: Membership,
    /// Current members, the user included (when known).
    pub members: Option<u32>,
    /// The members, the user included (empty until loaded).
    pub member_list: Vec<GroupMember>,
    /// The group is muted.
    pub muted: bool,
    /// Members in the group's voice right now (member numbers).
    pub voice: Vec<i64>,
    /// The group's invite link (short form when the relay offers one), if made.
    pub link: Option<String>,
}

/// One member of a group.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GroupMember {
    /// The engine's member number (what a mention refers to).
    pub id: i64,
    /// Their name in the group.
    pub name: String,
    /// The friend behind them, if they are one.
    pub friend: Option<i64>,
    /// This is the user.
    pub me: bool,
    /// They made the group.
    pub owner: bool,
    /// Whether they are around.
    pub presence: Presence,
    /// The game they are in.
    pub playing: Option<Playing>,
    /// Blocked by the user (their messages collapse).
    pub blocked: bool,
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
    /// Text the author wrote (markdown, chat.html section 8 MARKDOWN).
    Text(String),
    /// Something that happened in the conversation (a member joined, a friend changed
    /// name): typed, so the page words it in the user's language.
    Notice(ChatNotice),
    /// A voice message.
    Voice {
        /// Its length.
        secs: u32,
        /// Bar heights 0..=255 for the waveform (empty = flat).
        wave: Vec<u8>,
    },
    /// A GIF, sent as a file (its name; the preview draws a stand-in).
    Gif(String),
    /// A picture (core agent, round 3).
    Image(Media),
    /// A video.
    Video(Media),
    /// Any other file.
    File(Media),
    /// A call, as one line.
    Call(CallRecord),
    /// A shared game (watch only).
    Stream(StreamRecord),
    /// Deleted for everyone.
    Deleted,
}

/// Something that happened in a conversation (core agent, round 3c). Who did it is the
/// message's author where that is a member; `None` in `by` is the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatNotice {
    /// The user and the friend are connected (the first line of a direct chat).
    Connected,
    /// The user made this group.
    GroupCreated,
    /// The user joined this group.
    YouJoined,
    /// A member joined (their name).
    MemberJoined(String),
    /// Someone added a member.
    MemberAdded {
        /// Who added them (`None` = the user).
        by: Option<String>,
        /// Who was added.
        who: String,
    },
    /// A member left (their name).
    MemberLeft(String),
    /// Someone removed a member.
    MemberRemoved {
        /// Who removed them (`None` = the user).
        by: Option<String>,
        /// Who was removed.
        who: String,
    },
    /// The user left the group.
    YouLeft,
    /// The user was removed from the group (by whom).
    YouRemoved(String),
    /// The group was deleted.
    GroupDeleted,
    /// The group's name changed (the new name).
    GroupNameChanged(String),
    /// A friend or member changed their name.
    NameChanged {
        /// The old name.
        from: String,
        /// The new name.
        to: String,
    },
    /// The friend deleted the conversation with the user (no more messages arrive).
    FriendDeleted,
    /// An invitation to a group, received or sent, in a direct chat.
    GroupInvitation {
        /// The group's name.
        group: String,
        /// The user sent it (to the friend).
        sent: bool,
    },
    /// The security code with this friend changed (compare it again).
    CodeChanged,
}

/// A picture, video or file in a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Media {
    /// The file's name, as the sender gave it.
    pub name: String,
    /// Its size in bytes.
    pub size: u64,
    /// A small preview picture (the bytes of a JPEG or PNG), shown before it is fetched.
    pub preview: Option<Vec<u8>>,
    /// A video's length in seconds, when the sender said.
    pub duration: Option<u32>,
    /// Where the file is.
    pub state: FileState,
}

/// Where a file in a message is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileState {
    /// Offered by the sender, not fetched (`ChatCommand::ReceiveFile` fetches it).
    Offered,
    /// Being fetched.
    Receiving {
        /// 0..=100.
        percent: u8,
    },
    /// Here: a file inside the Chat folder.
    Received(std::path::PathBuf),
    /// Being sent.
    Sending {
        /// 0..=100.
        percent: u8,
    },
    /// Sent (the user's own file stays where it was copied, inside the Chat folder).
    Sent,
    /// It could not be sent or fetched.
    Failed,
    /// Sending or fetching was stopped.
    Cancelled,
}

/// How a call went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallKind {
    /// It happened.
    Ended,
    /// Nobody answered.
    Missed,
    /// Declined.
    Declined,
}

/// A call in the transcript.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallRecord {
    /// How it went.
    pub kind: CallKind,
    /// How long it lasted.
    pub minutes: u32,
}

/// A shared game in the transcript.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamRecord {
    /// The game's title.
    pub title: String,
    /// Still going.
    pub live: bool,
    /// People watching.
    pub watching: u32,
    /// How long it ran (once over).
    pub minutes: u32,
}

/// The message a reply answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quote {
    /// That message's number, if it is still there.
    pub id: Option<i64>,
    /// Who wrote it.
    pub author: MessageAuthor,
    /// Its first line.
    pub text: String,
}

/// One emoji's reactions on a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reaction {
    /// The emoji.
    pub emoji: String,
    /// How many reacted with it.
    pub count: u32,
    /// The user is one of them.
    pub mine: bool,
    /// Their names, when known (the tooltip).
    pub who: Vec<String>,
}

/// A member named with @ in a group message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mention {
    /// The name as written after @.
    pub name: String,
    /// The member's number.
    pub member: i64,
    /// It is the user.
    pub me: bool,
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
pub struct ChatMessage {
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
    /// The message it answers.
    pub reply: Option<Quote>,
    /// Reactions, in the order they were first added.
    pub reactions: Vec<Reaction>,
    /// Edited after sending.
    pub edited: bool,
    /// Members it mentions (groups).
    pub mentions: Vec<Mention>,
}

/// A conversation as the chat list shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatPreview {
    /// The conversation.
    pub chat: ChatRef,
    /// The friend's or group's name.
    pub name: String,
    /// The latest message, if any.
    pub last: Option<ChatMessage>,
    /// Messages not read yet.
    pub unread: u32,
    /// Muted (does not add to the top bar's count).
    pub muted: bool,
}

/// A call the user is in (or ringing).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    /// The conversation.
    pub chat: ChatRef,
    /// Ringing out, or connected since.
    pub since: Option<Timestamp>,
    /// Who else is in it (names).
    pub with: Vec<String>,
    /// Who is speaking now (names; "you" excluded).
    pub speaking: Vec<String>,
    /// The user's microphone is off.
    pub muted: bool,
    /// The user stopped hearing.
    pub deafened: bool,
}

/// Someone is calling the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ringing {
    /// The conversation.
    pub chat: ChatRef,
    /// The caller's name.
    pub name: String,
    /// Since when (unanswered for 20 s it becomes a missed call).
    pub since: Timestamp,
}

/// The game the user shares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Share {
    /// With which conversation.
    pub chat: ChatRef,
    /// The game's title.
    pub title: String,
    /// Since when.
    pub since: Timestamp,
    /// People watching.
    pub watching: u32,
}

/// The stream the user watches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watching {
    /// In which conversation.
    pub chat: ChatRef,
    /// Whose stream (name).
    pub name: String,
    /// The game's title.
    pub title: String,
    /// People watching, the user included.
    pub watching: u32,
    /// The stream's sound is off.
    pub muted: bool,
}

/// Calls, sharing and watching (preview only until voice and streams exist).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChatLive {
    /// The call the user is in.
    pub call: Option<Call>,
    /// Someone calling.
    pub ringing: Option<Ringing>,
    /// The game the user shares.
    pub share: Option<Share>,
    /// The stream the user watches.
    pub watch: Option<Watching>,
}

/// Whether Chat runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ChatStatus {
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

/// Everything the Chat page shows, published as one snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChatSnapshot {
    /// Whether Chat runs.
    pub status: ChatStatus,
    /// The user's profile (once started).
    pub profile: Option<ChatProfile>,
    /// The user's reusable address, if they made one.
    pub address: Option<String>,
    /// The one-time invitation link made last, until someone uses it.
    pub invite: Option<String>,
    /// Friends.
    pub friends: Vec<Friend>,
    /// Groups.
    pub groups: Vec<ChatGroup>,
    /// Requests through the address, waiting for an answer.
    pub requests: Vec<FriendRequest>,
    /// Conversations, most recent first.
    pub chats: Vec<ChatPreview>,
    /// What this Chat can do.
    pub features: ChatFeatures,
    /// The relays cannot be reached (messages wait).
    pub offline: bool,
    /// Calls, sharing and watching.
    pub live: ChatLive,
    /// The security code of a friend, after `ChatCommand::ShowCode` (friend, code digits).
    pub code: Option<(i64, String)>,
    /// The emoji a reaction may use, in display form: `None` = any (the preview),
    /// `Some(list)` = only these (the real engine, `chat::REACTION_EMOJI`).
    pub reaction_emoji: Option<Vec<String>>,
}

impl ChatSnapshot {
    /// Unread messages over every unmuted conversation (the top bar's count).
    pub fn unread(&self) -> u32 {
        self.chats
            .iter()
            .filter(|c| !c.muted)
            .map(|c| c.unread)
            .sum()
    }

    /// The friend with this contact number.
    pub fn friend(&self, id: i64) -> Option<&Friend> {
        self.friends.iter().find(|f| f.id == id)
    }

    /// The group with this number.
    pub fn group(&self, id: i64) -> Option<&ChatGroup> {
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
