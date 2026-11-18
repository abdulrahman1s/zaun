//! What the engine reports becomes the snapshot (ADR-0033): new messages update the previews and
//! unread counts at once and are announced as notices, so an open conversation appends them
//! without asking again.
use crate::chat::{ChatEvent, Request};
use crate::model::{
    ChatMessage, ChatPreview, ChatProfile, ChatSnapshot, ChatStatus, MessageAuthor,
};
use crate::service::event::ChatPage;
use crate::service::prelude::*;

impl AppService {
    pub(super) fn on_chat_event(&mut self, event: ChatEvent) -> Result<()> {
        let paper = self.flow.chat.paper;
        let snap = &mut self.flow.chat.snapshot;
        match event {
            ChatEvent::Features {
                features,
                reactions,
            } => {
                snap.features = features;
                snap.reaction_emoji = reactions;
            }
            ChatEvent::Ready(profile) => {
                self.flow.chat.failures = 0;
                snap.profile = Some(local_profile(snap, profile, paper));
                snap.status = ChatStatus::Ready;
                // A new engine reports going offline afresh.
                snap.offline = false;
            }
            ChatEvent::Profile(profile) => snap.profile = Some(local_profile(snap, profile, paper)),
            ChatEvent::Chats {
                friends,
                groups,
                requests,
                chats,
            } => {
                snap.groups = keep_member_details(snap, groups);
                snap.friends = friends;
                snap.requests = requests;
                snap.chats = chats;
            }
            ChatEvent::Code { friend, code } => snap.code = Some((friend, code)),
            ChatEvent::Verified { friend, matched } => return self.on_verified(friend, matched),
            ChatEvent::Members { group, members } => {
                if let Some(g) = snap.group_mut(group) {
                    g.members = u32::try_from(members.len()).ok();
                    g.member_list = members;
                }
            }
            ChatEvent::Offline(offline) => snap.offline = offline,
            ChatEvent::Preview { chat, last } => {
                if let Some(p) = snap.preview_mut(chat)
                    && p.last.as_ref().is_none_or(|l| l.id < last.id)
                {
                    p.last = Some(*last);
                }
            }
            ChatEvent::OpenFile { path, kind } => {
                self.chat_open_file(&path, kind);
                return Ok(());
            }
            ChatEvent::Located { tag, path } => return self.on_located(tag, path),
            ChatEvent::GroupLink { group, link } => {
                if let Some(g) = snap.group_mut(group) {
                    g.link = link;
                }
            }
            ChatEvent::Invite(link) => snap.invite = Some(link),
            ChatEvent::Address(a) => snap.address = a,
            ChatEvent::Messages(messages) => self.on_new_messages(messages),
            ChatEvent::Updated(messages) => self.on_updated_messages(messages),
            ChatEvent::History {
                tag,
                chat,
                messages,
            } => {
                self.on_history(tag, chat, messages);
                return Ok(());
            }
            ChatEvent::Failed {
                request,
                code,
                detail,
            } => return self.on_chat_failed(request, code, detail),
            ChatEvent::Exited { status, started } => return self.on_chat_exited(status, started),
            ChatEvent::Call(signal) => return self.on_call_signal(signal),
            ChatEvent::Stream(signal) => return self.on_stream_signal(signal),
        }
        self.stream_mark_friends();
        self.chat_publish();
        Ok(())
    }

    fn on_verified(&mut self, friend: i64, matched: bool) -> Result<()> {
        if let Some(f) = self.flow.chat.snapshot.friend_mut(friend) {
            f.verified = matched;
        }
        self.chat_publish();
        ensure!(
            matched,
            ChtRequestFailed,
            detail = "the security codes differ"
        );
        Ok(())
    }

    fn on_new_messages(&mut self, messages: Vec<ChatMessage>) {
        let snap = &mut self.flow.chat.snapshot;
        let unknown = messages.iter().any(|m| !preview_new(snap, m));
        if unknown {
            self.chat_request(Request::Refresh);
        }
        self.overlay_messages(&messages);
        self.emit(Event::Notice(Notice::ChatMessages(messages)));
    }

    fn on_updated_messages(&mut self, messages: Vec<ChatMessage>) {
        for m in &messages {
            if let Some(p) = self.flow.chat.snapshot.preview_mut(m.chat)
                && p.last.as_ref().is_some_and(|l| l.id == m.id)
            {
                p.last = Some(m.clone());
            }
        }
        self.overlay_updated(&messages);
        self.emit(Event::Notice(Notice::ChatMessagesUpdated(messages)));
    }

    fn on_history(&mut self, tag: u64, chat: ChatRef, messages: Vec<ChatMessage>) {
        if let Some(id) = self.flow.chat.pages.remove(&tag) {
            if self.overlay_history(id, chat, &messages) {
                return;
            }
            self.emit(Event::QueryResult {
                id,
                result: QueryResult::Chat(ChatPage { chat, messages }),
            });
        }
    }
}

/// The profile the engine reports, with what EasyPlay keeps itself: the avatar pair and the
/// status the user chose.
fn local_profile(snap: &ChatSnapshot, mut p: ChatProfile, paper: Option<u8>) -> ChatProfile {
    p.paper = paper;
    if let Some(old) = &snap.profile {
        p.status = old.status;
    }
    p
}

/// Members and invite links are read separately: the new list keeps what the old one had.
fn keep_member_details(
    snap: &ChatSnapshot,
    mut groups: Vec<crate::model::ChatGroup>,
) -> Vec<crate::model::ChatGroup> {
    for g in &mut groups {
        if let Some(old) = snap.group(g.id) {
            g.member_list.clone_from(&old.member_list);
            g.link.clone_from(&old.link);
        }
    }
    groups
}

/// Put a new message into its conversation's preview (moving it to the top). `false` when
/// the conversation is not in the list yet.
fn preview_new(snap: &mut ChatSnapshot, m: &ChatMessage) -> bool {
    let Some(i) = snap.chats.iter().position(|p| p.chat == m.chat) else {
        return false;
    };
    let mut p: ChatPreview = snap.chats.remove(i);
    if p.last.as_ref().is_none_or(|l| l.id <= m.id) {
        p.last = Some(m.clone());
    }
    if m.unread && m.author != MessageAuthor::Me {
        p.unread += 1;
    }
    snap.chats.insert(0, p);
    true
}
