//! The SimpleX side of the engine seam: [`Request`]s become chat commands for the helper,
//! the helper's lines become [`ChatEvent`]s. Pure: no I/O, no clock, so it is tested
//! against the JSON recorded from the engine (ADR-0033; chat core 7.0, two helpers through a
//! local SMP relay and a local XFTP file server).
//!
//! The engine's JSON is read tolerantly through `serde_json::Value`: a field that is missing
//! or of another shape drops that item (logged), never the whole line, and unknown event
//! types are ignored. Commands follow `bots/api/COMMANDS.md` of simplex-chat v7.0.3 and the
//! parser in `Simplex.Chat.Library.Commands` for those that file leaves out (`/_get chat`,
//! `/_read chat`, `/_settings`, `/_member settings`, `/_get code`, `/_verify code`,
//! `/_reaction members`, `/set file paths`, `/xftp`).
//!
//! Links: a friend's link may be short (resolved through its relay) or full, so `Connect`
//! asks `/_connect plan` first and connects with the link the plan returns; a link that is
//! the user's own, or one already used, connects nothing. The engine makes every invitation
//! and address with a short link as well (`https://<relay>/i#…` or `/a#…`, no option needed),
//! in the same request to the relay that makes the queue; the user is given the short one
//! (a QR code that scans at the page's size) and the full one only if a relay offers none.
//!
//! Start-up: `/user` (or `/_create user` when there is none), `/set file paths` (it must
//! come before `/_start`), `/_start`, then `/smp` and `/xftp` when custom relays are set
//! (refused before `/_start`), then the chat list and the address. Requests made before that
//! wait. Events that change who the user talks to (a friend connected, a group joined, a
//! request arrived) make the codec read the chat list again; at most one such read is in
//! flight, and a change during it asks for one more.
//!
//! What the engine carries and how:
//! - **Replies** quote by item number (`quotedItemId`); **mentions** are `{name: member}`
//!   and the text names them `@name`, or `@'two words'` (the codec quotes, and unquotes
//!   what it receives, so the page always sees `@Name`).
//! - **Edits** (`/_update item`) and **deletion for everyone** (`/_delete item … broadcast`).
//!   With the engine's default "full deletion: off", a friend's copy is marked deleted and
//!   its text stays in their database; the page shows it as deleted.
//! - **Reactions** (`/_reaction`): only [`crate::chat::REACTION_EMOJI`]; ❤️ travels as
//!   U+2764 without the variation selector, as the engine wants it.
//! - **Mute** is the conversation's `enableNtfs` (a group muted keeps "mentions").
//! - **Block a friend**: the engine has no contact block, so EasyPlay keeps it in the
//!   contact's local custom data (`/_set custom`, never sent). The conversation leaves the
//!   list, and what they send while blocked is deleted on arrival (`/_delete item …
//!   internal`, this computer only). They are not told: their messages still reach the
//!   relay and the engine still acknowledges them as it does for everyone.
//! - **Block a group member** (`/_member settings … showMessages`): local; the engine marks
//!   their items "blocked" and the page collapses them.
//! - **Security codes** (`/_get code`, `/_verify code`); a contact is verified while its
//!   connection carries a code.
//! - **Profile**: name and `shortDescr` (the line about the user). The avatar pair is
//!   EasyPlay's own and stays on this computer.
//! - **Files** go through XFTP file servers. With the preset relays the engine's preset
//!   file servers are used; with custom relays only listed `xftp://` servers are, and
//!   without one, files are off (fail closed, as for `/smp`). A picture or GIF from a friend
//!   up to [`crate::chat::MAX_FILE`] is fetched by itself, but only from a file server the
//!   user's own list knows (the engine refuses others: `fileNotApproved`), so a sender
//!   cannot make this computer contact a server of their choosing.
//! - **Offline**: every SMP relay host the engine reported is disconnected.
//! - **Relays that answer.** The engine puts each new queue on a random enabled relay, and
//!   some relays may not answer from this network (no IPv6 route, a relay down). After the
//!   start, and after a network failure (at most once every [`CHECK_EVERY`]), the codec
//!   tests the enabled relays and file servers with the engine's own test (`/_server test`,
//!   run beside the other commands so a timeout holds nothing up), reads the server list
//!   (`/_servers`) and writes it back with the ones that failed disabled and marked
//!   `tested: false`; those are tested again next time and enabled when they answer. It
//!   never disables every server of a kind, nor every server of an operator (then nothing
//!   changes: the network is down, not the relays).
//! - **Retry.** An invitation, an address or a connection that fails with a network error
//!   waits for that check and is sent once more before the failure is reported. Only one
//!   invitation and one address are made at a time (repeats are ignored), and an address the
//!   engine already has (`duplicateContactLink`: a timed-out request made it after all) is
//!   read back with `/_show_address` instead of failing.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::chat::engine::{ChatEvent, FileKind, Request, RequestKind};
use crate::chat::{MAX_FILE, REACTION_EMOJI};
use crate::clock::{MonoInstant, Timestamp};
use crate::error::ErrorCode;
use crate::model::{
    ChatFeatures, ChatGroup, ChatMessage, ChatNotice, ChatPreview, ChatProfile, ChatRef, Delivery,
    FileState, Friend, FriendRequest, GroupMember, Media, Membership, Mention, MessageAuthor,
    MessageBody, Quote, Reaction,
};

/// How far back a chat is read for its latest shown message.
const PREVIEW_DEPTH: u32 = 20;

/// How many conversations the list reads.
pub const CHAT_LIST_COUNT: u32 = 200;

/// GIFs are sent as files picked from the GIF search. On since 2026-11-17: a Giphy GIF
/// downloaded through Signal's content proxy went through [`Request::SendFile`] between two
/// real engines and arrived byte for byte (`tests/chat_e2e.rs` with `EASYPLAY_TEST_GIPHY`;
/// the service part in `tests/chat_service.rs`).
pub const GIFS_SEND_WORKS: bool = true;

/// A 1×1 PNG: the preview the engine requires with a picture. EasyPlay shows the fetched
/// file itself, so no thumbnail is made (core cannot decode GIFs).
const PREVIEW: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

/// The least time between two checks of the relays (a check after a failure waits for it).
pub const CHECK_EVERY: Duration = Duration::from_secs(180);

/// The key under the contact's custom data that EasyPlay owns.
const CUSTOM_KEY: &str = "easyplay";

/// What a command's answer belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Pending {
    ShowUser,
    CreateUser,
    FilePaths,
    Start,
    Relays,
    FileRelays,
    ShowAddress,
    Chats,
    History {
        tag: u64,
        chat: ChatRef,
    },
    /// `/_connect plan`: what a link is, before connecting with it.
    Plan {
        /// The `Connect` request, for a retry.
        request: Request,
        /// This is the retry.
        retried: bool,
    },
    /// A request that reaches a relay (an invitation, an address, a connection): retried
    /// once after a relay check when the network fails.
    Net {
        kind: RequestKind,
        request: Request,
        retried: bool,
    },
    /// `/_get chat` for the latest shown message of a chat (`last`: its latest item).
    Preview {
        chat: ChatRef,
        last: i64,
    },
    /// `/_servers`: the server list, for a check.
    Servers,
    /// `/_server test`: one server of the check.
    ServerTest(String),
    /// `/_servers <json>`: the checked list written back.
    SetServers,
    /// `/freceive`, `/fcancel`: the item with its file now.
    File {
        /// It was a cancel (the engine's answer may still show the offer).
        cancel: bool,
    },
    /// `/_create link`, `/_get link`, `/_delete link` of a group.
    GroupLink {
        group: i64,
        /// Reading it (a missing link is no failure).
        read: bool,
    },
    Req(RequestKind),
    /// `/_get code @friend`.
    Code(i64),
    /// `/_verify code @friend`.
    Verify(i64),
    /// `/_members #group`.
    Members(i64),
    /// `/_member settings #group`: read the members again after.
    MemberBlocked(i64),
    /// An answer nobody waits for (deleting a blocked friend's message, fetching a file).
    Quiet,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Waiting for the helper's `ready`.
    Opening,
    /// Running the start-up commands.
    Starting,
    /// Started.
    Ready,
}

/// What the codec wants done: lines for the helper, events for the service.
#[derive(Debug, Default, PartialEq)]
pub struct Output {
    /// Lines to write to the helper's input, in order.
    pub lines: Vec<String>,
    /// Events to report, in order.
    pub events: Vec<ChatEvent>,
}

/// The SimpleX protocol state of one engine.
#[derive(Debug)]
pub struct Codec {
    name: String,
    smp: Vec<String>,
    xftp: Vec<String>,
    dir: PathBuf,
    phase: Phase,
    user: Option<i64>,
    profile: ChatProfile,
    /// The user's engine profile as last read, so a change keeps what EasyPlay does not set.
    user_profile: Value,
    next: u64,
    pending: HashMap<u64, Pending>,
    queued: Vec<Request>,
    chats_in_flight: bool,
    chats_again: bool,
    /// From the last list: each conversation's settings, each contact's custom data, the
    /// friends blocked, the user's member number in each group.
    settings: HashMap<ChatRef, Value>,
    custom: HashMap<i64, Value>,
    blocked: HashSet<i64>,
    me_in: HashMap<i64, i64>,
    /// Groups whose members were read (read again when they change).
    members_read: HashSet<i64>,
    /// SMP relay hosts the engine reported, and whether each is connected.
    hosts: HashMap<String, bool>,
    offline: bool,
    /// The time, as the engine last told it ([`Codec::tick`]).
    now: Option<MonoInstant>,
    /// The relay check under way, and when the last one started.
    check: Option<Check>,
    last_check: Option<MonoInstant>,
    /// Requests to send again once the check is done.
    after_check: Vec<Request>,
    /// The next rendered request is a retry.
    retrying: bool,
    /// An invitation or an address being made (`CreateInvite`, `Address`).
    links_busy: HashSet<RequestKind>,
    /// The file of each message seen with one (fetching, cancelling, opening it).
    files: RefCell<HashMap<(ChatRef, i64), FileRef>>,
    /// Groups whose invite link was read (owners and admins), and those to read.
    group_links_read: HashSet<i64>,
    links_to_read: Vec<i64>,
    /// The latest shown message of chats whose latest item is one the page does not show,
    /// by that item's number (read once with `/_get chat`), and those to read.
    previews: HashMap<ChatRef, (i64, Option<ChatMessage>)>,
    previews_to_read: Vec<(ChatRef, i64)>,
}

/// A message's file, as the engine knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FileRef {
    /// The engine's file number.
    id: i64,
    /// Where it is on this computer (sent, or received completely).
    local: Option<PathBuf>,
    /// What it is.
    kind: FileKind,
}

/// A relay check under way.
#[derive(Debug)]
struct Check {
    /// The server list as read (`userServers`).
    list: Value,
    /// Tests not answered yet.
    waiting: usize,
    /// Each tested server: whether it answered.
    passed: HashMap<String, bool>,
}

impl Codec {
    /// A codec for a first start under `name` (used only when the database has no user yet)
    /// with `relays` (empty = the engine's presets; `smp://` and `xftp://` addresses), for
    /// the engine's data folder `dir` (files go to `dir/files`).
    pub fn new(name: &str, relays: Vec<String>, dir: &Path) -> Self {
        let (xftp, smp) = relays.into_iter().partition(|r| r.starts_with("xftp://"));
        Self {
            name: name.to_owned(),
            smp,
            xftp,
            dir: dir.to_owned(),
            phase: Phase::Opening,
            user: None,
            profile: ChatProfile::default(),
            user_profile: Value::Null,
            next: 1,
            pending: HashMap::new(),
            queued: Vec::new(),
            chats_in_flight: false,
            chats_again: false,
            settings: HashMap::new(),
            custom: HashMap::new(),
            blocked: HashSet::new(),
            me_in: HashMap::new(),
            members_read: HashSet::new(),
            hosts: HashMap::new(),
            offline: false,
            now: None,
            check: None,
            last_check: None,
            after_check: Vec::new(),
            retrying: false,
            links_busy: HashSet::new(),
            files: RefCell::new(HashMap::new()),
            group_links_read: HashSet::new(),
            links_to_read: Vec::new(),
            previews: HashMap::new(),
            previews_to_read: Vec::new(),
        }
    }

    /// The time now (the engine calls this before each request and line; tests set it).
    pub fn tick(&mut self, now: MonoInstant) {
        self.now = Some(now);
    }

    /// Whether start-up finished.
    pub fn started(&self) -> bool {
        self.phase == Phase::Ready
    }

    /// Where received files go.
    pub fn files_dir(&self) -> PathBuf {
        self.dir.join("files")
    }

    /// Files can be sent and received: the preset file servers go with the preset relays;
    /// custom relays need a custom file server too.
    fn files_on(&self) -> bool {
        self.smp.is_empty() || !self.xftp.is_empty()
    }

    /// What this engine carries.
    pub fn features(&self) -> ChatFeatures {
        features_for(self.files_on())
    }

    /// A request from the service.
    pub fn request(&mut self, request: Request) -> Output {
        let mut out = Output::default();
        if self.phase == Phase::Ready {
            self.render(request, &mut out);
        } else {
            self.queued.push(request);
        }
        out
    }

    /// One line from the helper's output.
    pub fn on_line(&mut self, line: &str) -> Output {
        let mut out = Output::default();
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            tracing::warn!(len = line.len(), "chat: a line from the helper is not JSON");
            return out;
        };
        match v.get("t").and_then(Value::as_str) {
            Some("ready") => {
                self.phase = Phase::Starting;
                self.send(&mut out, Pending::ShowUser, "/user".to_owned());
            }
            Some("resp") => {
                let corr = v.get("corr").and_then(Value::as_u64).unwrap_or(0);
                let body = v.get("body").cloned().unwrap_or(Value::Null);
                match self.pending.remove(&corr) {
                    Some(p) => self.on_response(p, &body, &mut out),
                    None => tracing::warn!(corr, "chat: an answer to no command"),
                }
            }
            Some("event") => {
                let body = v.get("body").cloned().unwrap_or(Value::Null);
                if let Some(r) = body.get("result") {
                    self.on_event(r, &mut out);
                } else if let Some(e) = body.get("error") {
                    tracing::info!(error = %short(e), "chat: the engine reported an error");
                }
            }
            Some("init_error") => {
                let detail = v.get("body").map(short).unwrap_or_default();
                out.events
                    .push(failed(RequestKind::Start, ErrorCode::ChtDatabase, detail));
            }
            Some("fatal") => {
                let detail = v
                    .get("msg")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                out.events.push(failed(
                    RequestKind::Start,
                    ErrorCode::ChtUnavailable,
                    detail,
                ));
            }
            Some(other) => tracing::warn!(kind = other, "chat: unexpected line from the helper"),
            None => tracing::warn!("chat: a line from the helper without a kind"),
        }
        out
    }

    // ---- commands -------------------------------------------------------------------------

    fn send(&mut self, out: &mut Output, pending: Pending, cmd: String) {
        let corr = self.next;
        self.next += 1;
        self.pending.insert(corr, pending);
        out.lines
            .push(serde_json::json!({ "corr": corr, "cmd": cmd }).to_string());
    }

    /// A command run beside the others (its answer may come after later ones).
    fn send_bg(&mut self, out: &mut Output, pending: Pending, cmd: String) {
        let corr = self.next;
        self.next += 1;
        self.pending.insert(corr, pending);
        out.lines
            .push(serde_json::json!({ "corr": corr, "cmd": cmd, "bg": true }).to_string());
    }

    fn uid(&self) -> i64 {
        self.user.unwrap_or(1)
    }

    fn refresh(&mut self, out: &mut Output) {
        if self.chats_in_flight {
            self.chats_again = true;
            return;
        }
        self.chats_in_flight = true;
        let cmd = format!(
            "/_get chats {} count={CHAT_LIST_COUNT} {}",
            self.uid(),
            serde_json::json!({ "type": "filters", "favorite": false, "unread": false })
        );
        self.send(out, Pending::Chats, cmd);
    }

    #[allow(clippy::too_many_lines)]
    fn render(&mut self, request: Request, out: &mut Output) {
        let uid = self.uid();
        let retried = std::mem::take(&mut self.retrying);
        let (kind, cmd) = match request {
            Request::Refresh => {
                self.refresh(out);
                return;
            }
            Request::History {
                chat,
                before,
                count,
                tag,
            } => {
                let page = match before {
                    Some(id) => format!("before={id} count={count}"),
                    None => format!("count={count}"),
                };
                let cmd = format!("/_get chat {} {page}", chat_ref(chat));
                self.send(out, Pending::History { tag, chat }, cmd);
                return;
            }
            Request::SetProfile(p) => (
                RequestKind::SetProfile,
                format!("/_profile {uid} {}", self.profile_json(&p)),
            ),
            Request::CreateInvite | Request::CreateAddress => {
                let (kind, cmd) = if request == Request::CreateInvite {
                    (RequestKind::CreateInvite, format!("/_connect {uid}"))
                } else {
                    (RequestKind::Address, format!("/_address {uid}"))
                };
                if !retried && !self.links_busy.insert(kind) {
                    tracing::debug!(?kind, "chat: already being made");
                    return;
                }
                let pending = Pending::Net {
                    kind,
                    request,
                    retried,
                };
                self.send(out, pending, cmd);
                return;
            }
            Request::Connect(link) => {
                let link = link.trim().to_owned();
                if link.is_empty() || link.chars().any(char::is_whitespace) {
                    out.events.push(failed(
                        RequestKind::Connect,
                        ErrorCode::ChtBadLink,
                        "empty or contains spaces".to_owned(),
                    ));
                    return;
                }
                let cmd = format!("/_connect plan {uid} {link}");
                let pending = Pending::Plan {
                    request: Request::Connect(link),
                    retried,
                };
                self.send(out, pending, cmd);
                return;
            }
            Request::Accept(id) => (RequestKind::Answer, format!("/_accept {id}")),
            Request::Reject(id) => (RequestKind::Answer, format!("/_reject {id}")),
            Request::DeleteAddress => (RequestKind::Address, format!("/_delete_address {uid}")),
            Request::Send {
                chat,
                text,
                reply_to,
                mentions,
            } => {
                let mentions = match chat {
                    ChatRef::Group(_) => mentions,
                    ChatRef::Direct(_) => Vec::new(),
                };
                let mut m = serde_json::json!({
                    "msgContent": { "type": "text", "text": quote_mentions(&text, &mentions) },
                    "mentions": mention_map(&mentions),
                });
                if let Some(q) = reply_to {
                    m["quotedItemId"] = q.into();
                }
                (
                    RequestKind::Send,
                    format!("/_send {} json {}", chat_ref(chat), Value::Array(vec![m])),
                )
            }
            Request::SendFile {
                chat,
                path,
                kind,
                preview,
                duration,
            } => {
                if !self.files_on() {
                    out.events.push(failed(
                        RequestKind::SendFile,
                        ErrorCode::ChtRequestFailed,
                        "files are off: custom relays without a file server".to_owned(),
                    ));
                    return;
                }
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let image = preview
                    .as_deref()
                    .map_or_else(|| PREVIEW.to_owned(), data_uri);
                // A GIF goes as a picture: the engine's apps show it as one, and EasyPlay
                // draws the fetched file.
                let mc = match kind {
                    FileKind::Image | FileKind::Gif => {
                        serde_json::json!({ "type": "image", "text": "", "image": image })
                    }
                    FileKind::Video => serde_json::json!({
                        "type": "video", "text": "", "image": image,
                        "duration": duration.unwrap_or(0),
                    }),
                    FileKind::File => serde_json::json!({ "type": "file", "text": "" }),
                };
                let m = serde_json::json!([{
                    "fileSource": { "filePath": path },
                    "msgContent": mc,
                    "mentions": {},
                }]);
                tracing::debug!(name, "chat: sending a file");
                (
                    RequestKind::SendFile,
                    format!("/_send {} json {m}", chat_ref(chat)),
                )
            }
            Request::ReceiveFile { chat, item } | Request::CancelFile { chat, item } => {
                let cancel = matches!(request, Request::CancelFile { .. });
                let Some(f) = self.files.borrow().get(&(chat, item)).cloned() else {
                    out.events.push(failed(
                        RequestKind::File,
                        ErrorCode::ChtRequestFailed,
                        "no file known for that message".to_owned(),
                    ));
                    return;
                };
                let cmd = if cancel {
                    format!("/fcancel {}", f.id)
                } else {
                    format!("/freceive {} encrypt=off", f.id)
                };
                self.send(out, Pending::File { cancel }, cmd);
                return;
            }
            Request::OpenFile { chat, item } => {
                let f = self.files.borrow().get(&(chat, item)).cloned();
                match f.and_then(|f| Some((f.local?, f.kind))) {
                    Some((path, kind)) => out.events.push(ChatEvent::OpenFile { path, kind }),
                    None => out.events.push(failed(
                        RequestKind::File,
                        ErrorCode::ChtRequestFailed,
                        "that file is not on this computer".to_owned(),
                    )),
                }
                return;
            }
            Request::CreateGroupLink(group) => {
                let pending = Pending::GroupLink { group, read: false };
                self.send(out, pending, format!("/_create link #{group} member"));
                return;
            }
            Request::DeleteGroupLink(group) => {
                let pending = Pending::GroupLink { group, read: false };
                self.send(out, pending, format!("/_delete link #{group}"));
                return;
            }
            Request::Edit { chat, item, text } => {
                let m = serde_json::json!({
                    "msgContent": { "type": "text", "text": text },
                    "mentions": {},
                });
                (
                    RequestKind::Edit,
                    format!("/_update item {} {item} json {m}", chat_ref(chat)),
                )
            }
            Request::Delete { chat, item } => (
                RequestKind::Delete,
                format!("/_delete item {} {item} broadcast", chat_ref(chat)),
            ),
            Request::React {
                chat,
                item,
                emoji,
                on,
            } => {
                let Some(e) = engine_emoji(&emoji) else {
                    out.events.push(failed(
                        RequestKind::React,
                        ErrorCode::ChtRequestFailed,
                        format!("not a reaction the engine carries: {emoji}"),
                    ));
                    return;
                };
                let r = serde_json::json!({ "type": "emoji", "emoji": e });
                (
                    RequestKind::React,
                    format!(
                        "/_reaction {} {item} {} {r}",
                        chat_ref(chat),
                        if on { "on" } else { "off" }
                    ),
                )
            }
            Request::SetMuted { chat, on } => {
                let mut s = self
                    .settings
                    .get(&chat)
                    .cloned()
                    .filter(Value::is_object)
                    .unwrap_or_else(|| serde_json::json!({ "favorite": false }));
                let ntfs = match (on, chat) {
                    (false, _) => "all",
                    (true, ChatRef::Direct(_)) => "none",
                    // A muted group still notifies the user's mentions.
                    (true, ChatRef::Group(_)) => "mentions",
                };
                s["enableNtfs"] = ntfs.into();
                self.settings.insert(chat, s.clone());
                (
                    RequestKind::Mute,
                    format!("/_settings {} {s}", chat_ref(chat)),
                )
            }
            Request::SetBlocked { friend, on } => {
                let mut data = self
                    .custom
                    .get(&friend)
                    .cloned()
                    .filter(Value::is_object)
                    .unwrap_or_else(|| serde_json::json!({}));
                if let Some(map) = data.as_object_mut() {
                    if on {
                        map.insert(CUSTOM_KEY.into(), serde_json::json!({ "blocked": true }));
                    } else {
                        map.remove(CUSTOM_KEY);
                    }
                }
                if on {
                    self.blocked.insert(friend);
                } else {
                    self.blocked.remove(&friend);
                }
                let empty = data.as_object().is_none_or(serde_json::Map::is_empty);
                self.custom.insert(friend, data.clone());
                let cmd = if empty {
                    format!("/_set custom @{friend}")
                } else {
                    format!("/_set custom @{friend} {data}")
                };
                (RequestKind::Block, cmd)
            }
            Request::SetMemberBlocked { group, member, on } => {
                let s = serde_json::json!({ "showMessages": !on });
                let cmd = format!("/_member settings #{group} {member} {s}");
                self.send(out, Pending::MemberBlocked(group), cmd);
                return;
            }
            Request::ShowCode(friend) => {
                self.send(out, Pending::Code(friend), format!("/_get code @{friend}"));
                return;
            }
            Request::Verify { friend, code } => {
                let code: String = code
                    .chars()
                    .filter(|c| c.is_ascii_digit() || *c == ' ')
                    .collect();
                let cmd = format!("/_verify code @{friend} {}", code.trim());
                self.send(out, Pending::Verify(friend), cmd);
                return;
            }
            Request::Members(group) => {
                self.members_read.insert(group);
                self.send(out, Pending::Members(group), format!("/_members #{group}"));
                return;
            }
            Request::MarkRead(chat) => (
                RequestKind::MarkRead,
                format!("/_read chat {}", chat_ref(chat)),
            ),
            Request::NewGroup(name) => (
                RequestKind::Group,
                format!("/_group {uid} {}", group_profile_json(&name)),
            ),
            Request::AddMember { group, friend } => (
                RequestKind::Group,
                format!("/_add #{group} {friend} member"),
            ),
            Request::JoinGroup(g) => (RequestKind::Group, format!("/_join #{g}")),
            Request::LeaveGroup(g) => (RequestKind::Group, format!("/_leave #{g}")),
            // `full` (the friend is told); the mode is a word, not JSON (APIDeleteChat).
            Request::RemoveFriend(c) => (RequestKind::RemoveFriend, format!("/_delete @{c} full")),
        };
        self.send(out, Pending::Req(kind), cmd);
    }

    /// The user's engine profile with `p`'s name and line about them; everything else the
    /// engine had (preferences, picture) kept.
    fn profile_json(&self, p: &ChatProfile) -> Value {
        let mut v = serde_json::json!({});
        for k in [
            "displayName",
            "fullName",
            "shortDescr",
            "description",
            "image",
            "contactLink",
            "preferences",
            "peerType",
        ] {
            if let Some(x) = self.user_profile.get(k).filter(|x| !x.is_null()) {
                v[k] = x.clone();
            }
        }
        v["displayName"] = p.name.trim().into();
        if v.get("fullName").is_none() {
            v["fullName"] = "".into();
        }
        let about = p.about.trim();
        if about.is_empty() {
            if let Some(m) = v.as_object_mut() {
                m.remove("shortDescr");
            }
        } else {
            v["shortDescr"] = about.into();
        }
        v
    }

    // ---- answers --------------------------------------------------------------------------

    #[allow(clippy::too_many_lines)]
    fn on_response(&mut self, pending: Pending, body: &Value, out: &mut Output) {
        let result = body.get("result");
        let error = body.get("error");
        let ty = result.and_then(|r| r.get("type")).and_then(Value::as_str);
        let start_failed = |out: &mut Output| {
            out.events.push(failed(
                RequestKind::Start,
                ErrorCode::ChtRequestFailed,
                error.map(short).unwrap_or_default(),
            ));
        };
        match pending {
            Pending::ShowUser => match (ty, result) {
                (Some("activeUser"), Some(r)) => self.on_user(r, out),
                _ => {
                    let cmd = format!(
                        "/_create user {}",
                        serde_json::json!({
                            "profile": { "displayName": self.name, "fullName": "" },
                            "pastTimestamp": false,
                        })
                    );
                    self.send(out, Pending::CreateUser, cmd);
                }
            },
            Pending::CreateUser => match (ty, result) {
                (Some("activeUser"), Some(r)) => self.on_user(r, out),
                _ => start_failed(out),
            },
            Pending::FilePaths => {
                if error.is_some() {
                    start_failed(out);
                } else {
                    self.send(out, Pending::Start, "/_start".to_owned());
                }
            }
            Pending::Start => match ty {
                Some("chatStarted" | "chatRunning") => self.set_relays(out),
                _ => start_failed(out),
            },
            Pending::Relays => {
                if error.is_some() {
                    // Fail closed: the user chose these relays, so never fall back to the
                    // presets.
                    start_failed(out);
                } else if self.xftp.is_empty() {
                    self.become_ready(out);
                } else {
                    let cmd = format!("/xftp {}", self.xftp.join(" "));
                    self.send(out, Pending::FileRelays, cmd);
                }
            }
            Pending::FileRelays => {
                if error.is_some() {
                    start_failed(out);
                } else {
                    self.become_ready(out);
                }
            }
            Pending::ShowAddress => {
                let link = result
                    .filter(|_| ty == Some("userContactLink"))
                    .and_then(|r| r.get("contactLink"))
                    .and_then(|l| l.get("connLinkContact"))
                    .and_then(link_text);
                out.events.push(ChatEvent::Address(link));
            }
            Pending::Chats => {
                self.chats_in_flight = false;
                if let (Some("apiChats"), Some(r)) = (ty, result) {
                    let list = self.parse_chats(r);
                    out.events.push(list);
                    // The latest shown message of chats whose latest item is hidden.
                    for (chat, last) in std::mem::take(&mut self.previews_to_read) {
                        let pending = Pending::Preview { chat, last };
                        let cmd = format!("/_get chat {} count={PREVIEW_DEPTH}", chat_ref(chat));
                        self.send(out, pending, cmd);
                    }
                    // The invite link of each group the user runs, once per start.
                    for g in std::mem::take(&mut self.links_to_read) {
                        if self.group_links_read.insert(g) {
                            let pending = Pending::GroupLink {
                                group: g,
                                read: true,
                            };
                            self.send(out, pending, format!("/_get link #{g}"));
                        }
                    }
                } else {
                    out.events.push(failed(
                        RequestKind::Refresh,
                        ErrorCode::ChtRequestFailed,
                        error.map(short).unwrap_or_default(),
                    ));
                }
                if std::mem::take(&mut self.chats_again) {
                    self.refresh(out);
                }
            }
            Pending::History { tag, chat } => {
                let c = result
                    .filter(|_| ty == Some("apiChat"))
                    .and_then(|r| r.get("chat"));
                let items = c.and_then(|c| c.get("chatItems")).and_then(Value::as_array);
                match items {
                    Some(items) => {
                        let ctx = self.ctx(c.and_then(|c| c.get("chatInfo")), chat);
                        let messages = items
                            .iter()
                            .filter_map(|ci| parse_item(chat, ci, &ctx))
                            .collect();
                        out.events.push(ChatEvent::History {
                            tag,
                            chat,
                            messages,
                        });
                    }
                    None => out.events.push(failed(
                        RequestKind::History,
                        ErrorCode::ChtRequestFailed,
                        error.map(short).unwrap_or_default(),
                    )),
                }
            }
            Pending::Plan { request, retried } => {
                if let Some(e) = error.filter(|_| ty != Some("connectionPlan")) {
                    return self.on_net_error(RequestKind::Connect, request, retried, e, out);
                }
                self.on_plan(result, ty, request, retried, out);
            }
            Pending::Net {
                kind,
                request,
                retried,
            } => {
                if let Some(e) = error {
                    return self.on_net_error(kind, request, retried, e, out);
                }
                self.links_busy.remove(&kind);
                self.on_request_done(kind, ty, result, out);
            }
            Pending::Servers => {
                match result
                    .filter(|_| ty == Some("userServers"))
                    .and_then(|r| r.get("userServers"))
                {
                    Some(list) => self.test_servers(list.clone(), out),
                    None => {
                        tracing::info!(error = %error.map(short).unwrap_or_default(), "chat: the server list was not read");
                        self.check_done(out);
                    }
                }
            }
            Pending::ServerTest(server) => {
                let passed = ty == Some("serverTestResult")
                    && result
                        .and_then(|r| r.get("testFailure"))
                        .is_none_or(Value::is_null);
                if !passed {
                    let why = result
                        .and_then(|r| r.get("testFailure"))
                        .or(error)
                        .map(short)
                        .unwrap_or_default();
                    tracing::info!(server = %host_of(&server), why, "chat: a relay did not answer its test");
                }
                if let Some(c) = &mut self.check {
                    c.passed.insert(server, passed);
                    c.waiting = c.waiting.saturating_sub(1);
                    if c.waiting == 0 {
                        self.apply_check(out);
                    }
                }
            }
            Pending::SetServers => {
                if let Some(e) = error {
                    tracing::info!(error = %short(e), "chat: the checked server list was refused");
                }
                self.check_done(out);
            }
            Pending::Code(friend) => {
                match result
                    .filter(|_| ty == Some("contactCode"))
                    .and_then(|r| r.get("connectionCode"))
                    .and_then(Value::as_str)
                {
                    Some(code) => out.events.push(ChatEvent::Code {
                        friend,
                        code: code.to_owned(),
                    }),
                    None => out.events.push(failed(
                        RequestKind::Code,
                        ErrorCode::ChtRequestFailed,
                        error.map(short).unwrap_or_default(),
                    )),
                }
            }
            Pending::Verify(friend) => {
                match result
                    .filter(|_| ty == Some("connectionVerified"))
                    .and_then(|r| r.get("verified"))
                    .and_then(Value::as_bool)
                {
                    Some(matched) => {
                        out.events.push(ChatEvent::Verified { friend, matched });
                        self.refresh(out);
                    }
                    None => out.events.push(failed(
                        RequestKind::Code,
                        ErrorCode::ChtRequestFailed,
                        error.map(short).unwrap_or_default(),
                    )),
                }
            }
            Pending::Members(group) => {
                match result
                    .filter(|_| ty == Some("groupMembers"))
                    .and_then(|r| r.get("group"))
                {
                    Some(g) => out.events.push(ChatEvent::Members {
                        group,
                        members: parse_members(g),
                    }),
                    None => out.events.push(failed(
                        RequestKind::Members,
                        ErrorCode::ChtRequestFailed,
                        error.map(short).unwrap_or_default(),
                    )),
                }
            }
            Pending::MemberBlocked(group) => {
                if let Some(e) = error {
                    out.events.push(failed(
                        RequestKind::Block,
                        error_code(RequestKind::Block, e),
                        short(e),
                    ));
                } else {
                    self.members_read.insert(group);
                    self.send(out, Pending::Members(group), format!("/_members #{group}"));
                }
            }
            Pending::File { cancel } => {
                if let Some(e) = error {
                    out.events.push(failed(
                        RequestKind::File,
                        error_code(RequestKind::File, e),
                        short(e),
                    ));
                    return;
                }
                let m = result
                    .and_then(|r| r.get("chatItem").or_else(|| r.get("chatItem_")))
                    .filter(|a| !a.is_null())
                    .and_then(|a| self.parse_a_item(a));
                if let Some(mut m) = m {
                    if cancel
                        && let MessageBody::Image(x) | MessageBody::Video(x) | MessageBody::File(x) =
                            &mut m.body
                    {
                        x.state = FileState::Cancelled;
                    }
                    out.events.push(ChatEvent::Updated(vec![m]));
                }
            }
            Pending::GroupLink { group, read } => match (ty, result) {
                (Some("groupLinkCreated" | "groupLink"), Some(r)) => {
                    let link = r
                        .get("groupLink")
                        .and_then(|l| l.get("connLinkContact"))
                        .and_then(link_text);
                    out.events.push(ChatEvent::GroupLink { group, link });
                }
                (Some("groupLinkDeleted"), _) => {
                    out.events.push(ChatEvent::GroupLink { group, link: None });
                }
                _ => {
                    let e = error.cloned().unwrap_or(Value::Null);
                    if read || e.to_string().contains("groupLinkNotFound") {
                        out.events.push(ChatEvent::GroupLink { group, link: None });
                    } else {
                        out.events.push(failed(
                            RequestKind::GroupLink,
                            error_code(RequestKind::GroupLink, &e),
                            short(&e),
                        ));
                    }
                }
            },
            Pending::Preview { chat, last } => {
                let found = result
                    .filter(|_| ty == Some("apiChat"))
                    .and_then(|r| r.get("chat"));
                let Some(c) = found else {
                    tracing::info!(error = %error.map(short).unwrap_or_default(), "chat: a preview was not read");
                    return;
                };
                let ctx = self.ctx(c.get("chatInfo"), chat);
                let shown = c
                    .get("chatItems")
                    .and_then(Value::as_array)
                    .and_then(|items| items.iter().rev().find_map(|ci| parse_item(chat, ci, &ctx)));
                self.previews.insert(chat, (last, shown.clone()));
                if let Some(m) = shown {
                    out.events.push(ChatEvent::Preview { chat, last: m });
                }
            }
            Pending::Quiet => {
                if let Some(e) = error {
                    tracing::info!(error = %short(e), "chat: a background command failed");
                }
            }
            Pending::Req(kind) => {
                if let Some(e) = error {
                    let code = error_code(kind, e);
                    out.events.push(failed(kind, code, short(e)));
                    if code == ErrorCode::ChtNoConnection {
                        self.start_check(out);
                    }
                    return;
                }
                self.on_request_done(kind, ty, result, out);
            }
        }
    }

    fn on_plan(
        &mut self,
        result: Option<&Value>,
        ty: Option<&str>,
        request: Request,
        retried: bool,
        out: &mut Output,
    ) {
        let plan = result.filter(|_| ty == Some("connectionPlan"));
        let Some(plan) = plan else {
            out.events.push(failed(
                RequestKind::Connect,
                ErrorCode::ChtBadLink,
                result.map(short).unwrap_or_default(),
            ));
            return;
        };
        let p = plan.get("connectionPlan").unwrap_or(&Value::Null);
        let kind = p.get("type").and_then(Value::as_str).unwrap_or("");
        let sub = ["invitationLinkPlan", "contactAddressPlan", "groupLinkPlan"]
            .iter()
            .find_map(|k| p.get(*k))
            .and_then(|s| s.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let link = plan.get("connLink").and_then(|l| {
            let full = l.get("connFullLink")?.as_str()?;
            Some(match l.get("connShortLink").and_then(Value::as_str) {
                Some(s) => format!("{full} {s}"),
                None => full.to_owned(),
            })
        });
        match (kind, sub, link) {
            ("error", _, _) | (_, _, None) => out.events.push(failed(
                RequestKind::Connect,
                ErrorCode::ChtBadLink,
                short(p),
            )),
            (_, "ownLink", _) => out.events.push(failed(
                RequestKind::Connect,
                ErrorCode::ChtBadLink,
                "the user's own link".to_owned(),
            )),
            (_, "known" | "connecting" | "connectingProhibit", _) => self.refresh(out),
            (_, _, Some(link)) => {
                let uid = self.uid();
                let cmd = format!("/_connect {uid} {link}");
                let pending = Pending::Net {
                    kind: RequestKind::Connect,
                    request,
                    retried,
                };
                self.send(out, pending, cmd);
            }
        }
    }

    fn on_user(&mut self, r: &Value, out: &mut Output) {
        let user = r.get("user");
        self.user = user.and_then(|u| u.get("userId")).and_then(Value::as_i64);
        let profile = user
            .and_then(|u| u.get("profile"))
            .cloned()
            .unwrap_or(Value::Null);
        self.profile = ChatProfile {
            name: profile
                .get("displayName")
                .and_then(Value::as_str)
                .unwrap_or(&self.name)
                .to_owned(),
            about: profile
                .get("shortDescr")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            ..ChatProfile::default()
        };
        self.user_profile = profile;
        let paths = serde_json::json!({
            "appFilesFolder": self.files_dir(),
            "appTempFolder": self.dir.join("tmp"),
            "appAssetsFolder": self.dir.join("assets"),
        });
        self.send(out, Pending::FilePaths, format!("/set file paths {paths}"));
    }

    fn set_relays(&mut self, out: &mut Output) {
        if !self.smp.is_empty() {
            let cmd = format!("/smp {}", self.smp.join(" "));
            self.send(out, Pending::Relays, cmd);
        } else if !self.xftp.is_empty() {
            let cmd = format!("/xftp {}", self.xftp.join(" "));
            self.send(out, Pending::FileRelays, cmd);
        } else {
            self.become_ready(out);
        }
    }

    fn become_ready(&mut self, out: &mut Output) {
        self.phase = Phase::Ready;
        out.events.push(ChatEvent::Features {
            features: self.features(),
            reactions: Some(REACTION_EMOJI.iter().map(|e| (*e).to_owned()).collect()),
        });
        out.events.push(ChatEvent::Ready(self.profile.clone()));
        self.refresh(out);
        let uid = self.uid();
        self.send(out, Pending::ShowAddress, format!("/_show_address {uid}"));
        for r in std::mem::take(&mut self.queued) {
            self.render(r, out);
        }
        self.start_check(out);
    }

    // ---- relays that answer, retries ----------------------------------------------------

    /// A request that reaches a relay failed. A network failure, the first time: check the
    /// relays and send it again. An address the engine already has: read it back.
    fn on_net_error(
        &mut self,
        kind: RequestKind,
        request: Request,
        retried: bool,
        e: &Value,
        out: &mut Output,
    ) {
        let code = error_code(kind, e);
        if code == ErrorCode::ChtNoConnection && !retried {
            tracing::info!(?kind, error = %short(e), "chat: a relay did not answer; checking the relays, then once more");
            self.after_check.push(request);
            if !self.start_check(out) && self.check.is_none() {
                // Checked a moment ago: just send it again.
                self.flush_retries(out);
            }
            return;
        }
        self.links_busy.remove(&kind);
        if kind == RequestKind::Address && e.to_string().contains("duplicateContactLink") {
            // A request that timed out made it after all.
            let uid = self.uid();
            self.send(out, Pending::ShowAddress, format!("/_show_address {uid}"));
            return;
        }
        out.events.push(failed(kind, code, short(e)));
        if code == ErrorCode::ChtNoConnection {
            self.start_check(out);
        }
    }

    /// Start a relay check unless one runs or one started less than [`CHECK_EVERY`] ago.
    /// Whether one started.
    fn start_check(&mut self, out: &mut Output) -> bool {
        if self.check.is_some() {
            return false;
        }
        let recent = match (self.last_check, self.now) {
            (Some(last), Some(now)) => now.saturating_duration_since(last) < CHECK_EVERY,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if recent {
            return false;
        }
        self.last_check = self.now.or(self.last_check);
        self.check = Some(Check {
            list: Value::Null,
            waiting: 0,
            passed: HashMap::new(),
        });
        let uid = self.uid();
        self.send(out, Pending::Servers, format!("/_servers {uid}"));
        true
    }

    /// Test every enabled server, and those a check disabled before.
    fn test_servers(&mut self, list: Value, out: &mut Output) {
        let servers: Vec<String> = candidates(&list, !self.smp.is_empty())
            .into_iter()
            .map(|(_, _, _, s)| s)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        if servers.is_empty() {
            return self.check_done(out);
        }
        if let Some(c) = &mut self.check {
            c.list = list;
            c.waiting = servers.len();
        }
        let uid = self.uid();
        for s in servers {
            let cmd = format!("/_server test {uid} {s}");
            self.send_bg(out, Pending::ServerTest(s), cmd);
        }
    }

    /// Every test answered: disable what failed and enable what recovered, unless that
    /// would leave a kind of server, or an operator's servers of a kind, with none.
    fn apply_check(&mut self, out: &mut Output) {
        let Some(c) = self.check.as_mut() else { return };
        let mut list = c.list.clone();
        let found = candidates(&list, !self.smp.is_empty());
        let mut changed = false;
        for key in ["smpServers", "xftpServers"] {
            let of_kind: Vec<_> = found.iter().filter(|f| f.1 == key).collect();
            if !of_kind.iter().any(|f| c.passed.get(&f.3) == Some(&true)) {
                continue;
            }
            let ops: HashSet<usize> = of_kind.iter().map(|f| f.0).collect();
            for op in ops {
                let in_op: Vec<_> = of_kind.iter().filter(|f| f.0 == op).collect();
                if !in_op.iter().any(|f| c.passed.get(&f.3) == Some(&true)) {
                    continue;
                }
                for f in in_op {
                    let ok = c.passed.get(&f.3) == Some(&true);
                    let s = &mut list[f.0][f.1][f.2];
                    if s.get("enabled").and_then(Value::as_bool) != Some(ok) {
                        changed = true;
                        tracing::info!(server = %host_of(&f.3), enabled = ok, "chat: relay");
                    }
                    s["enabled"] = ok.into();
                    s["tested"] = ok.into();
                }
            }
        }
        if changed {
            let uid = self.uid();
            self.send(out, Pending::SetServers, format!("/_servers {uid} {list}"));
        } else {
            self.check_done(out);
        }
    }

    fn check_done(&mut self, out: &mut Output) {
        self.check = None;
        self.flush_retries(out);
    }

    fn flush_retries(&mut self, out: &mut Output) {
        for r in std::mem::take(&mut self.after_check) {
            self.retrying = true;
            self.render(r, out);
        }
    }

    fn on_request_done(
        &mut self,
        kind: RequestKind,
        ty: Option<&str>,
        result: Option<&Value>,
        out: &mut Output,
    ) {
        let Some(r) = result else { return };
        match (kind, ty) {
            (RequestKind::CreateInvite, Some("invitation")) => {
                if let Some(link) = r.get("connLinkInvitation").and_then(link_text) {
                    out.events.push(ChatEvent::Invite(link));
                }
            }
            (RequestKind::Address, Some("userContactLinkCreated")) => {
                let link = r.get("connLinkContact").and_then(link_text);
                out.events.push(ChatEvent::Address(link));
            }
            (RequestKind::Address, Some("userContactLinkDeleted")) => {
                out.events.push(ChatEvent::Address(None));
            }
            (RequestKind::SetProfile, Some("userProfileUpdated")) => {
                if let Some(p) = r.get("toProfile") {
                    if let Some(name) = p.get("displayName").and_then(Value::as_str) {
                        self.profile.name = name.to_owned();
                    }
                    self.profile.about = p
                        .get("shortDescr")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned();
                    self.user_profile = p.clone();
                    out.events.push(ChatEvent::Profile(self.profile.clone()));
                }
            }
            (RequestKind::Send | RequestKind::SendFile, Some("newChatItems")) => {
                let m = self.parse_items(r);
                if !m.is_empty() {
                    out.events.push(ChatEvent::Messages(m));
                }
            }
            (RequestKind::Edit, Some("chatItemUpdated")) => {
                if let Some(m) = r.get("chatItem").and_then(|a| self.parse_a_item(a)) {
                    out.events.push(ChatEvent::Updated(vec![m]));
                }
            }
            (RequestKind::Delete, Some("chatItemsDeleted")) => {
                let m = self.parse_deletions(r);
                if !m.is_empty() {
                    out.events.push(ChatEvent::Updated(m));
                }
            }
            (RequestKind::React, Some("chatItemReaction")) => {
                if let Some(m) = self.parse_reaction(r) {
                    out.events.push(ChatEvent::Updated(vec![m]));
                }
            }
            (RequestKind::MarkRead | RequestKind::SetProfile, _) => {}
            (
                RequestKind::Connect
                | RequestKind::Answer
                | RequestKind::Group
                | RequestKind::RemoveFriend
                | RequestKind::Mute
                | RequestKind::Block,
                _,
            ) => self.refresh(out),
            (k, t) => tracing::debug!(?k, ?t, "chat: answer not used"),
        }
    }

    // ---- events ---------------------------------------------------------------------------

    fn on_event(&mut self, r: &Value, out: &mut Output) {
        let ty = r.get("type").and_then(Value::as_str).unwrap_or("");
        match ty {
            "newChatItems" => self.on_new_items(r, out),
            "chatItemsStatusesUpdated" => {
                let m = self.parse_items(r);
                if !m.is_empty() {
                    out.events.push(ChatEvent::Updated(m));
                }
            }
            "chatItemUpdated"
            | "rcvFileComplete"
            | "sndFileCompleteXFTP"
            | "rcvFileAccepted"
            | "rcvFileDescrReady"
            | "rcvFileStart"
            | "rcvFileProgressXFTP"
            | "sndFileProgressXFTP"
            | "rcvFileSndCancelled"
            | "rcvFileError"
            | "rcvFileWarning"
            | "sndFileError"
            | "sndFileWarning" => {
                if let Some(m) = r
                    .get("chatItem")
                    .or_else(|| r.get("chatItem_"))
                    .filter(|a| !a.is_null())
                    .and_then(|a| self.parse_a_item(a))
                {
                    out.events.push(ChatEvent::Updated(vec![m]));
                }
            }
            "chatItemsDeleted" => {
                let m = self.parse_deletions(r);
                if !m.is_empty() {
                    out.events.push(ChatEvent::Updated(m));
                }
            }
            "chatItemReaction" => {
                if let Some(m) = self.parse_reaction(r) {
                    out.events.push(ChatEvent::Updated(vec![m]));
                }
            }
            "hostConnected" | "hostDisconnected" => {
                if r.get("protocol").and_then(Value::as_str) == Some("smp")
                    && let Some(host) = r.get("transportHost").and_then(Value::as_str)
                {
                    self.hosts.insert(host.to_owned(), ty == "hostConnected");
                    let offline = !self.hosts.values().any(|up| *up);
                    if offline != self.offline {
                        self.offline = offline;
                        out.events.push(ChatEvent::Offline(offline));
                    }
                }
            }
            "contactConnected"
            | "contactUpdated"
            | "contactDeletedByContact"
            | "contactSndReady"
            | "receivedContactRequest"
            | "receivedGroupInvitation"
            | "userJoinedGroup"
            | "groupUpdated"
            | "joinedGroupMember"
            | "deletedMember"
            | "leftMember"
            | "deletedMemberUser"
            | "groupDeleted"
            | "connectedToGroupMember"
            | "memberRole"
            | "groupMemberUpdated"
                if self.phase == Phase::Ready =>
            {
                self.refresh(out);
                if let Some(g) = r
                    .get("groupInfo")
                    .and_then(|g| g.get("groupId"))
                    .and_then(Value::as_i64)
                    .filter(|g| self.members_read.contains(g))
                {
                    self.send(out, Pending::Members(g), format!("/_members #{g}"));
                }
            }
            _ => {}
        }
    }

    /// New items: a blocked friend's are deleted on arrival and never reported; small
    /// pictures are fetched.
    fn on_new_items(&mut self, r: &Value, out: &mut Output) {
        let mut drop: HashMap<i64, Vec<i64>> = HashMap::new();
        let mut fetch = Vec::new();
        let mut messages = Vec::new();
        for a in r
            .get("chatItems")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(m) = self.parse_a_item(a) else {
                continue;
            };
            if let ChatRef::Direct(c) = m.chat
                && self.blocked.contains(&c)
                && m.author != MessageAuthor::Me
            {
                drop.entry(c).or_default().push(m.id);
                continue;
            }
            if let Some(id) = self.fetchable(a) {
                fetch.push(id);
            }
            messages.push(m);
        }
        for (c, ids) in drop {
            let ids: Vec<String> = ids.iter().map(i64::to_string).collect();
            let cmd = format!("/_delete item @{c} {} internal", ids.join(","));
            self.send(out, Pending::Quiet, cmd);
        }
        for id in fetch {
            self.send(out, Pending::Quiet, format!("/freceive {id} encrypt=off"));
        }
        if !messages.is_empty() {
            out.events.push(ChatEvent::Messages(messages));
        }
    }

    /// The file number of a received picture that is fetched by itself: files on, small
    /// enough, not from a member the user blocked.
    fn fetchable(&self, a: &Value) -> Option<i64> {
        if !self.files_on() {
            return None;
        }
        let ci = a.get("chatItem")?;
        let file = ci.get("file")?;
        let image = str_at(ci, &["content", "msgContent", "type"]) == Some("image");
        let invited = str_at(file, &["fileStatus", "type"]) == Some("rcvInvitation");
        let size = file.get("fileSize").and_then(Value::as_u64)?;
        let blocked = ci
            .get("meta")
            .and_then(|m| m.get("itemDeleted"))
            .is_some_and(|d| !d.is_null());
        (image && invited && size <= MAX_FILE && !blocked)
            .then(|| file.get("fileId").and_then(Value::as_i64))
            .flatten()
    }

    // ---- reading the engine's JSON ------------------------------------------------------

    /// What reading items of `chat` needs: the user's member number there, the files folder.
    fn ctx(&self, info: Option<&Value>, chat: ChatRef) -> Ctx<'_> {
        let me = info.and_then(me_of).or(match chat {
            ChatRef::Group(g) => self.me_in.get(&g).copied(),
            ChatRef::Direct(_) => None,
        });
        Ctx {
            me,
            files: self.files_dir(),
            known: &self.files,
        }
    }

    /// `chatItems` of a `newChatItems` or `chatItemsStatusesUpdated`.
    fn parse_items(&self, r: &Value) -> Vec<ChatMessage> {
        r.get("chatItems")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|a| self.parse_a_item(a)).collect())
            .unwrap_or_default()
    }

    /// An `AChatItem` (`chatInfo` + `chatItem`).
    fn parse_a_item(&self, a: &Value) -> Option<ChatMessage> {
        let info = a.get("chatInfo")?;
        let chat = chat_of(info)?;
        parse_item(chat, a.get("chatItem")?, &self.ctx(Some(info), chat))
    }

    /// `chatItemDeletions` (sent or received): each item, shown as deleted.
    fn parse_deletions(&self, r: &Value) -> Vec<ChatMessage> {
        r.get("chatItemDeletions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|d| {
                let a = d
                    .get("toChatItem")
                    .filter(|t| !t.is_null())
                    .or_else(|| d.get("deletedChatItem"))?;
                let mut m = self.parse_a_item(a)?;
                m.body = MessageBody::Deleted;
                m.reactions.clear();
                m.unread = false;
                Some(m)
            })
            .collect()
    }

    /// A `chatItemReaction`: the item with its reactions now.
    fn parse_reaction(&self, r: &Value) -> Option<ChatMessage> {
        let reaction = r.get("reaction")?;
        let info = reaction.get("chatInfo")?;
        let chat = chat_of(info)?;
        let ci = reaction.get("chatReaction")?.get("chatItem")?;
        parse_item(chat, ci, &self.ctx(Some(info), chat))
    }

    /// An `apiChats` answer: the whole list. Remembers what later commands need.
    #[allow(clippy::too_many_lines)]
    fn parse_chats(&mut self, r: &Value) -> ChatEvent {
        let mut friends = Vec::new();
        let mut groups = Vec::new();
        let mut requests = Vec::new();
        let mut chats = Vec::new();
        self.settings.clear();
        self.custom.clear();
        self.blocked.clear();
        self.me_in.clear();
        for c in r
            .get("chats")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(info) = c.get("chatInfo") else {
                continue;
            };
            let unread = c
                .get("chatStats")
                .and_then(|s| s.get("unreadCount"))
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(0);
            let (chat, name, muted) = match info.get("type").and_then(Value::as_str) {
                Some("direct") => {
                    let Some(ct) = info.get("contact") else {
                        continue;
                    };
                    let Some(id) = ct.get("contactId").and_then(Value::as_i64) else {
                        continue;
                    };
                    // A contact the engine prepared but never connected (its built-in
                    // contacts), or one deleted: not a friend.
                    if ct.get("activeConn").is_none_or(Value::is_null)
                        || ct.get("chatDeleted").and_then(Value::as_bool) == Some(true)
                    {
                        continue;
                    }
                    let name = str_at(ct, &["profile", "displayName"])
                        .or_else(|| ct.get("localDisplayName").and_then(Value::as_str))
                        .unwrap_or("")
                        .to_owned();
                    let ready = str_at(ct, &["activeConn", "connStatus", "type"]) == Some("ready");
                    let chat = ChatRef::Direct(id);
                    let muted = self.remember_settings(chat, ct.get("chatSettings"));
                    let custom = ct.get("customData").cloned().unwrap_or(Value::Null);
                    let blocked = custom
                        .get(CUSTOM_KEY)
                        .and_then(|e| e.get("blocked"))
                        .and_then(Value::as_bool)
                        == Some(true);
                    if !custom.is_null() {
                        self.custom.insert(id, custom);
                    }
                    if blocked {
                        self.blocked.insert(id);
                    }
                    let verified = ct
                        .get("activeConn")
                        .and_then(|c| c.get("connectionCode"))
                        .is_some_and(|c| !c.is_null());
                    friends.push(Friend {
                        id,
                        name: name.clone(),
                        ready,
                        about: str_at(ct, &["profile", "shortDescr"])
                            .unwrap_or("")
                            .to_owned(),
                        since: timestamp(ct.get("createdAt").and_then(Value::as_str)),
                        verified,
                        blocked,
                        muted,
                        ..Friend::default()
                    });
                    if blocked {
                        // Reachable from "Blocked", not in the list.
                        continue;
                    }
                    (chat, name, muted)
                }
                Some("group") => {
                    let Some(g) = info.get("groupInfo") else {
                        continue;
                    };
                    let Some(id) = g.get("groupId").and_then(Value::as_i64) else {
                        continue;
                    };
                    let name = str_at(g, &["groupProfile", "displayName"])
                        .or_else(|| g.get("localDisplayName").and_then(Value::as_str))
                        .unwrap_or("")
                        .to_owned();
                    let members = g
                        .get("groupSummary")
                        .and_then(|s| s.get("currentMembers"))
                        .and_then(Value::as_u64)
                        .and_then(|n| u32::try_from(n).ok());
                    if let Some(me) = me_of(info) {
                        self.me_in.insert(id, me);
                    }
                    if matches!(
                        str_at(g, &["membership", "memberRole"]),
                        Some("owner" | "admin")
                    ) && membership(str_at(g, &["membership", "memberStatus"]).unwrap_or(""))
                        == Membership::Member
                    {
                        self.links_to_read.push(id);
                    }
                    let chat = ChatRef::Group(id);
                    let muted = self.remember_settings(chat, g.get("chatSettings"));
                    groups.push(ChatGroup {
                        id,
                        name: name.clone(),
                        membership: membership(
                            str_at(g, &["membership", "memberStatus"]).unwrap_or(""),
                        ),
                        members,
                        muted,
                        ..ChatGroup::default()
                    });
                    (chat, name, muted)
                }
                Some("contactRequest") => {
                    let Some(req) = info.get("contactRequest") else {
                        continue;
                    };
                    if let Some(id) = req.get("contactRequestId").and_then(Value::as_i64) {
                        let name = str_at(req, &["profile", "displayName"])
                            .or_else(|| req.get("localDisplayName").and_then(Value::as_str))
                            .unwrap_or("")
                            .to_owned();
                        requests.push(FriendRequest { id, name });
                    }
                    continue;
                }
                _ => continue,
            };
            let ctx = self.ctx(Some(info), chat);
            let items = c.get("chatItems").and_then(Value::as_array);
            let mut last = items
                .and_then(|items| items.iter().rev().find_map(|ci| parse_item(chat, ci, &ctx)));
            // The latest item is one the page does not show: the latest shown one, read
            // separately (once per latest item).
            let newest = items
                .and_then(|i| i.last())
                .and_then(|ci| ci.get("meta"))
                .and_then(|m| m.get("itemId"))
                .and_then(Value::as_i64);
            if last.is_none()
                && let Some(newest) = newest
            {
                match self.previews.get(&chat) {
                    Some((id, m)) if *id == newest => last.clone_from(m),
                    _ => self.previews_to_read.push((chat, newest)),
                }
            }
            chats.push(ChatPreview {
                chat,
                name,
                last,
                unread,
                muted,
            });
        }
        ChatEvent::Chats {
            friends,
            groups,
            requests,
            chats,
        }
    }

    /// Keep a conversation's settings for `/_settings`; whether it is muted.
    fn remember_settings(&mut self, chat: ChatRef, s: Option<&Value>) -> bool {
        let Some(s) = s.filter(|s| s.is_object()) else {
            return false;
        };
        self.settings.insert(chat, s.clone());
        s.get("enableNtfs")
            .and_then(Value::as_str)
            .is_some_and(|n| n != "all")
    }
}

/// What reading one conversation's items needs.
struct Ctx<'a> {
    /// The user's member number (groups).
    me: Option<i64>,
    /// Where received files are.
    files: PathBuf,
    /// Messages' files, remembered as they are read.
    known: &'a RefCell<HashMap<(ChatRef, i64), FileRef>>,
}

fn failed(request: RequestKind, code: ErrorCode, detail: String) -> ChatEvent {
    ChatEvent::Failed {
        request,
        code,
        detail,
    }
}

/// A compact form of an engine value for the log (bounded).
fn short(v: &Value) -> String {
    let mut s = v.to_string();
    if s.len() > 400 {
        let mut end = 400;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push('…');
    }
    s
}

fn error_code(kind: RequestKind, e: &Value) -> ErrorCode {
    let text = e.to_string();
    if ["NETWORK", "TIMEOUT", "HOST", "noConnection"]
        .iter()
        .any(|w| text.contains(w))
    {
        ErrorCode::ChtNoConnection
    } else if kind == RequestKind::Connect {
        ErrorCode::ChtBadLink
    } else {
        ErrorCode::ChtRequestFailed
    }
}

fn chat_ref(chat: ChatRef) -> String {
    match chat {
        ChatRef::Direct(id) => format!("@{id}"),
        ChatRef::Group(id) => format!("#{id}"),
    }
}

fn group_profile_json(name: &str) -> String {
    serde_json::json!({ "displayName": name, "fullName": "" }).to_string()
}

/// The engine's form of a reaction emoji, if it carries it (❤️ without U+FE0F).
fn engine_emoji(display: &str) -> Option<String> {
    REACTION_EMOJI
        .contains(&display)
        .then(|| display.replace('\u{FE0F}', ""))
}

/// The display form of an emoji the engine reports.
fn display_emoji(engine: &str) -> String {
    REACTION_EMOJI
        .iter()
        .find(|e| e.replace('\u{FE0F}', "") == engine)
        .map_or_else(|| engine.to_owned(), |e| (*e).to_owned())
}

/// A name as the engine's text names it: bare when it is one word, else `'quoted'`.
fn mention_word(name: &str) -> String {
    if !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        name.to_owned()
    } else {
        format!("'{name}'")
    }
}

/// `@Two Words` in the text becomes `@'Two Words'` (the engine's form).
fn quote_mentions(text: &str, mentions: &[(String, i64)]) -> String {
    let mut text = text.to_owned();
    for (name, _) in mentions {
        let word = mention_word(name);
        if word != *name {
            text = text.replace(&format!("@{name}"), &format!("@{word}"));
        }
    }
    text
}

/// `@'Two Words'` back to `@Two Words`, for the names this message mentions.
fn unquote_mentions(text: &str, names: &[&str]) -> String {
    let mut text = text.to_owned();
    for name in names {
        text = text.replace(&format!("@'{name}'"), &format!("@{name}"));
    }
    text
}

fn mention_map(mentions: &[(String, i64)]) -> Value {
    Value::Object(
        mentions
            .iter()
            .map(|(n, m)| (n.clone(), Value::from(*m)))
            .collect(),
    )
}

/// A created link: the short form when the relay supports it, else the full one.
fn link_text(l: &Value) -> Option<String> {
    l.get("connShortLink")
        .and_then(Value::as_str)
        .or_else(|| l.get("connFullLink").and_then(Value::as_str))
        .map(str::to_owned)
}

fn str_at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter().try_fold(v, |v, k| v.get(*k))?.as_str()
}

fn timestamp(s: Option<&str>) -> Option<Timestamp> {
    let t: jiff::Timestamp = s?.parse().ok()?;
    Some(Timestamp::from_millis(t.as_millisecond()))
}

/// The conversation of a `chatInfo`, if it is one the Chat page shows.
fn chat_of(info: &Value) -> Option<ChatRef> {
    match info.get("type").and_then(Value::as_str)? {
        "direct" => Some(ChatRef::Direct(
            info.get("contact")?.get("contactId")?.as_i64()?,
        )),
        "group" => Some(ChatRef::Group(
            info.get("groupInfo")?.get("groupId")?.as_i64()?,
        )),
        _ => None,
    }
}

/// The user's member number in a group's `chatInfo`.
fn me_of(info: &Value) -> Option<i64> {
    info.get("groupInfo")?
        .get("membership")?
        .get("groupMemberId")?
        .as_i64()
}

/// Content types that are settings, not conversation: never shown.
fn hidden(content_type: &str) -> bool {
    content_type == "chatBanner"
        || content_type.ends_with("ChatFeature")
        || content_type.ends_with("ChatPreference")
        || content_type.ends_with("GroupFeature")
        || content_type.ends_with("ChatFeatureRejected")
        || content_type.ends_with("GroupFeatureRejected")
}

/// The engine's display name in a profile.
fn name_in(v: Option<&Value>) -> Option<String> {
    v?.get("displayName")?.as_str().map(str::to_owned)
}

/// The notice an engine item stands for, or `None` for one the page does not show:
/// encryption and feature banners (the conversation's start says it), role and queue
/// changes, errors, and anything unknown (never shown in the engine's own words).
fn notice(ctype: &str, content: &Value, dir: Option<&Value>) -> Option<ChatNotice> {
    let member = || {
        dir.and_then(|d| str_at(d, &["groupMember", "memberProfile", "displayName"]))
            .map(str::to_owned)
    };
    let sub = |k: &str| {
        content
            .get(k)
            .and_then(|e| e.get("type"))
            .and_then(Value::as_str)
    };
    Some(match ctype {
        "rcvDirectE2EEInfo" | "sndDirectE2EEInfo" => ChatNotice::Connected,
        "sndGroupE2EEInfo" => ChatNotice::GroupCreated,
        "rcvGroupE2EEInfo" => ChatNotice::YouJoined,
        "rcvGroupInvitation" | "sndGroupInvitation" => ChatNotice::GroupInvitation {
            group: str_at(content, &["groupInvitation", "groupProfile", "displayName"])?.to_owned(),
            sent: ctype == "sndGroupInvitation",
        },
        "rcvDirectEvent" => {
            let e = content.get("rcvDirectEvent")?;
            match sub("rcvDirectEvent")? {
                "contactDeleted" => ChatNotice::FriendDeleted,
                "profileUpdated" => renamed(e)?,
                _ => return None,
            }
        }
        "rcvGroupEvent" => {
            let e = content.get("rcvGroupEvent")?;
            match sub("rcvGroupEvent")? {
                "memberConnected" => ChatNotice::MemberJoined(member()?),
                "memberAdded" => ChatNotice::MemberAdded {
                    by: Some(member()?),
                    who: name_in(e.get("profile"))?,
                },
                "memberLeft" => ChatNotice::MemberLeft(member()?),
                "memberDeleted" => ChatNotice::MemberRemoved {
                    by: Some(member()?),
                    who: name_in(e.get("profile"))?,
                },
                "userDeleted" => ChatNotice::YouRemoved(member()?),
                "groupDeleted" => ChatNotice::GroupDeleted,
                "groupUpdated" => ChatNotice::GroupNameChanged(name_in(e.get("groupProfile"))?),
                "memberProfileUpdated" => renamed(e)?,
                _ => return None,
            }
        }
        "sndGroupEvent" => {
            let e = content.get("sndGroupEvent")?;
            match sub("sndGroupEvent")? {
                "memberDeleted" => ChatNotice::MemberRemoved {
                    by: None,
                    who: name_in(e.get("profile"))?,
                },
                "userLeft" => ChatNotice::YouLeft,
                "groupUpdated" => ChatNotice::GroupNameChanged(name_in(e.get("groupProfile"))?),
                _ => return None,
            }
        }
        "rcvConnEvent" => match sub("rcvConnEvent")? {
            "verificationCodeReset" => ChatNotice::CodeChanged,
            _ => return None,
        },
        _ => return None,
    })
}

/// A profile change that changed the name (other changes are not shown).
fn renamed(e: &Value) -> Option<ChatNotice> {
    let from = name_in(e.get("fromProfile"))?;
    let to = name_in(e.get("toProfile"))?;
    (from != to).then_some(ChatNotice::NameChanged { from, to })
}

/// A call item as one line (calls are not carried yet: one that was not ended counts as
/// missed).
fn call_record(content: &Value) -> crate::model::CallRecord {
    use crate::model::{CallKind, CallRecord};
    let kind = match content.get("status").and_then(Value::as_str) {
        Some("ended") => CallKind::Ended,
        Some("rejected") => CallKind::Declined,
        _ => CallKind::Missed,
    };
    let secs = content.get("duration").and_then(Value::as_u64).unwrap_or(0);
    CallRecord {
        kind,
        minutes: u32::try_from(secs / 60).unwrap_or(u32::MAX),
    }
}

/// Who a `chatDir` names.
fn author_of(dir: &Value) -> Option<MessageAuthor> {
    Some(match dir.get("type").and_then(Value::as_str)? {
        "directSnd" | "groupSnd" | "localSnd" => MessageAuthor::Me,
        "groupRcv" => MessageAuthor::Member(
            str_at(dir, &["groupMember", "memberProfile", "displayName"])
                .or_else(|| str_at(dir, &["groupMember", "localDisplayName"]))
                .unwrap_or("")
                .to_owned(),
        ),
        _ => MessageAuthor::Friend,
    })
}

/// A whole percentage of `done` out of `total`.
fn percent(done: Option<u64>, total: Option<u64>) -> u8 {
    match (done, total) {
        (Some(d), Some(t)) if t > 0 => u8::try_from(d.min(t) * 100 / t).unwrap_or(100),
        _ => 0,
    }
}

/// A `data:` URI's bytes (the preview picture of a picture or video).
fn data_uri_bytes(uri: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    if uri == PREVIEW {
        return None;
    }
    let (_, b64) = uri.split_once("base64,")?;
    base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .ok()
}

/// A preview picture's `data:` URI (PNG or JPEG, by its first bytes).
fn data_uri(bytes: &[u8]) -> String {
    use base64::Engine as _;
    let kind = if bytes.starts_with(b"\x89PNG") {
        "png"
    } else {
        "jpg"
    };
    format!(
        "data:image/{kind};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// A picture, video or file message (`mc` its `msgContent`). A GIF that is here (sent, or
/// fetched) is a [`MessageBody::Gif`] with its path; everything else a [`Media`] whose
/// state follows the engine's file status.
fn file_body(chat: ChatRef, item: i64, ci: &Value, mc: &Value, ctx: &Ctx<'_>) -> MessageBody {
    let kind = match mc.get("type").and_then(Value::as_str) {
        Some("image") => FileKind::Image,
        Some("video") => FileKind::Video,
        _ => FileKind::File,
    };
    let file = ci.get("file").filter(|f| !f.is_null());
    let name = file
        .and_then(|f| f.get("fileName"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let size = file
        .and_then(|f| f.get("fileSize"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let status = file
        .and_then(|f| f.get("fileStatus"))
        .unwrap_or(&Value::Null);
    let path = file
        .and_then(|f| str_at(f, &["fileSource", "filePath"]))
        .unwrap_or("");
    let n = |k: &str| status.get(k).and_then(Value::as_u64);
    let ty = status.get("type").and_then(Value::as_str).unwrap_or("");
    let mut local = None;
    let state = match ty {
        "rcvInvitation" => FileState::Offered,
        "rcvAccepted" | "rcvWarning" => FileState::Receiving { percent: 0 },
        "rcvTransfer" => FileState::Receiving {
            percent: percent(n("rcvProgress"), n("rcvTotal")),
        },
        "rcvComplete" => {
            // The engine names received files relative to the files folder; anything that
            // is not a bare name (a path the sender could have chosen) is not followed.
            let p = Path::new(path);
            let bare = p.components().count() == 1
                && matches!(p.components().next(), Some(std::path::Component::Normal(_)));
            if bare {
                let full = ctx.files.join(p);
                local = Some(full.clone());
                FileState::Received(full)
            } else {
                tracing::warn!("chat: a received file with a path that is not a bare name");
                FileState::Failed
            }
        }
        "sndStored" | "sndWarning" => FileState::Sending { percent: 0 },
        "sndTransfer" => FileState::Sending {
            percent: percent(n("sndProgress"), n("sndTotal")),
        },
        "sndComplete" => FileState::Sent,
        "sndCancelled" | "rcvCancelled" | "rcvAborted" => FileState::Cancelled,
        _ => FileState::Failed,
    };
    if ty.starts_with("snd") && Path::new(path).is_absolute() {
        local = Some(PathBuf::from(path));
    }
    let gif = kind == FileKind::Image
        && Path::new(&name)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("gif"));
    let kind = if gif { FileKind::Gif } else { kind };
    if let Some(id) = file.and_then(|f| f.get("fileId")).and_then(Value::as_i64) {
        ctx.known.borrow_mut().insert(
            (chat, item),
            FileRef {
                id,
                local: local.clone(),
                kind,
            },
        );
    }
    if gif && let Some(p) = &local {
        return MessageBody::Gif(p.to_string_lossy().into_owned());
    }
    let media = Media {
        name,
        size,
        preview: mc
            .get("image")
            .and_then(Value::as_str)
            .and_then(data_uri_bytes),
        duration: mc
            .get("duration")
            .and_then(Value::as_u64)
            .and_then(|d| u32::try_from(d).ok()),
        state,
    };
    match kind {
        FileKind::Image | FileKind::Gif => MessageBody::Image(media),
        FileKind::Video => MessageBody::Video(media),
        FileKind::File => MessageBody::File(media),
    }
}

/// A `ChatItem` in `chat`.
#[allow(clippy::too_many_lines)]
fn parse_item(chat: ChatRef, ci: &Value, ctx: &Ctx<'_>) -> Option<ChatMessage> {
    let meta = ci.get("meta")?;
    let content = ci.get("content")?;
    let ctype = content.get("type").and_then(Value::as_str)?;
    if hidden(ctype) {
        return None;
    }
    let item_text = meta.get("itemText").and_then(Value::as_str).unwrap_or("");
    let mentions: Vec<Mention> = ci
        .get("mentions")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(name, m)| {
            let member = m.get("memberRef")?.get("groupMemberId")?.as_i64()?;
            Some(Mention {
                name: name.clone(),
                member,
                me: ctx.me == Some(member),
            })
        })
        .collect();
    let names: Vec<&str> = mentions.iter().map(|m| m.name.as_str()).collect();
    let item = meta.get("itemId")?.as_i64()?;
    let mut body = match ctype {
        "sndMsgContent" | "rcvMsgContent" => {
            let mc = content.get("msgContent")?;
            match mc.get("type").and_then(Value::as_str) {
                Some("text") => {
                    MessageBody::Text(unquote_mentions(mc.get("text")?.as_str()?, &names))
                }
                Some("image" | "video" | "file") => file_body(chat, item, ci, mc, ctx),
                // Voice and links are shown by their text for now.
                _ => MessageBody::Text(item_text.to_owned()),
            }
        }
        "sndDeleted" | "rcvDeleted" | "sndModerated" | "rcvModerated" | "rcvBlocked" => {
            MessageBody::Deleted
        }
        "sndCall" | "rcvCall" => MessageBody::Call(call_record(content)),
        _ => match notice(ctype, content, ci.get("chatDir")) {
            Some(n) => MessageBody::Notice(n),
            None => {
                tracing::debug!(ctype, "chat: an engine notice not shown");
                return None;
            }
        },
    };
    let deleted = meta
        .get("itemDeleted")
        .filter(|d| !d.is_null())
        .and_then(|d| d.get("type"))
        .and_then(Value::as_str);
    if matches!(deleted, Some("deleted" | "moderated")) {
        body = MessageBody::Deleted;
    }
    // "blocked" / "blockedByAdmin": a member the user (or an admin) blocked; the text stays
    // and the page collapses it.
    let hidden_member = matches!(deleted, Some("blocked" | "blockedByAdmin"));
    let author = author_of(ci.get("chatDir")?)?;
    let status = str_at(meta, &["itemStatus", "type"]).unwrap_or("");
    let delivery = match status {
        "sndNew" => Delivery::Sending,
        "sndSent" => Delivery::Sent,
        "sndRcvd" => Delivery::Delivered,
        s if s.starts_with("sndError") || s.starts_with("sndWarning") => Delivery::Failed,
        s if s.starts_with("snd") => Delivery::Sent,
        _ => Delivery::Received,
    };
    let at = timestamp(meta.get("itemTs").and_then(Value::as_str))
        .or_else(|| timestamp(meta.get("createdAt").and_then(Value::as_str)))
        .unwrap_or(Timestamp::EPOCH);
    let reply = ci.get("quotedItem").filter(|q| !q.is_null()).map(|q| {
        let author = q.get("chatDir").and_then(author_of).unwrap_or(match chat {
            ChatRef::Direct(_) => MessageAuthor::Friend,
            ChatRef::Group(_) => MessageAuthor::Member(String::new()),
        });
        let text = str_at(q, &["content", "text"])
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("")
            .to_owned();
        Quote {
            id: q.get("itemId").and_then(Value::as_i64),
            author,
            text,
        }
    });
    let reactions = if body == MessageBody::Deleted {
        Vec::new()
    } else {
        ci.get("reactions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|r| {
                let emoji = str_at(r, &["reaction", "emoji"])?;
                let count = u32::try_from(r.get("totalReacted")?.as_u64()?).ok()?;
                (count > 0).then(|| Reaction {
                    emoji: display_emoji(emoji),
                    count,
                    mine: r.get("userReacted").and_then(Value::as_bool) == Some(true),
                    who: Vec::new(),
                })
            })
            .collect()
    };
    Some(ChatMessage {
        id: meta.get("itemId")?.as_i64()?,
        chat,
        author,
        body,
        at,
        delivery,
        unread: status == "rcvNew" && !hidden_member,
        reply,
        reactions,
        edited: meta.get("itemEdited").and_then(Value::as_bool) == Some(true),
        mentions,
    })
}

fn membership(status: &str) -> Membership {
    match status {
        "invited" => Membership::Invited,
        "accepted" | "pendingApproval" | "pendingReview" => Membership::Joining,
        "left" | "removed" | "deleted" | "groupDeleted" | "rejected" => Membership::Left,
        _ => Membership::Member,
    }
}

/// One `GroupMember`.
fn member(m: &Value, me: bool) -> Option<GroupMember> {
    Some(GroupMember {
        id: m.get("groupMemberId")?.as_i64()?,
        name: str_at(m, &["memberProfile", "displayName"])
            .or_else(|| m.get("localDisplayName").and_then(Value::as_str))
            .unwrap_or("")
            .to_owned(),
        friend: if me {
            None
        } else {
            m.get("memberContactId").and_then(Value::as_i64)
        },
        me,
        owner: m.get("memberRole").and_then(Value::as_str) == Some("owner"),
        blocked: m
            .get("memberSettings")
            .and_then(|s| s.get("showMessages"))
            .and_then(Value::as_bool)
            == Some(false),
        ..GroupMember::default()
    })
}

/// A `groupMembers` answer's `group` (`groupInfo` + `members`, the user not among them):
/// the user first, then everyone still in the group.
fn parse_members(g: &Value) -> Vec<GroupMember> {
    let me = g
        .get("groupInfo")
        .and_then(|i| i.get("membership"))
        .and_then(|m| member(m, true));
    let others = g
        .get("members")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|m| {
            membership(m.get("memberStatus").and_then(Value::as_str).unwrap_or(""))
                != Membership::Left
        })
        .filter_map(|m| member(m, false));
    me.into_iter().chain(others).collect()
}

/// The servers a check tests in a `userServers` list: (operator entry, kind, index,
/// address) of every server not deleted, of an enabled operator (or the user's own), that
/// is enabled or was disabled by an earlier check (`tested: false`). With `own_only` (custom
/// relays) only the user's own: the presets stay untouched, and uncontacted.
fn candidates(list: &Value, own_only: bool) -> Vec<(usize, &'static str, usize, String)> {
    let mut found = Vec::new();
    for (i, op) in list.as_array().into_iter().flatten().enumerate() {
        let operator = op.get("operator").filter(|o| !o.is_null());
        let enabled_op = match operator {
            None => true,
            Some(_) if own_only => false,
            Some(o) => o.get("enabled").and_then(Value::as_bool) == Some(true),
        };
        if !enabled_op {
            continue;
        }
        for key in ["smpServers", "xftpServers"] {
            for (j, s) in op
                .get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                let flag = |k: &str| s.get(k).and_then(Value::as_bool);
                if flag("deleted") == Some(true) {
                    continue;
                }
                if (flag("enabled") == Some(true) || flag("tested") == Some(false))
                    && let Some(addr) = s.get("server").and_then(Value::as_str)
                {
                    found.push((i, key, j, addr.to_owned()));
                }
            }
        }
    }
    found
}

/// A server's host, for the log (no key fingerprint).
fn host_of(server: &str) -> &str {
    server.rsplit('@').next().unwrap_or(server)
}

#[cfg(test)]
mod tests;

/// What the engine carries with these relays (`smp://` and `xftp://` addresses; empty = the
/// presets), known before it starts: Settings shows the same rows whether Chat is on or off.
pub fn features_with(relays: &[String]) -> ChatFeatures {
    let custom_relays = relays.iter().any(|r| r.starts_with("smp://"));
    let custom_files = relays.iter().any(|r| r.starts_with("xftp://"));
    features_for(!custom_relays || custom_files)
}

fn features_for(files: bool) -> ChatFeatures {
    ChatFeatures {
        files,
        gifs: files && GIFS_SEND_WORKS,
        reactions: true,
        replies: true,
        edit: true,
        mute: true,
        block: true,
        verify: true,
        mentions: true,
        ..ChatFeatures::default()
    }
}
