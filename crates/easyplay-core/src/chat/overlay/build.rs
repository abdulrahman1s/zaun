//! The overlay's snapshot from the Chat snapshot, the open conversation's messages and the
//! toasts the service keeps (ADR-0037 §5).
use std::collections::VecDeque;

use super::notice::Notice;
use super::snapshot::{
    BodyKind, CallLine, CallPerson, Conversation, Link, MAX_CHATS, MAX_MESSAGES, MAX_PEOPLE,
    OverlayCall, OverlayChat, OverlayLook, OverlayMessage, OverlayRing, OverlaySnapshot, Person,
    Status, TOAST_CHARS, Toast,
};
use crate::model::{
    CallKind, CallLink, ChatMessage, ChatRef, ChatSnapshot, MessageAuthor, MessageBody, Presence,
};

/// A voice level from which a person counts as speaking (as `chat::media` reports it).
pub const SPEAKING_LEVEL: u8 = 12;

/// What the snapshot is made of.
pub struct Inputs<'a> {
    pub chat: &'a ChatSnapshot,
    /// The conversation shown and its latest messages, oldest first.
    pub open: Option<(ChatRef, &'a VecDeque<ChatMessage>)>,
    pub toasts: &'a VecDeque<Toast>,
    pub look: OverlayLook,
    /// The running game's title.
    pub game: Option<String>,
    /// Sharing a game is possible in this build.
    pub can_share: bool,
}

/// The snapshot, without pictures (`super::pictures::attach` adds the new ones).
pub fn build(i: &Inputs<'_>) -> OverlaySnapshot {
    let chat = i.chat;
    OverlaySnapshot {
        me: chat.profile.as_ref().map(|p| Person {
            name: p.name.clone(),
            paper: p.paper,
            picture: p.picture.as_ref().map(crate::model::Picture::key),
        }),
        game: i.game.clone(),
        chats: chat
            .chats
            .iter()
            .filter(|p| match p.chat {
                ChatRef::Direct(id) => chat.friend(id).is_none_or(|f| !f.blocked),
                ChatRef::Group(_) => true,
            })
            .take(MAX_CHATS)
            .map(|p| OverlayChat {
                chat: p.chat.into(),
                who: who_of(chat, p.chat, &p.name),
                status: status_of(chat, p.chat),
                unread: p.unread,
                muted: p.muted,
                last: p.last.as_ref().map(|m| message(chat, m)),
            })
            .collect(),
        open: i.open.map(|(c, msgs)| conversation(chat, c, msgs)),
        call: call(chat, i.can_share),
        ringing: chat.live.ringing.as_ref().map(|r| OverlayRing {
            chat: r.chat.into(),
            who: who_of(chat, r.chat, &r.name),
            since: r.since.as_millis(),
        }),
        toasts: i.toasts.iter().cloned().collect(),
        look: i.look.clone(),
        pictures: Vec::new(),
    }
}

fn conversation(chat: &ChatSnapshot, c: ChatRef, msgs: &VecDeque<ChatMessage>) -> Conversation {
    let name = chat.chat_name(c).unwrap_or_default();
    let members = match c {
        ChatRef::Group(g) => chat.group(g).map(|g| {
            let n = g.members.unwrap_or_else(|| count(g.member_list.len()));
            let online = g
                .member_list
                .iter()
                .filter(|m| m.me || m.presence == Presence::Online)
                .count();
            (n, count(online))
        }),
        ChatRef::Direct(_) => None,
    };
    Conversation {
        chat: c.into(),
        who: who_of(chat, c, name),
        status: status_of(chat, c),
        members,
        messages: msgs
            .iter()
            .skip(msgs.len().saturating_sub(MAX_MESSAGES))
            .map(|m| message(chat, m))
            .collect(),
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// One message in the overlay's words.
pub fn message(chat: &ChatSnapshot, m: &ChatMessage) -> OverlayMessage {
    let (kind, text) = body(&m.body);
    let author = match &m.author {
        MessageAuthor::Me => me(chat),
        MessageAuthor::Friend => who_of(chat, m.chat, chat.chat_name(m.chat).unwrap_or_default()),
        MessageAuthor::Member(name) => member(chat, m.chat, name),
    };
    OverlayMessage {
        id: m.id,
        author,
        me: m.author == MessageAuthor::Me,
        kind,
        text,
        notice: match &m.body {
            MessageBody::Notice(n) => Some(Notice::from(n)),
            _ => None,
        },
        call: match &m.body {
            MessageBody::Call(c) => Some(match c.kind {
                CallKind::Ended => CallLine::Ended { minutes: c.minutes },
                CallKind::Missed => CallLine::Missed,
                CallKind::Declined => CallLine::Declined,
            }),
            _ => None,
        },
        at: m.at.as_millis(),
        unread: m.unread,
        mentions: m.mentions.iter().map(|x| x.name.clone()).collect(),
    }
}

/// A toast for a new message from someone else, numbered `id`.
pub fn toast(chat: &ChatSnapshot, m: &ChatMessage, id: u64) -> Toast {
    let (kind, text) = body(&m.body);
    let (who, group) = match (&m.author, m.chat) {
        (MessageAuthor::Member(name), ChatRef::Group(_)) => (
            member(chat, m.chat, name),
            chat.chat_name(m.chat).map(str::to_owned),
        ),
        (_, c) => (who_of(chat, c, chat.chat_name(c).unwrap_or_default()), None),
    };
    Toast {
        id,
        chat: m.chat.into(),
        who,
        group,
        kind,
        text: text.chars().take(TOAST_CHARS).collect(),
    }
}

fn call(chat: &ChatSnapshot, can_share: bool) -> Option<OverlayCall> {
    let c = chat.live.call.as_ref()?;
    let mut people: Vec<CallPerson> = c
        .with
        .iter()
        .map(|name| {
            let speaking = c.speaking.contains(name);
            let who = match c.chat {
                ChatRef::Direct(_) => who_of(chat, c.chat, name),
                ChatRef::Group(_) => member(chat, c.chat, name),
            };
            CallPerson {
                who,
                me: false,
                speaking,
                muted: false,
                level: if speaking { chat.live.levels.friend } else { 0 },
            }
        })
        .collect();
    let more = count(people.len().saturating_sub(MAX_PEOPLE));
    people.truncate(MAX_PEOPLE);
    let level = if c.muted { 0 } else { chat.live.levels.me };
    people.push(CallPerson {
        who: me(chat),
        me: true,
        speaking: level >= SPEAKING_LEVEL,
        muted: c.muted,
        level,
    });
    Some(OverlayCall {
        chat: c.chat.into(),
        name: chat.chat_name(c.chat).unwrap_or_default().to_owned(),
        since: c.since.map(crate::clock::Timestamp::as_millis),
        link: match chat.live.link {
            CallLink::Ringing => Link::Ringing,
            CallLink::Connecting => Link::Connecting,
            CallLink::Connected => Link::Connected,
            CallLink::Interrupted => Link::Interrupted,
        },
        people,
        more,
        muted: c.muted,
        deafened: c.deafened,
        sharing: chat.live.share.as_ref().is_some_and(|s| s.chat == c.chat),
        can_share,
    })
}

fn me(chat: &ChatSnapshot) -> Person {
    chat.profile
        .as_ref()
        .map_or_else(Person::default, |p| Person {
            name: p.name.clone(),
            paper: p.paper,
            picture: p.picture.as_ref().map(crate::model::Picture::key),
        })
}

/// A conversation's face: the friend's or the group's.
fn who_of(chat: &ChatSnapshot, c: ChatRef, name: &str) -> Person {
    let (paper, picture) = match c {
        ChatRef::Direct(id) => chat
            .friend(id)
            .map_or((None, None), |f| (f.paper, f.picture.as_ref())),
        ChatRef::Group(id) => chat
            .group(id)
            .map_or((None, None), |g| (None, g.picture.as_ref())),
    };
    Person {
        name: name.to_owned(),
        paper,
        picture: picture.map(crate::model::Picture::key),
    }
}

/// A group member by name (their picture when the members were read).
fn member(chat: &ChatSnapshot, c: ChatRef, name: &str) -> Person {
    let m = match c {
        ChatRef::Group(g) => chat
            .group(g)
            .and_then(|g| g.member_list.iter().find(|m| m.name == name)),
        ChatRef::Direct(_) => None,
    };
    let friend = m.and_then(|m| m.friend).and_then(|f| chat.friend(f));
    Person {
        name: name.to_owned(),
        paper: friend.and_then(|f| f.paper),
        picture: m
            .and_then(|m| m.picture.as_ref())
            .or_else(|| friend.and_then(|f| f.picture.as_ref()))
            .map(crate::model::Picture::key),
    }
}

fn status_of(chat: &ChatSnapshot, c: ChatRef) -> Option<Status> {
    let ChatRef::Direct(id) = c else { return None };
    chat.friend(id).map(|f| match f.presence {
        Presence::Online => Status::Online,
        Presence::Away => Status::Away,
        Presence::Busy => Status::Busy,
        Presence::Offline => Status::Offline,
    })
}

fn body(b: &MessageBody) -> (BodyKind, String) {
    match b {
        MessageBody::Text(t) => (BodyKind::Text, t.clone()),
        MessageBody::Notice(_) => (BodyKind::Notice, String::new()),
        MessageBody::Voice { .. } => (BodyKind::Voice, String::new()),
        MessageBody::Gif(name) => (BodyKind::Gif, name.clone()),
        MessageBody::Image(m) => (BodyKind::Picture, m.caption.clone()),
        MessageBody::Video(m) => (BodyKind::Video, m.caption.clone()),
        MessageBody::File(m) => (BodyKind::File, m.name.clone()),
        MessageBody::Call(_) => (BodyKind::Call, String::new()),
        MessageBody::Stream(s) => (BodyKind::Stream, s.title.clone()),
        MessageBody::Deleted => (BodyKind::Deleted, String::new()),
    }
}
