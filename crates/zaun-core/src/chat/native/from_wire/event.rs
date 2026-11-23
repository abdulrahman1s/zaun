//! The wire's [`Event`](zaun_chat_wire::Event) as a [`ChatEvent`], variant for variant.

use zaun_chat_wire::Event as W;

use super::media::{call, stream};
use super::message::{message, messages, preview};
use super::model::{
    chat_ref, features, file_kind, friend, group, member, profile, request, request_kind,
};
use crate::chat::engine::ChatEvent;
use crate::error::ErrorCode;

pub(in crate::chat::native) fn event(e: W) -> ChatEvent {
    match e {
        W::Features {
            features: f,
            reactions,
        } => ChatEvent::Features {
            features: features(f),
            reactions,
        },
        W::Ready(p) => ChatEvent::Ready(profile(p)),
        W::Profile(p) => ChatEvent::Profile(profile(p)),
        W::Chats {
            friends,
            groups,
            requests,
            chats,
        } => ChatEvent::Chats {
            friends: friends.into_iter().map(friend).collect(),
            groups: groups.into_iter().map(group).collect(),
            requests: requests.into_iter().map(request).collect(),
            chats: chats.into_iter().map(preview).collect(),
        },
        W::Invite(link) => ChatEvent::Invite(link),
        W::Address(a) => ChatEvent::Address(a),
        W::Messages(ms) => ChatEvent::Messages(messages(ms)),
        W::Updated(ms) => ChatEvent::Updated(messages(ms)),
        W::History {
            tag,
            chat,
            messages: ms,
        } => ChatEvent::History {
            tag,
            chat: chat_ref(chat),
            messages: messages(ms),
        },
        W::Code { friend, code } => ChatEvent::Code { friend, code },
        W::Verified { friend, matched } => ChatEvent::Verified { friend, matched },
        W::Members { group, members } => ChatEvent::Members {
            group,
            members: members.into_iter().map(member).collect(),
        },
        W::Offline(b) => ChatEvent::Offline(b),
        W::Preview { chat, last } => ChatEvent::Preview {
            chat: chat_ref(chat),
            last: Box::new(message(*last)),
        },
        W::OpenFile { path, kind } => ChatEvent::OpenFile {
            path,
            kind: file_kind(kind),
        },
        W::Located { tag, path } => ChatEvent::Located { tag, path },
        W::GroupLink { group, link } => ChatEvent::GroupLink { group, link },
        W::Call(s) => ChatEvent::Call(call(s)),
        W::Stream(s) => ChatEvent::Stream(stream(s)),
        W::Failed {
            request,
            code,
            detail,
        } => failed(request_kind(request), &code, detail),
        W::Exited { status, started } => ChatEvent::Exited { status, started },
    }
}

/// An unknown code is the generic one; what the helper said stays in the log.
fn failed(request: crate::chat::engine::RequestKind, code: &str, detail: String) -> ChatEvent {
    let code = code.parse::<ErrorCode>().unwrap_or_else(|_| {
        tracing::warn!(
            code,
            detail,
            "chat: the helper failed a request with an unknown code"
        );
        ErrorCode::ChtRequestFailed
    });
    ChatEvent::Failed {
        request,
        code,
        detail,
    }
}
