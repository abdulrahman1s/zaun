//! Core's [`Request`] as the wire's, variant for variant.

use zaun_chat_wire::Request as W;

use super::media::{call, stream};
use super::model::{chat_ref as c, file_kind, picture, profile};
use crate::chat::engine::Request;

pub(in crate::chat::native) fn request(r: Request) -> W {
    match r {
        Request::SetProfile(p) => W::SetProfile(profile(p)),
        Request::SetPicture(p) => W::SetPicture(p.map(picture)),
        Request::SetGroupPicture { group, picture: p } => W::SetGroupPicture {
            group,
            picture: p.map(picture),
        },
        Request::CreateInvite => W::CreateInvite,
        Request::Connect(link) => W::Connect(link),
        Request::Accept(id) => W::Accept(id),
        Request::Reject(id) => W::Reject(id),
        Request::CreateAddress => W::CreateAddress,
        Request::DeleteAddress => W::DeleteAddress,
        Request::Send {
            chat,
            text,
            reply_to,
            mentions,
        } => W::Send {
            chat: c(chat),
            text,
            reply_to,
            mentions,
        },
        Request::SendFile {
            chat,
            path,
            kind,
            preview,
            duration,
            text,
        } => W::SendFile {
            chat: c(chat),
            path,
            kind: file_kind(kind),
            preview,
            duration,
            text,
        },
        Request::ReceiveFile { chat, item } => W::ReceiveFile {
            chat: c(chat),
            item,
        },
        Request::CancelFile { chat, item } => W::CancelFile {
            chat: c(chat),
            item,
        },
        Request::OpenFile { chat, item } => W::OpenFile {
            chat: c(chat),
            item,
        },
        Request::LocateFile { chat, item, tag } => W::LocateFile {
            chat: c(chat),
            item,
            tag,
        },
        Request::CreateGroupLink(g) => W::CreateGroupLink(g),
        Request::DeleteGroupLink(g) => W::DeleteGroupLink(g),
        Request::Edit { chat, item, text } => W::Edit {
            chat: c(chat),
            item,
            text,
        },
        Request::Delete { chat, item } => W::Delete {
            chat: c(chat),
            item,
        },
        Request::React {
            chat,
            item,
            emoji,
            on,
        } => W::React {
            chat: c(chat),
            item,
            emoji,
            on,
        },
        Request::SetMuted { chat, on } => W::SetMuted { chat: c(chat), on },
        Request::SetBlocked { friend, on } => W::SetBlocked { friend, on },
        Request::SetMemberBlocked { group, member, on } => {
            W::SetMemberBlocked { group, member, on }
        }
        Request::ShowCode(f) => W::ShowCode(f),
        Request::Verify { friend, code } => W::Verify { friend, code },
        Request::Members(g) => W::Members(g),
        Request::MarkRead(chat) => W::MarkRead(c(chat)),
        Request::History {
            chat,
            before,
            count,
            tag,
        } => W::History {
            chat: c(chat),
            before,
            count,
            tag,
        },
        Request::Refresh => W::Refresh,
        Request::NewGroup(name) => W::NewGroup(name),
        Request::AddMember { group, friend } => W::AddMember { group, friend },
        Request::JoinGroup(g) => W::JoinGroup(g),
        Request::LeaveGroup(g) => W::LeaveGroup(g),
        Request::RemoveFriend(f) => W::RemoveFriend(f),
        Request::Call(r) => W::Call(call(r)),
        Request::Stream(r) => W::Stream(stream(r)),
    }
}
