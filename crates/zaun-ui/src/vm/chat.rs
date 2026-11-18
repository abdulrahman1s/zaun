//! The Chat page's view-model (ADR-0033): the open conversation, its loaded messages, the
//! composer drafts, the text of the page's dialogs, and which one-time requests were sent.
//! No egui types.

use std::collections::{HashMap, HashSet};

use zaun_core::error::UserError;
use zaun_core::ids::QueryId;
use zaun_core::model::{ChatMessage, ChatRef};

use crate::bridge::QueryPayload;

/// Messages per page of a conversation (`ChatCommand::LoadChat`): a shorter page is the
/// first message.
pub const PAGE: usize = 50;

/// Longest name friends see, in characters (after trimming).
pub const NAME_MAX: usize = 40;

/// A name as it is sent: trimmed, 1 to [`NAME_MAX`] characters; `None` when it is not one.
pub fn valid_name(typed: &str) -> Option<String> {
    let t = typed.trim();
    let n = t.chars().count();
    (1..=NAME_MAX).contains(&n).then(|| t.to_owned())
}

/// A dialog of the Chat page, queued with the other dialogs (`vm::dialogs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatDialog {
    /// Your invite, their link, the reusable address.
    AddFriend,
    /// Name a new group.
    NewGroup,
    /// Remove a friend (the contact number) and the conversation.
    RemoveFriend(i64),
    /// Leave a group (its number).
    LeaveGroup(i64),
    /// Your profile: avatar colour, name, about you.
    Profile,
    /// Compare security codes with a friend.
    Verify(i64),
    /// "Share <game> with <friend>?" for this conversation.
    Share(ChatRef),
    /// "Start a game to share it" (nothing is running) for this conversation.
    StartGame(ChatRef),
    /// "Block <name>?"
    Block(i64),
    /// The people you blocked, each with Unblock.
    Blocked,
    /// The link in `ChatVm::link_to_open` may be harmful: open it anyway?
    UnsafeLink,
    /// "Invite to <group>": friends to add, or the group's own link (details).
    InviteToGroup(i64),
    /// A group's picture, for its owner (its number; ADR-0038).
    GroupPicture(i64),
}

/// The open conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conversation {
    /// Which.
    pub chat: ChatRef,
    /// Loaded messages, oldest first, one per id. Shared, so a frame takes the list
    /// without copying it (changes copy it only while a frame still holds the old one).
    pub messages: std::sync::Arc<Vec<ChatMessage>>,
    /// The page asked for and not answered yet, and whether it is an older page.
    pub pending: Option<(QueryId, bool)>,
    /// The latest page has arrived.
    pub loaded: bool,
    /// Every older message is loaded (a page shorter than [`PAGE`] arrived).
    pub complete: bool,
    /// Loading failed (the transcript says so; no new request until it is opened again).
    pub failed: Option<UserError>,
    /// Older messages were put in front: the transcript keeps the reader's place.
    pub prepended: bool,
    /// The transcript was brought to its end once after the first page.
    pub settled: bool,
}

impl Conversation {
    fn new(chat: ChatRef) -> Self {
        Self {
            chat,
            messages: std::sync::Arc::default(),
            pending: None,
            loaded: false,
            complete: false,
            failed: None,
            prepended: false,
            settled: false,
        }
    }

    /// The oldest loaded message's id.
    pub fn oldest(&self) -> Option<i64> {
        self.messages.first().map(|m| m.id)
    }

    /// Put `m` in its place by id; an existing message with that id is replaced.
    fn put(&mut self, m: &ChatMessage) {
        let list = std::sync::Arc::make_mut(&mut self.messages);
        match list.binary_search_by_key(&m.id, |x| x.id) {
            Ok(i) => list[i] = m.clone(),
            Err(i) => list.insert(i, m.clone()),
        }
    }
}

/// Chat page state kept across frames.
#[derive(Debug, Default)]
pub struct ChatVm {
    /// The open conversation.
    pub open: Option<Conversation>,
    /// What is typed in each conversation's composer.
    pub drafts: HashMap<ChatRef, String>,
    /// "Your name" while Chat is off (taken from the preferences once).
    pub name: Option<String>,
    /// The switch was turned on: shown on until the snapshot leaves Off.
    pub turning_on: bool,
    /// The Add-a-friend dialog's "Their link" field.
    pub link: String,
    /// The New-group dialog's name field.
    pub group_name: String,
    /// A one-time invite was asked for while the Add-a-friend dialog is open.
    pub invite_asked: bool,
    /// `MarkRead` was sent for this conversation at this latest message.
    pub marked: Option<(ChatRef, Option<i64>)>,
    /// The conversation row to give focus to (after closing a conversation).
    pub focus_row: Option<ChatRef>,
    /// Put focus in the composer when it next shows.
    pub focus_composer: bool,
    /// The last frame showed only the open conversation (a narrow window): Back closes it.
    pub narrow_open: bool,
    // ---- chat.html section 8 (state.chat). Each field says which part owns it.
    /// The list's search (list).
    pub query: String,
    /// The details panel; `None` = the default for the width (open from 1240) (details).
    pub details: Option<bool>,
    /// The message being answered (composer, conversation).
    pub reply: Option<i64>,
    /// The user's message being edited; its text is in the draft (composer, conversation).
    pub edit: Option<i64>,
    /// An open popover (composer, conversation).
    pub picker: Option<Picker>,
    /// Suggestions while typing `:na` or `@na` (composer).
    pub suggest: Option<Suggest>,
    /// A voice message being recorded since this UI time in seconds (composer).
    pub recording: Option<f64>,
    /// The voice message playing (conversation).
    pub voice: Option<VoicePlay>,
    /// The formatting row under "Aa" (composer).
    pub format_bar: bool,
    /// The watched stream takes 72% of the height (trays).
    pub stage_large: bool,
    /// How often each emoji was used: "Frequently used", quick reactions, `:` order (emoji).
    pub freq: HashMap<String, u32>,
    /// Saved GIFs (gif).
    pub saved_gifs: Vec<String>,
    /// "New" sits above this message of this conversation (conversation).
    pub new_from: Option<(ChatRef, i64)>,
    /// Spoilers (message, index) and collapsed messages (message, `u32::MAX`) the user
    /// opened (conversation).
    pub revealed: HashSet<(i64, u32)>,
    /// A link waiting for the user's answer in `ChatDialog::UnsafeLink` (services).
    pub link_to_open: Option<String>,
    /// Pictures and files waiting in the composer, sent with the next message (composer).
    pub attachments: Vec<Attachment>,
    /// Where Ctrl+V reads the clipboard from (composer; tests set their own).
    pub clipboard: Clipboard,
    /// The open conversation takes files dropped on the window in this pass (the composer
    /// sets it while it draws; the shell's drop zone reads and clears it afterwards).
    pub takes_drops: bool,
    /// That link was not checked (no answer), rather than flagged (services).
    pub link_unchecked: bool,
    /// The GIF picker's search and pictures (services).
    pub gif: GifVm,
    /// Messages deleted for everyone but held back while their toast offers Undo; each goes
    /// out when its toast leaves (conversation).
    pub deleting: Vec<Deleting>,
    /// The picture shown in the full-window viewer (conversation).
    pub viewer: Option<Viewer>,
    // ---- list agent (page layout, list)
    /// What `details: None` means at the window's width: open from 1240 wide (list).
    pub details_auto: bool,
    // ---- details agent (trays, details, incoming, dialogs)
    /// A share waits for the game started from "Start a game to share it" (details).
    pub pending_share: Option<PendingShare>,
    /// The Add-a-friend dialog's tab: 0 "Share my link", 1 "I have a link" (details).
    pub invite_tab: usize,
    /// The New-group dialog's ticked friends (details).
    pub group_pick: Vec<i64>,
    /// A group just asked for, waiting to appear so its members can be added (details).
    pub pending_group: Option<PendingGroup>,
    /// The Your-profile dialog's draft (`None`: not opened yet) (details).
    pub profile: Option<ProfileDraft>,
    /// The Share dialog's choices for this session, until `[chat]` keeps them (details).
    pub share_choice: Option<ShareChoice>,
    /// `ShowCode` was sent for this friend while the Verify dialog is open (details).
    pub code_asked: Option<i64>,
    /// The Invite-to-group dialog: its tab (0 friends, 1 link), the friends ticked, and the
    /// search over them (details).
    pub group_invite: GroupInvite,
    /// The picture being chosen in Your profile or a group's picture (pictures).
    pub picture: crate::vm::chat_picture::PictureEdit,
    /// The call box, the watch stage and the share dialog's steps (callui).
    pub call: crate::vm::chat_call::CallVm,
}

/// The Invite-to-group dialog's state (details).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GroupInvite {
    /// 0 = Friends, 1 = Share a link.
    pub tab: usize,
    /// Friends ticked.
    pub pick: Vec<i64>,
    /// The search over them.
    pub query: String,
    /// `CreateGroupLink` was sent and the link has not arrived yet.
    pub link_asked: bool,
}

/// A share that begins once the game started for it is running (details).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingShare {
    /// The conversation.
    pub chat: ChatRef,
    /// The game started for it.
    pub game: zaun_core::ids::GameId,
    /// The game was seen starting or running (an idle game after that ends the wait).
    pub started: bool,
    /// When it was asked for: a game that has not started within [`WAIT_SECS`] ends the
    /// wait (the launch was refused or needs something first).
    pub asked: zaun_core::clock::MonoInstant,
}

/// How long a share waits for its game to start, or a new group to appear (details).
pub const WAIT_SECS: u64 = 30;

/// A group asked for with `NewGroup`, whose friends are added once it exists (details).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingGroup {
    /// Its name.
    pub name: String,
    /// The friends to add.
    pub friends: Vec<i64>,
    /// The groups there were before it was asked for.
    pub before: Vec<i64>,
    /// When it was asked for (it stops waiting after [`WAIT_SECS`]).
    pub asked: zaun_core::clock::MonoInstant,
}

/// The Your-profile dialog's fields (details).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileDraft {
    /// Name.
    pub name: String,
    /// About you.
    pub about: String,
    /// The avatar's pair (0..10).
    pub paper: u8,
}

/// The Share dialog's picture and sound (details).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShareChoice {
    /// 0 = 720p, 1 = 1080p, 2 = 1440p, 3 = as in the game (callui: notes J).
    pub quality: usize,
    /// 0 = 30, 1 = 60 frames a second.
    pub fps: usize,
    /// Include the game's sound.
    pub sound: bool,
}

/// A picture of a message shown over the whole window (conversation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Viewer {
    /// The conversation.
    pub chat: ChatRef,
    /// The message.
    pub item: i64,
}

/// A message the user deleted, waiting for its toast's Undo (conversation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deleting {
    /// The conversation.
    pub chat: ChatRef,
    /// The message.
    pub item: i64,
    /// The toast's undo key (`UiState::undo`); gone from there means Undo was pressed.
    pub undo_key: u64,
}

/// The GIF picker's state (services): the search, Klipy's answer and the pictures asked
/// for. Pictures are kept as bytes; the picker decodes them.
#[derive(Debug, Default)]
pub struct GifVm {
    /// What is typed in the search field.
    pub query: String,
    /// When the field last changed (UI seconds): the search goes out after a pause.
    pub typed_at: f64,
    /// The words of the list shown or awaited (`""` = trending); `None` before the first.
    pub asked: Option<String>,
    /// The answer awaited.
    pub pending: Option<QueryId>,
    /// The last answer.
    pub answer: Option<zaun_core::gifs::GifAnswer>,
    /// Picker pictures by address.
    pub previews: HashMap<String, GifPicture>,
    /// Picture requests in flight.
    pub preview_q: HashMap<QueryId, String>,
}

/// A picker picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GifPicture {
    /// Asked for.
    Asked,
    /// Arrived.
    Ready(zaun_core::gifs::GifBytes),
    /// Not available.
    Failed,
}

/// Picker pictures kept at most (the oldest answers' pictures go first).
pub const GIF_PICTURES_MAX: usize = 120;

impl GifVm {
    /// A query answer: `true` when it was one of the picker's.
    pub fn on_query(&mut self, id: QueryId, payload: &QueryPayload) -> bool {
        if let Some(url) = self.preview_q.remove(&id) {
            let p = match payload {
                QueryPayload::GifPreview(g) if g.url == url => match &g.bytes {
                    Some(b) => GifPicture::Ready(b.clone()),
                    None => GifPicture::Failed,
                },
                _ => GifPicture::Failed,
            };
            self.previews.insert(url, p);
            return true;
        }
        if self.pending != Some(id) {
            return false;
        }
        self.pending = None;
        match payload {
            QueryPayload::Gifs(a) => {
                // Pictures of other lists are dropped once there are many.
                if self.previews.len() > GIF_PICTURES_MAX {
                    let keep: HashSet<&str> =
                        a.gifs.iter().map(|g| g.preview_url.as_str()).collect();
                    self.previews
                        .retain(|u, p| keep.contains(u.as_str()) || *p == GifPicture::Asked);
                }
                self.answer = Some(a.clone());
            }
            _ => {
                self.answer = Some(zaun_core::gifs::GifAnswer::problem(
                    self.asked.clone().unwrap_or_default(),
                    zaun_core::gifs::GifProblem::Offline,
                ));
            }
        }
        true
    }
}

/// A picture or file waiting in the composer (pasted, dropped or chosen; composer).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Attachment {
    /// A picture from the clipboard: its size and RGBA pixels.
    Image {
        /// Width and height in pixels.
        size: [usize; 2],
        /// RGBA, not premultiplied, row by row.
        rgba: std::sync::Arc<Vec<u8>>,
    },
    /// A file on this computer (a picture, a video or anything else).
    File(std::path::PathBuf),
}

/// Attachments waiting at most (composer).
pub const MAX_ATTACHMENTS: usize = 10;

/// What the clipboard holds, as far as pasting into Chat goes (composer).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Clip {
    /// Files copied in a file manager (`text/uri-list`).
    Files(Vec<std::path::PathBuf>),
    /// A picture: its size and RGBA pixels (not premultiplied).
    Image {
        /// Width and height.
        size: [usize; 2],
        /// The pixels, row by row.
        rgba: Vec<u8>,
    },
    /// Text, or nothing: an ordinary paste.
    Other,
}

/// Reads the clipboard when Ctrl+V reaches the composer's field (behind a trait so tests
/// need no clipboard). Reading it is not file I/O; files are only named, never opened.
pub trait ClipboardSource: Send + Sync {
    /// What is on the clipboard now.
    fn read(&self) -> Clip;
}

/// The clipboard the composer reads: the desktop's (`None`), or one a test set (composer).
#[derive(Clone, Default)]
pub struct Clipboard(Option<std::sync::Arc<dyn ClipboardSource>>);

impl Clipboard {
    /// Read `source` instead of the desktop's clipboard.
    pub fn new(source: std::sync::Arc<dyn ClipboardSource>) -> Self {
        Self(Some(source))
    }

    /// The source set, if any.
    pub fn source(&self) -> Option<&std::sync::Arc<dyn ClipboardSource>> {
        self.0.as_ref()
    }
}

impl std::fmt::Debug for Clipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() {
            "Clipboard(set)"
        } else {
            "Clipboard(desktop)"
        })
    }
}

/// A popover of the Chat page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Picker {
    /// The emoji picker: inserting into the composer, or reacting to a message.
    Emoji {
        /// The message to react to (`None` = insert at the cursor).
        react_to: Option<i64>,
    },
    /// The GIF picker.
    Gif,
}

/// What a suggestion list offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuggestKind {
    /// Emoji names after `:`.
    Emoji,
    /// Group members after `@`.
    Mention,
}

/// The suggestions above the composer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggest {
    /// Emoji or members.
    pub kind: SuggestKind,
    /// What is typed after the `:` or `@`.
    pub query: String,
    /// The highlighted suggestion.
    pub index: usize,
}

/// A voice message playing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoicePlay {
    /// The message.
    pub item: i64,
    /// Where, in seconds.
    pub pos: f32,
    /// 1, 1.5 or 2.
    pub speed: f32,
    /// Paused.
    pub paused: bool,
}

impl ChatVm {
    /// Add attachments to the composer (at most [`MAX_ATTACHMENTS`] wait, each once).
    pub fn attach(&mut self, more: Vec<Attachment>) {
        for a in more {
            if self.attachments.len() < MAX_ATTACHMENTS && !self.attachments.contains(&a) {
                self.attachments.push(a);
            }
        }
    }

    /// Open `chat` (or keep it open). Returns `true` when it was not open, or its loading
    /// failed, so the caller asks for its latest page.
    pub fn open_chat(&mut self, chat: ChatRef) -> bool {
        if self
            .open
            .as_ref()
            .is_some_and(|c| c.chat == chat && c.failed.is_none())
        {
            return false;
        }
        self.open = Some(Conversation::new(chat));
        self.focus_composer = true;
        true
    }

    /// Close the open conversation; its row takes focus.
    pub fn close_chat(&mut self) {
        if let Some(c) = self.open.take() {
            self.focus_row = Some(c.chat);
        }
        self.narrow_open = false;
    }

    /// The page `q` was asked for (`older`: before the oldest loaded message).
    pub fn asked(&mut self, q: QueryId, older: bool) {
        if let Some(c) = &mut self.open {
            c.pending = Some((q, older));
        }
    }

    /// Whether an older page should be asked for now (the reader reached the top).
    pub fn wants_older(&self) -> Option<i64> {
        let c = self.open.as_ref()?;
        if !c.loaded || c.complete || c.pending.is_some() || c.failed.is_some() {
            return None;
        }
        c.oldest()
    }

    /// Messages arrived or changed (`updated`) — from `UiEvent::ChatMessages`. New ones of
    /// the open conversation join it (once each); changed ones replace theirs.
    pub fn on_messages(&mut self, messages: &[ChatMessage], updated: bool) {
        let Some(c) = &mut self.open else {
            return;
        };
        let chat = c.chat;
        for m in messages.iter().filter(|m| m.chat == chat) {
            let known = c.messages.iter().any(|x| x.id == m.id);
            if updated && !known {
                // A change to a message not loaded (older than the loaded pages).
                continue;
            }
            c.put(m);
        }
    }

    /// A query answer: returns `true` when it was one of this page's.
    pub fn on_query(&mut self, id: QueryId, payload: &QueryPayload) -> bool {
        let Some(c) = &mut self.open else {
            return false;
        };
        let Some((q, older)) = c.pending else {
            return false;
        };
        if q != id {
            return false;
        }
        c.pending = None;
        match payload {
            QueryPayload::Chat(page) if page.chat == c.chat => {
                if page.messages.len() < PAGE {
                    c.complete = true;
                }
                let before = c.messages.len();
                for m in &page.messages {
                    c.put(m);
                }
                if older && c.messages.len() > before {
                    c.prepended = true;
                }
                c.loaded = true;
            }
            QueryPayload::Failed(e) => c.failed = Some(e.clone()),
            _ => {}
        }
        true
    }

    /// Whether the details panel is open: the user's choice, else the default for the
    /// window's width (open from 1240 wide).
    pub fn details_open(&self) -> bool {
        self.details.unwrap_or(self.details_auto)
    }

    /// Open or close the details panel; the choice then sticks for the session.
    pub fn toggle_details(&mut self) {
        self.details = Some(!self.details_open());
    }

    /// `Esc` on the Chat page (chat.html `chatKey`), one step at a time: dismiss the
    /// suggestions, clear the search (when `search_focused`), close a picker, discard a
    /// recording, cancel a reply or an edit (an edit empties the field), shrink the stage,
    /// close the details, and in a narrow window go back to the list. Returns whether a
    /// step was taken.
    pub fn escape(&mut self, search_focused: bool) -> bool {
        // The picture viewer covers everything: it closes first (conversation).
        if self.viewer.take().is_some() {
            return true;
        }
        // The call's device menu, then full screen (callui).
        if self.call.escape_over() {
            return true;
        }
        // Attachments waiting go first, while the field is empty (composer).
        if !self.attachments.is_empty()
            && self
                .open
                .as_ref()
                .is_none_or(|o| self.drafts.get(&o.chat).is_none_or(String::is_empty))
        {
            self.attachments.clear();
            return true;
        }
        if self.suggest.take().is_some() {
            return true;
        }
        if search_focused && !self.query.is_empty() {
            self.query.clear();
            return true;
        }
        if self.picker.take().is_some() || self.recording.take().is_some() {
            return true;
        }
        if self.reply.is_some() || self.edit.is_some() {
            self.reply = None;
            if self.edit.take().is_some()
                && let Some(c) = self.open.as_ref().map(|o| o.chat)
            {
                self.drafts.remove(&c);
            }
            return true;
        }
        if self.stage_large {
            self.stage_large = false;
            return true;
        }
        // A focused tile of the call's stage (callui).
        if self.call.unfocus() {
            return true;
        }
        if self.open.is_some() && self.details_open() {
            self.details = Some(false);
            return true;
        }
        if self.narrow_open && self.open.is_some() {
            self.close_chat();
            return true;
        }
        false
    }

    /// Whether `MarkRead` should go out for `chat`, whose latest message is `latest` and
    /// which has `unread` messages; records it when so (once per latest message).
    pub fn should_mark(&mut self, chat: ChatRef, latest: Option<i64>, unread: u32) -> bool {
        if unread == 0 || self.marked == Some((chat, latest)) {
            return false;
        }
        self.marked = Some((chat, latest));
        true
    }
}

#[cfg(test)]
mod tests {
    use zaun_core::clock::Timestamp;
    use zaun_core::model::{Delivery, MessageAuthor, MessageBody};
    use zaun_core::service::ChatPage;

    use super::*;

    fn msg(id: i64, text: &str) -> ChatMessage {
        ChatMessage {
            id,
            chat: ChatRef::Direct(1),
            author: MessageAuthor::Friend,
            body: MessageBody::Text(text.into()),
            at: Timestamp::from_millis(id * 1000),
            delivery: Delivery::Received,
            unread: false,
            reply: None,
            reactions: Vec::new(),
            edited: false,
            mentions: Vec::new(),
        }
    }

    fn q(n: u128) -> QueryId {
        QueryId::from_uuid(uuid::Uuid::from_u128(n))
    }

    #[test]
    fn pages_merge_in_order_and_new_messages_join_once() {
        let mut vm = ChatVm::default();
        assert!(vm.open_chat(ChatRef::Direct(1)));
        assert!(!vm.open_chat(ChatRef::Direct(1)));
        vm.asked(q(1), false);
        let page = ChatPage {
            chat: ChatRef::Direct(1),
            messages: (60..110).map(|i| msg(i, "a")).collect(),
        };
        assert!(vm.on_query(q(1), &QueryPayload::Chat(page)));
        assert_eq!(vm.wants_older(), Some(60));
        vm.asked(q(2), true);
        assert_eq!(vm.wants_older(), None);
        let older = ChatPage {
            chat: ChatRef::Direct(1),
            messages: (50..60).map(|i| msg(i, "b")).collect(),
        };
        assert!(vm.on_query(q(2), &QueryPayload::Chat(older)));
        let c = vm.open.as_ref().unwrap();
        assert!(c.complete && c.prepended);
        assert_eq!(c.messages.first().map(|m| m.id), Some(50));
        assert_eq!(vm.wants_older(), None);
        vm.on_messages(&[msg(110, "new"), msg(110, "new")], false);
        vm.on_messages(&[msg(110, "edited"), msg(7, "unknown")], true);
        let c = vm.open.as_ref().unwrap();
        assert_eq!(c.messages.len(), 61);
        assert_eq!(
            c.messages.last().unwrap().body,
            MessageBody::Text("edited".into())
        );
        assert!(!vm.on_query(
            q(3),
            &QueryPayload::Chat(ChatPage {
                chat: ChatRef::Direct(1),
                messages: vec![]
            })
        ));
    }

    /// Only the open conversation's messages are held: another one opened, or the page
    /// closed, drops them; what was typed stays.
    #[test]
    fn only_the_open_conversation_keeps_its_messages() {
        let mut vm = ChatVm::default();
        vm.open_chat(ChatRef::Direct(1));
        vm.asked(q(1), false);
        let page = ChatPage {
            chat: ChatRef::Direct(1),
            messages: (0..PAGE as i64).map(|i| msg(i, "a")).collect(),
        };
        assert!(vm.on_query(q(1), &QueryPayload::Chat(page)));
        vm.drafts.insert(ChatRef::Direct(1), "half typed".into());
        assert!(vm.open_chat(ChatRef::Direct(2)));
        let open = vm.open.as_ref().unwrap();
        assert_eq!(open.chat, ChatRef::Direct(2));
        assert!(open.messages.is_empty());
        vm.on_messages(&[msg(99, "late")], false);
        assert!(vm.open.as_ref().unwrap().messages.is_empty());
        vm.close_chat();
        assert!(vm.open.is_none());
        assert_eq!(
            vm.drafts.get(&ChatRef::Direct(1)).map(String::as_str),
            Some("half typed")
        );
    }

    #[test]
    fn mark_read_goes_out_once_per_latest_message() {
        let mut vm = ChatVm::default();
        let c = ChatRef::Group(3);
        assert!(vm.should_mark(c, Some(9), 2));
        assert!(!vm.should_mark(c, Some(9), 2));
        assert!(!vm.should_mark(c, Some(10), 0));
        assert!(vm.should_mark(c, Some(10), 1));
    }

    /// `Esc` takes one step at a time (list agent).
    #[test]
    fn escape_takes_one_step_at_a_time() {
        let mut vm = ChatVm {
            details_auto: true,
            ..ChatVm::default()
        };
        assert!(!vm.escape(false));
        vm.open_chat(ChatRef::Direct(1));
        vm.narrow_open = true;
        vm.edit = Some(4);
        vm.drafts.insert(ChatRef::Direct(1), "fixed".into());
        vm.picker = Some(Picker::Gif);
        assert!(vm.escape(false));
        assert!(vm.picker.is_none() && vm.edit.is_some());
        assert!(vm.escape(false));
        assert!(vm.edit.is_none() && vm.drafts.is_empty());
        assert!(vm.details_open());
        assert!(vm.escape(false));
        assert!(!vm.details_open());
        assert!(vm.escape(false));
        assert!(vm.open.is_none());
        assert!(!vm.escape(false));
    }

    #[test]
    fn names_are_trimmed_and_at_most_forty_characters() {
        assert_eq!(valid_name("  Sam "), Some("Sam".into()));
        assert_eq!(valid_name("   "), None);
        assert!(valid_name(&"x".repeat(40)).is_some());
        assert!(valid_name(&"x".repeat(41)).is_none());
    }
}
