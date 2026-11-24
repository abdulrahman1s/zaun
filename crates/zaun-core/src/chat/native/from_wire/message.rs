//! The wire's messages as core's.

use zaun_chat_wire::model as wm;

use super::model::{chat_ref, time};
use super::notice::notice;
use crate::model::{
    CallKind, CallRecord, ChatMessage, ChatPreview, Delivery, FileState, Media, Mention,
    MessageAuthor, MessageBody, Quote, Reaction, StreamRecord,
};

fn author(a: wm::MessageAuthor) -> MessageAuthor {
    match a {
        wm::MessageAuthor::Me => MessageAuthor::Me,
        wm::MessageAuthor::Friend => MessageAuthor::Friend,
        wm::MessageAuthor::Member(name) => MessageAuthor::Member(name),
    }
}

fn file_state(s: wm::FileState) -> FileState {
    match s {
        wm::FileState::Offered => FileState::Offered,
        wm::FileState::Receiving { percent } => FileState::Receiving { percent },
        wm::FileState::Received(path) => FileState::Received(path),
        wm::FileState::Sending { percent } => FileState::Sending { percent },
        wm::FileState::Sent => FileState::Sent,
        wm::FileState::Failed => FileState::Failed,
        wm::FileState::Cancelled => FileState::Cancelled,
    }
}

fn media(m: wm::Media) -> Media {
    Media {
        name: m.name,
        size: m.size,
        preview: m.preview,
        duration: m.duration,
        caption: m.caption,
        local: m.local,
        state: file_state(m.state),
    }
}

fn body(b: wm::MessageBody, by: &wm::MessageAuthor) -> MessageBody {
    match b {
        wm::MessageBody::Text(t) => MessageBody::Text(t),
        wm::MessageBody::Notice(n) => MessageBody::Notice(notice(n, by)),
        wm::MessageBody::Voice { secs, wave } => MessageBody::Voice { secs, wave },
        wm::MessageBody::Gif(name) => MessageBody::Gif(name),
        wm::MessageBody::Image(m) => MessageBody::Image(media(m)),
        wm::MessageBody::Video(m) => MessageBody::Video(media(m)),
        wm::MessageBody::File(m) => MessageBody::File(media(m)),
        wm::MessageBody::Call(c) => MessageBody::Call(CallRecord {
            kind: match c.kind {
                wm::CallKind::Ended => CallKind::Ended,
                wm::CallKind::Missed => CallKind::Missed,
                wm::CallKind::Declined => CallKind::Declined,
            },
            minutes: c.minutes,
        }),
        wm::MessageBody::Stream(s) => MessageBody::Stream(StreamRecord {
            title: s.title,
            live: s.live,
            watching: s.watching,
            minutes: s.minutes,
        }),
        wm::MessageBody::Deleted => MessageBody::Deleted,
    }
}

fn delivery(d: wm::Delivery) -> Delivery {
    match d {
        wm::Delivery::Sending => Delivery::Sending,
        wm::Delivery::Sent => Delivery::Sent,
        wm::Delivery::Delivered => Delivery::Delivered,
        wm::Delivery::DeliveredBadHash => Delivery::DeliveredAltered,
        wm::Delivery::Failed => Delivery::Failed,
        wm::Delivery::Received => Delivery::Received,
    }
}

pub(super) fn message(m: wm::ChatMessage) -> ChatMessage {
    ChatMessage {
        id: m.id,
        chat: chat_ref(m.chat),
        body: body(m.body, &m.author),
        author: author(m.author),
        at: time(m.at),
        delivery: delivery(m.delivery),
        unread: m.unread,
        reply: m.reply.map(|q| Quote {
            id: q.id,
            author: author(q.author),
            text: q.text,
        }),
        reactions: m
            .reactions
            .into_iter()
            .map(|r| Reaction {
                emoji: r.emoji,
                count: r.count,
                mine: r.mine,
                who: r.who,
            })
            .collect(),
        edited: m.edited,
        mentions: m
            .mentions
            .into_iter()
            .map(|n| Mention {
                name: n.name,
                member: n.member,
                me: n.me,
            })
            .collect(),
    }
}

pub(super) fn messages(ms: Vec<wm::ChatMessage>) -> Vec<ChatMessage> {
    ms.into_iter().map(message).collect()
}

pub(super) fn preview(p: wm::ChatPreview) -> ChatPreview {
    ChatPreview {
        chat: chat_ref(p.chat),
        name: p.name,
        last: p.last.map(message),
        unread: p.unread,
        muted: p.muted,
    }
}
