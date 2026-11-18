//! Answers that carry chat data: the list, a page, a preview, codes, members, files, links.
use super::respond::Answer;
use super::*;

impl Codec {
    pub(super) fn on_chats(&mut self, a: Answer<'_>, out: &mut Output) {
        self.chats_in_flight = false;
        if let (Some("apiChats"), Some(r)) = (a.ty, a.result) {
            let list = self.parse_chats(r);
            out.events.push(list);
            self.read_hidden_previews(out);
            self.read_group_links(out);
        } else {
            out.events.push(a.failed(RequestKind::Refresh));
        }
        if std::mem::take(&mut self.chats_again) {
            self.refresh(out);
        }
    }

    /// The latest shown message of chats whose latest item is hidden.
    fn read_hidden_previews(&mut self, out: &mut Output) {
        for (chat, last) in std::mem::take(&mut self.previews_to_read) {
            let pending = Pending::Preview { chat, last };
            let cmd = format!("/_get chat {} count={PREVIEW_DEPTH}", chat_ref(chat));
            self.send(out, pending, cmd);
        }
    }

    /// The invite link of each group the user runs, once per start.
    fn read_group_links(&mut self, out: &mut Output) {
        for g in std::mem::take(&mut self.links_to_read) {
            if self.group_links_read.insert(g) {
                let pending = Pending::GroupLink {
                    group: g,
                    read: true,
                };
                self.send(out, pending, format!("/_get link #{g}"));
            }
        }
    }

    pub(super) fn on_history(&mut self, tag: u64, chat: ChatRef, a: Answer<'_>, out: &mut Output) {
        let c = a.of_type("apiChat").and_then(|r| r.get("chat"));
        let items = c.and_then(|c| c.get("chatItems")).and_then(Value::as_array);
        let Some(items) = items else {
            out.events.push(a.failed(RequestKind::History));
            return;
        };
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

    pub(super) fn on_preview(&mut self, chat: ChatRef, last: i64, a: Answer<'_>, out: &mut Output) {
        let Some(c) = a.of_type("apiChat").and_then(|r| r.get("chat")) else {
            tracing::info!(error = %a.error_text(), "chat: a preview was not read");
            return;
        };
        let ctx = self.ctx(c.get("chatInfo"), chat);
        let shown = c
            .get("chatItems")
            .and_then(Value::as_array)
            .and_then(|items| items.iter().rev().find_map(|ci| parse_item(chat, ci, &ctx)));
        self.previews.insert(chat, (last, shown.clone()));
        if let Some(m) = shown {
            out.events.push(ChatEvent::Preview {
                chat,
                last: Box::new(m),
            });
        }
    }

    pub(super) fn on_verify(&mut self, friend: i64, a: Answer<'_>, out: &mut Output) {
        let verified = a
            .of_type("connectionVerified")
            .and_then(|r| r.get("verified"))
            .and_then(Value::as_bool);
        match verified {
            Some(matched) => {
                out.events.push(ChatEvent::Verified { friend, matched });
                self.refresh(out);
            }
            None => out.events.push(a.failed(RequestKind::Code)),
        }
    }

    pub(super) fn on_member_blocked(&mut self, group: i64, a: Answer<'_>, out: &mut Output) {
        if let Some(e) = a.error {
            let code = error_code(RequestKind::Block, e);
            out.events.push(failed(RequestKind::Block, code, short(e)));
        } else {
            self.members_read.insert(group);
            self.send(out, Pending::Members(group), format!("/_members #{group}"));
        }
    }

    pub(super) fn on_file(&mut self, cancel: bool, a: Answer<'_>, out: &mut Output) {
        if let Some(e) = a.error {
            let code = error_code(RequestKind::File, e);
            out.events.push(failed(RequestKind::File, code, short(e)));
            return;
        }
        let m = (a.result)
            .and_then(|r| r.get("chatItem").or_else(|| r.get("chatItem_")))
            .filter(|i| !i.is_null())
            .and_then(|i| self.parse_a_item(i));
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
}

pub(super) fn on_code(friend: i64, a: Answer<'_>, out: &mut Output) {
    let code = a
        .of_type("contactCode")
        .and_then(|r| r.get("connectionCode"))
        .and_then(Value::as_str);
    match code {
        Some(code) => out.events.push(ChatEvent::Code {
            friend,
            code: code.to_owned(),
        }),
        None => out.events.push(a.failed(RequestKind::Code)),
    }
}

pub(super) fn on_members(group: i64, a: Answer<'_>, out: &mut Output) {
    match a.of_type("groupMembers").and_then(|r| r.get("group")) {
        Some(g) => out.events.push(ChatEvent::Members {
            group,
            members: parse_members(g),
        }),
        None => out.events.push(a.failed(RequestKind::Members)),
    }
}

pub(super) fn on_group_link(group: i64, read: bool, a: Answer<'_>, out: &mut Output) {
    match (a.ty, a.result) {
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
            let e = a.error.cloned().unwrap_or(Value::Null);
            if read || e.to_string().contains("groupLinkNotFound") {
                out.events.push(ChatEvent::GroupLink { group, link: None });
            } else {
                let code = error_code(RequestKind::GroupLink, &e);
                out.events
                    .push(failed(RequestKind::GroupLink, code, short(&e)));
            }
        }
    }
}
