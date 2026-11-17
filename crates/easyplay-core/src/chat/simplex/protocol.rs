//! The SimpleX side of the engine seam: [`Request`]s become chat commands for the helper,
//! the helper's lines become [`ChatEvent`]s. Pure: no I/O, no clock, so it is tested
//! against the JSON recorded from the engine (ADR-0033, Phase 0).
//!
//! The engine's JSON is read tolerantly through `serde_json::Value`: a field that is missing
//! or of another shape drops that item (logged), never the whole line, and unknown event
//! types are ignored. Commands follow `bots/api/COMMANDS.md` of simplex-chat v7.0.3 and the
//! parser in `Simplex.Chat.Library.Commands` for the two that file leaves out (`/_get chat`,
//! `/_read chat`).
//!
//! Links: a friend's link may be short (resolved through its relay) or full, so `Connect`
//! asks `/_connect plan` first and connects with the link the plan returns; a link that is
//! the user's own, or one already used, connects nothing.
//!
//! Start-up: `/user` (or `/_create user` when there is none), `/_start`, `/smp` when custom
//! relays are set (it is refused before `/_start`), then the chat list and the address.
//! Requests made before that wait. Events that change who the user talks to (a friend
//! connected, a group joined, a request arrived) make the codec read the chat list again;
//! at most one such read is in flight, and a change during it asks for one more.

use std::collections::HashMap;

use serde_json::Value;

use crate::chat::engine::{ChatEvent, Request, RequestKind};
use crate::clock::Timestamp;
use crate::error::ErrorCode;
use crate::model::{
    ChatGroup, ChatMessage, ChatPreview, ChatProfile, ChatRef, Delivery, Friend, FriendRequest,
    Membership, MessageAuthor, MessageBody,
};

/// How many conversations the list reads.
pub const CHAT_LIST_COUNT: u32 = 200;

/// What a command's answer belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    ShowUser,
    CreateUser,
    Start,
    Relays,
    ShowAddress,
    Chats,
    History {
        tag: u64,
        chat: ChatRef,
    },
    /// `/_connect plan`: what a link is, before connecting with it.
    Plan,
    Req(RequestKind),
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
    relays: Vec<String>,
    phase: Phase,
    user: Option<i64>,
    profile: ChatProfile,
    next: u64,
    pending: HashMap<u64, Pending>,
    queued: Vec<Request>,
    chats_in_flight: bool,
    chats_again: bool,
}

impl Codec {
    /// A codec for a first start under `name` (used only when the database has no user yet)
    /// with `relays` (empty = the engine's presets).
    pub fn new(name: &str, relays: Vec<String>) -> Self {
        Self {
            name: name.to_owned(),
            relays,
            phase: Phase::Opening,
            user: None,
            profile: ChatProfile::default(),
            next: 1,
            pending: HashMap::new(),
            queued: Vec::new(),
            chats_in_flight: false,
            chats_again: false,
        }
    }

    /// Whether start-up finished.
    pub fn started(&self) -> bool {
        self.phase == Phase::Ready
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

    fn render(&mut self, request: Request, out: &mut Output) {
        let uid = self.uid();
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
                format!("/_profile {uid} {}", profile_json(&p.name)),
            ),
            Request::CreateInvite => (RequestKind::CreateInvite, format!("/_connect {uid}")),
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
                self.send(out, Pending::Plan, format!("/_connect plan {uid} {link}"));
                return;
            }
            Request::Accept(id) => (RequestKind::Answer, format!("/_accept {id}")),
            Request::Reject(id) => (RequestKind::Answer, format!("/_reject {id}")),
            Request::CreateAddress => (RequestKind::Address, format!("/_address {uid}")),
            Request::DeleteAddress => (RequestKind::Address, format!("/_delete_address {uid}")),
            Request::Send { chat, text } => {
                let msg = serde_json::json!([{
                    "msgContent": { "type": "text", "text": text },
                    "mentions": {},
                }]);
                (
                    RequestKind::Send,
                    format!("/_send {} json {msg}", chat_ref(chat)),
                )
            }
            Request::MarkRead(chat) => (
                RequestKind::MarkRead,
                format!("/_read chat {}", chat_ref(chat)),
            ),
            Request::NewGroup(name) => (
                RequestKind::Group,
                format!("/_group {uid} {}", profile_json(&name)),
            ),
            Request::AddMember { group, friend } => (
                RequestKind::Group,
                format!("/_add #{group} {friend} member"),
            ),
            Request::JoinGroup(g) => (RequestKind::Group, format!("/_join #{g}")),
            Request::LeaveGroup(g) => (RequestKind::Group, format!("/_leave #{g}")),
            Request::RemoveFriend(c) => (
                RequestKind::RemoveFriend,
                format!(
                    "/_delete @{c} {}",
                    serde_json::json!({ "type": "full", "notify": true })
                ),
            ),
        };
        self.send(out, Pending::Req(kind), cmd);
    }

    // ---- answers --------------------------------------------------------------------------

    fn on_response(&mut self, pending: Pending, body: &Value, out: &mut Output) {
        let result = body.get("result");
        let error = body.get("error");
        let ty = result.and_then(|r| r.get("type")).and_then(Value::as_str);
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
                _ => out.events.push(failed(
                    RequestKind::Start,
                    ErrorCode::ChtRequestFailed,
                    error.map(short).unwrap_or_default(),
                )),
            },
            Pending::Start => match ty {
                Some("chatStarted" | "chatRunning") => {
                    if self.relays.is_empty() {
                        self.become_ready(out);
                    } else {
                        let cmd = format!("/smp {}", self.relays.join(" "));
                        self.send(out, Pending::Relays, cmd);
                    }
                }
                _ => out.events.push(failed(
                    RequestKind::Start,
                    ErrorCode::ChtRequestFailed,
                    error.map(short).unwrap_or_default(),
                )),
            },
            Pending::Relays => {
                if error.is_some() {
                    // Fail closed: the user chose these relays, so never fall back to the
                    // presets.
                    out.events.push(failed(
                        RequestKind::Start,
                        ErrorCode::ChtRequestFailed,
                        error.map(short).unwrap_or_default(),
                    ));
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
                    out.events.push(parse_chats(r));
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
                let items = result
                    .filter(|_| ty == Some("apiChat"))
                    .and_then(|r| r.get("chat"))
                    .and_then(|c| c.get("chatItems"))
                    .and_then(Value::as_array);
                match items {
                    Some(items) => {
                        let messages = items.iter().filter_map(|ci| parse_item(chat, ci)).collect();
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
            Pending::Plan => {
                let plan = result.filter(|_| ty == Some("connectionPlan"));
                let Some(plan) = plan else {
                    let e = error.cloned().unwrap_or(Value::Null);
                    out.events.push(failed(
                        RequestKind::Connect,
                        error_code(RequestKind::Connect, &e),
                        short(&e),
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
                        self.send(out, Pending::Req(RequestKind::Connect), cmd);
                    }
                }
            }
            Pending::Req(kind) => {
                if let Some(e) = error {
                    out.events.push(failed(kind, error_code(kind, e), short(e)));
                    return;
                }
                self.on_request_done(kind, ty, result, out);
            }
        }
    }

    fn on_user(&mut self, r: &Value, out: &mut Output) {
        let user = r.get("user");
        self.user = user.and_then(|u| u.get("userId")).and_then(Value::as_i64);
        self.profile = ChatProfile {
            name: user
                .and_then(|u| u.get("profile"))
                .and_then(|p| p.get("displayName"))
                .and_then(Value::as_str)
                .unwrap_or(&self.name)
                .to_owned(),
        };
        self.send(out, Pending::Start, "/_start".to_owned());
    }

    fn become_ready(&mut self, out: &mut Output) {
        self.phase = Phase::Ready;
        out.events.push(ChatEvent::Ready(self.profile.clone()));
        self.refresh(out);
        let uid = self.uid();
        self.send(out, Pending::ShowAddress, format!("/_show_address {uid}"));
        for r in std::mem::take(&mut self.queued) {
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
                if let Some(name) = r
                    .get("toProfile")
                    .and_then(|p| p.get("displayName"))
                    .and_then(Value::as_str)
                {
                    self.profile.name = name.to_owned();
                    out.events.push(ChatEvent::Profile(self.profile.clone()));
                }
            }
            (RequestKind::Send, Some("newChatItems")) => {
                let m = parse_items(r);
                if !m.is_empty() {
                    out.events.push(ChatEvent::Messages(m));
                }
            }
            (RequestKind::MarkRead | RequestKind::SetProfile, _) => {}
            (
                RequestKind::Connect
                | RequestKind::Answer
                | RequestKind::Group
                | RequestKind::RemoveFriend,
                _,
            ) => self.refresh(out),
            (k, t) => tracing::debug!(?k, ?t, "chat: answer not used"),
        }
    }

    // ---- events ---------------------------------------------------------------------------

    fn on_event(&mut self, r: &Value, out: &mut Output) {
        let ty = r.get("type").and_then(Value::as_str).unwrap_or("");
        match ty {
            "newChatItems" => {
                let m = parse_items(r);
                if !m.is_empty() {
                    out.events.push(ChatEvent::Messages(m));
                }
            }
            "chatItemsStatusesUpdated" | "chatItemUpdated" => {
                let m = if ty == "chatItemUpdated" {
                    r.get("chatItem")
                        .and_then(parse_a_item)
                        .into_iter()
                        .collect()
                } else {
                    parse_items(r)
                };
                if !m.is_empty() {
                    out.events.push(ChatEvent::Updated(m));
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
                if self.phase == Phase::Ready =>
            {
                self.refresh(out);
            }
            _ => {}
        }
    }
}

// ---- reading the engine's JSON ----------------------------------------------------------

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

fn profile_json(name: &str) -> String {
    serde_json::json!({ "displayName": name, "fullName": "" }).to_string()
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

/// `chatItems` of a `newChatItems` or `chatItemsStatusesUpdated`.
fn parse_items(r: &Value) -> Vec<ChatMessage> {
    r.get("chatItems")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(parse_a_item).collect())
        .unwrap_or_default()
}

/// An `AChatItem` (`chatInfo` + `chatItem`).
fn parse_a_item(a: &Value) -> Option<ChatMessage> {
    let chat = chat_of(a.get("chatInfo")?)?;
    parse_item(chat, a.get("chatItem")?)
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

/// A `ChatItem` in `chat`.
fn parse_item(chat: ChatRef, ci: &Value) -> Option<ChatMessage> {
    let meta = ci.get("meta")?;
    let content = ci.get("content")?;
    let ctype = content.get("type").and_then(Value::as_str)?;
    if hidden(ctype) {
        return None;
    }
    let item_text = meta.get("itemText").and_then(Value::as_str).unwrap_or("");
    let body = match ctype {
        "sndMsgContent" | "rcvMsgContent" => {
            let mc = content.get("msgContent")?;
            match mc.get("type").and_then(Value::as_str) {
                Some("text") => MessageBody::Text(mc.get("text")?.as_str()?.to_owned()),
                // Pictures, files, voice and links are shown by their text for now.
                _ => MessageBody::Text(item_text.to_owned()),
            }
        }
        _ => MessageBody::Event(item_text.to_owned()),
    };
    let dir = ci.get("chatDir")?;
    let author = match dir.get("type").and_then(Value::as_str)? {
        "directSnd" | "groupSnd" | "localSnd" => MessageAuthor::Me,
        "directRcv" => MessageAuthor::Friend,
        "groupRcv" => MessageAuthor::Member(
            str_at(dir, &["groupMember", "memberProfile", "displayName"])
                .or_else(|| str_at(dir, &["groupMember", "localDisplayName"]))
                .unwrap_or("")
                .to_owned(),
        ),
        _ => MessageAuthor::Friend,
    };
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
    Some(ChatMessage {
        id: meta.get("itemId")?.as_i64()?,
        chat,
        author,
        body,
        at,
        delivery,
        unread: status == "rcvNew",
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

/// An `apiChats` answer: the whole list.
fn parse_chats(r: &Value) -> ChatEvent {
    let mut friends = Vec::new();
    let mut groups = Vec::new();
    let mut requests = Vec::new();
    let mut chats = Vec::new();
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
        let (chat, name) = match info.get("type").and_then(Value::as_str) {
            Some("direct") => {
                let Some(ct) = info.get("contact") else {
                    continue;
                };
                let Some(id) = ct.get("contactId").and_then(Value::as_i64) else {
                    continue;
                };
                // A contact the engine prepared but never connected (its built-in contacts),
                // or one deleted: not a friend.
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
                friends.push(Friend {
                    id,
                    name: name.clone(),
                    ready,
                });
                (ChatRef::Direct(id), name)
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
                groups.push(ChatGroup {
                    id,
                    name: name.clone(),
                    membership: membership(
                        str_at(g, &["membership", "memberStatus"]).unwrap_or(""),
                    ),
                    members,
                });
                (ChatRef::Group(id), name)
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
        let last = c
            .get("chatItems")
            .and_then(Value::as_array)
            .and_then(|items| items.iter().rev().find_map(|ci| parse_item(chat, ci)));
        chats.push(ChatPreview {
            chat,
            name,
            last,
            unread,
        });
    }
    ChatEvent::Chats {
        friends,
        groups,
        requests,
        chats,
    }
}

#[cfg(test)]
mod tests;
