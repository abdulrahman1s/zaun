//! One value of every core request.

use std::path::PathBuf;

use crate::chat::engine::{FileKind, Request};
use crate::chat::media::{CallRequest, StreamRequest};
use crate::model::{ChatProfile, ChatRef, MyStatus};

/// Every variant's position: a new `Request` variant fails to compile here until it has a
/// sample below.
pub(super) fn index(r: &Request) -> usize {
    match r {
        Request::SetProfile(_) => 0,
        Request::SetPicture(_) => 1,
        Request::SetGroupPicture { .. } => 2,
        Request::CreateInvite => 3,
        Request::Connect(_) => 4,
        Request::Accept(_) => 5,
        Request::Reject(_) => 6,
        Request::CreateAddress => 7,
        Request::DeleteAddress => 8,
        Request::Send { .. } => 9,
        Request::SendFile { .. } => 10,
        Request::ReceiveFile { .. } => 11,
        Request::CancelFile { .. } => 12,
        Request::OpenFile { .. } => 13,
        Request::LocateFile { .. } => 14,
        Request::CreateGroupLink(_) => 15,
        Request::DeleteGroupLink(_) => 16,
        Request::Edit { .. } => 17,
        Request::Delete { .. } => 18,
        Request::React { .. } => 19,
        Request::SetMuted { .. } => 20,
        Request::SetBlocked { .. } => 21,
        Request::SetMemberBlocked { .. } => 22,
        Request::ShowCode(_) => 23,
        Request::Verify { .. } => 24,
        Request::Members(_) => 25,
        Request::MarkRead(_) => 26,
        Request::History { .. } => 27,
        Request::Refresh => 28,
        Request::NewGroup(_) => 29,
        Request::AddMember { .. } => 30,
        Request::JoinGroup(_) => 31,
        Request::LeaveGroup(_) => 32,
        Request::RemoveFriend(_) => 33,
        Request::Call(_) => 34,
        Request::Stream(_) => 35,
    }
}

pub(super) const VARIANTS: usize = 36;

pub(super) fn requests() -> Vec<Request> {
    let g = ChatRef::Group(4);
    let d = ChatRef::Direct(2);
    let mut v = vec![
        Request::SetProfile(ChatProfile {
            name: "Ada".into(),
            about: "hi".into(),
            paper: Some(3),
            status: MyStatus::Busy,
            picture: None,
        }),
        Request::SetPicture(None),
        Request::SetGroupPicture {
            group: 4,
            picture: None,
        },
        Request::CreateInvite,
        Request::Connect("link".into()),
        Request::Accept(1),
        Request::Reject(2),
        Request::CreateAddress,
        Request::DeleteAddress,
        Request::Send {
            chat: g,
            text: "hello @Bo".into(),
            reply_to: Some(9),
            mentions: vec![("Bo".into(), 3)],
        },
        Request::SendFile {
            chat: d,
            path: PathBuf::from("/chat/a.png"),
            kind: FileKind::Gif,
            preview: Some(vec![1, 2, 3, 4]),
            duration: Some(5),
            text: "caption".into(),
        },
        Request::ReceiveFile { chat: d, item: 6 },
        Request::CancelFile { chat: g, item: 7 },
        Request::OpenFile { chat: d, item: 8 },
        Request::LocateFile {
            chat: d,
            item: 8,
            tag: 99,
        },
        Request::CreateGroupLink(4),
        Request::DeleteGroupLink(4),
        Request::Edit {
            chat: d,
            item: 1,
            text: "new".into(),
        },
        Request::Delete { chat: g, item: 2 },
        Request::React {
            chat: g,
            item: 3,
            emoji: "👍".into(),
            on: true,
        },
        Request::SetMuted { chat: g, on: true },
        Request::SetBlocked {
            friend: 2,
            on: false,
        },
        Request::SetMemberBlocked {
            group: 4,
            member: 5,
            on: true,
        },
        Request::ShowCode(2),
        Request::Verify {
            friend: 2,
            code: "12345 67890".into(),
        },
        Request::Members(4),
        Request::MarkRead(d),
        Request::History {
            chat: g,
            before: Some(50),
            count: 30,
            tag: 7,
        },
        Request::Refresh,
        Request::NewGroup("Raid".into()),
        Request::AddMember {
            group: 4,
            friend: 2,
        },
        Request::JoinGroup(4),
        Request::LeaveGroup(4),
        Request::RemoveFriend(2),
        Request::Call(CallRequest::Extra {
            friend: 2,
            candidates: vec!["candidate:1".into()],
        }),
        Request::Stream(StreamRequest::Offer {
            friend: 2,
            sdp: "v=0".into(),
        }),
    ];
    v.sort_by_key(index);
    v
}
