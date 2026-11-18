//! Requests become chat commands. `render` is the table; each request kind has a function that
//! returns the command to send under `Pending::Req`, or `None` when it already sent (or
//! answered) itself.
use super::*;

impl Codec {
    pub(super) fn render(&mut self, request: Request, out: &mut Output) {
        let uid = self.uid();
        let retried = std::mem::take(&mut self.retrying);
        let sent = match request {
            Request::Refresh => {
                self.refresh(out);
                None
            }
            Request::History {
                chat,
                before,
                count,
                tag,
            } => {
                self.render_history(chat, before, count, tag, out);
                None
            }
            Request::SetProfile(p) => Some(self.render_set_profile(&p)),
            Request::SetPicture(p) => Some(self.render_set_picture(p.as_ref())),
            Request::SetGroupPicture { group, picture } => {
                self.render_set_group_picture(group, picture.as_ref(), out)
            }
            Request::CreateInvite | Request::CreateAddress => {
                self.render_link(request, retried, out);
                None
            }
            Request::Connect(link) => {
                self.render_connect(&link, retried, out);
                None
            }
            Request::Accept(id) => Some((RequestKind::Answer, format!("/_accept {id}"))),
            Request::Reject(id) => Some((RequestKind::Answer, format!("/_reject {id}"))),
            Request::DeleteAddress => {
                Some((RequestKind::Address, format!("/_delete_address {uid}")))
            }
            Request::Send {
                chat,
                text,
                reply_to,
                mentions,
            } => Some(render_text(chat, &text, reply_to, mentions)),
            Request::SendFile {
                chat,
                path,
                kind,
                preview,
                duration,
                text,
            } => {
                let file = render_files::OutFile {
                    path: &path,
                    kind,
                    preview: preview.as_deref(),
                    duration,
                    text: &text,
                };
                self.render_send_file(chat, &file, out)
            }
            Request::ReceiveFile { chat, item } => {
                self.render_file_action(chat, item, false, out);
                None
            }
            Request::CancelFile { chat, item } => {
                self.render_file_action(chat, item, true, out);
                None
            }
            Request::OpenFile { chat, item } => {
                self.render_open_file(chat, item, out);
                None
            }
            Request::LocateFile { chat, item, tag } => {
                self.render_locate_file(chat, item, tag, out);
                None
            }
            Request::CreateGroupLink(group) => {
                let pending = Pending::GroupLink { group, read: false };
                self.send(out, pending, format!("/_create link #{group} member"));
                None
            }
            Request::DeleteGroupLink(group) => {
                let pending = Pending::GroupLink { group, read: false };
                self.send(out, pending, format!("/_delete link #{group}"));
                None
            }
            Request::Edit { chat, item, text } => Some(render_edit(chat, item, &text)),
            Request::Delete { chat, item } => Some((
                RequestKind::Delete,
                format!("/_delete item {} {item} broadcast", chat_ref(chat)),
            )),
            Request::React {
                chat,
                item,
                emoji,
                on,
            } => render_react(chat, item, &emoji, on, out),
            Request::SetMuted { chat, on } => Some(self.render_set_muted(chat, on)),
            Request::SetBlocked { friend, on } => Some(self.render_set_blocked(friend, on)),
            Request::SetMemberBlocked { group, member, on } => {
                let s = serde_json::json!({ "showMessages": !on });
                let cmd = format!("/_member settings #{group} {member} {s}");
                self.send(out, Pending::MemberBlocked(group), cmd);
                None
            }
            Request::ShowCode(friend) => {
                self.send(out, Pending::Code(friend), format!("/_get code @{friend}"));
                None
            }
            Request::Verify { friend, code } => {
                self.render_verify(friend, &code, out);
                None
            }
            Request::Members(group) => {
                self.members_read.insert(group);
                self.send(out, Pending::Members(group), format!("/_members #{group}"));
                None
            }
            Request::MarkRead(chat) => Some((
                RequestKind::MarkRead,
                format!("/_read chat {}", chat_ref(chat)),
            )),
            Request::NewGroup(name) => Some((
                RequestKind::Group,
                format!("/_group {uid} {}", group_profile_json(&name)),
            )),
            Request::AddMember { group, friend } => Some((
                RequestKind::Group,
                format!("/_add #{group} {friend} member"),
            )),
            Request::JoinGroup(g) => Some((RequestKind::Group, format!("/_join #{g}"))),
            Request::LeaveGroup(g) => Some((RequestKind::Group, format!("/_leave #{g}"))),
            // `full` (the friend is told); the mode is a word, not JSON (APIDeleteChat).
            Request::RemoveFriend(c) => {
                Some((RequestKind::RemoveFriend, format!("/_delete @{c} full")))
            }
            Request::Call(c) => {
                self.render_call(c, out);
                None
            }
            Request::Stream(s) => {
                self.render_stream(s, out);
                None
            }
        };
        if let Some((kind, cmd)) = sent {
            self.send(out, Pending::Req(kind), cmd);
        }
    }

    fn render_history(
        &mut self,
        chat: ChatRef,
        before: Option<i64>,
        count: u32,
        tag: u64,
        out: &mut Output,
    ) {
        let page = match before {
            Some(id) => format!("before={id} count={count}"),
            None => format!("count={count}"),
        };
        let cmd = format!("/_get chat {} {page}", chat_ref(chat));
        self.send(out, Pending::History { tag, chat }, cmd);
    }

    /// An invitation or an address: only one of each is made at a time (a retry is exempt).
    fn render_link(&mut self, request: Request, retried: bool, out: &mut Output) {
        let uid = self.uid();
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
    }

    fn render_connect(&mut self, link: &str, retried: bool, out: &mut Output) {
        let link = link.trim().to_owned();
        if link.is_empty() || link.chars().any(char::is_whitespace) {
            out.events.push(failed(
                RequestKind::Connect,
                ErrorCode::ChtBadLink,
                "empty or contains spaces".to_owned(),
            ));
            return;
        }
        let cmd = format!("/_connect plan {} {link}", self.uid());
        let pending = Pending::Plan {
            request: Request::Connect(link),
            retried,
        };
        self.send(out, pending, cmd);
    }
}

fn render_text(
    chat: ChatRef,
    text: &str,
    reply_to: Option<i64>,
    mentions: Vec<(String, i64)>,
) -> (RequestKind, String) {
    let mentions = match chat {
        ChatRef::Group(_) => mentions,
        ChatRef::Direct(_) => Vec::new(),
    };
    let mut m = serde_json::json!({
        "msgContent": { "type": "text", "text": quote_mentions(text, &mentions) },
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

fn render_edit(chat: ChatRef, item: i64, text: &str) -> (RequestKind, String) {
    let m = serde_json::json!({
        "msgContent": { "type": "text", "text": text },
        "mentions": {},
    });
    (
        RequestKind::Edit,
        format!("/_update item {} {item} json {m}", chat_ref(chat)),
    )
}

fn render_react(
    chat: ChatRef,
    item: i64,
    emoji: &str,
    on: bool,
    out: &mut Output,
) -> Option<(RequestKind, String)> {
    let Some(e) = engine_emoji(emoji) else {
        out.events.push(failed(
            RequestKind::React,
            ErrorCode::ChtRequestFailed,
            format!("not a reaction the engine carries: {emoji}"),
        ));
        return None;
    };
    let r = serde_json::json!({ "type": "emoji", "emoji": e });
    let state = if on { "on" } else { "off" };
    Some((
        RequestKind::React,
        format!("/_reaction {} {item} {state} {r}", chat_ref(chat)),
    ))
}
