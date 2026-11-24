//! One value of every request and event variant.

use std::path::PathBuf;

use zaun_chat_wire::model::*;
use zaun_chat_wire::{Event, FileKind, Request, RequestKind};

pub fn picture() -> Picture {
    Picture {
        bytes: vec![0xff, 0xd8, 0, 1, 2],
        mime: "image/jpeg".into(),
    }
}

pub fn profile() -> ChatProfile {
    ChatProfile {
        name: "Ada".into(),
        about: "hi".into(),
        paper: Some(3),
        status: MyStatus::Busy,
        picture: Some(picture()),
    }
}

pub fn message(body: MessageBody) -> ChatMessage {
    ChatMessage {
        id: 7,
        chat: ChatRef::Group(2),
        author: MessageAuthor::Member("Bo".into()),
        body,
        at: 1_700_000_000_123,
        delivery: Delivery::Received,
        unread: true,
        reply: Some(Quote {
            id: Some(3),
            author: MessageAuthor::Me,
            text: "q".into(),
        }),
        reactions: vec![Reaction {
            emoji: "👍".into(),
            count: 2,
            mine: true,
            who: vec!["Bo".into()],
        }],
        edited: true,
        mentions: vec![Mention {
            name: "Ada".into(),
            member: 1,
            me: true,
        }],
    }
}

fn media(state: FileState) -> Media {
    Media {
        name: "a.png".into(),
        size: 9,
        preview: Some(vec![1, 2, 3, 4]),
        duration: Some(5),
        caption: "c".into(),
        local: Some(PathBuf::from("/d/a.png")),
        state,
    }
}

pub fn bodies() -> Vec<MessageBody> {
    vec![
        MessageBody::Text("t".into()),
        MessageBody::Notice(ChatNotice::MemberAdded {
            by: None,
            who: "x".into(),
        }),
        MessageBody::Voice {
            secs: 2,
            wave: vec![1, 2],
        },
        MessageBody::Gif("g.gif".into()),
        MessageBody::Image(media(FileState::Receiving { percent: 40 })),
        MessageBody::Video(media(FileState::Received(PathBuf::from("/d/v.mp4")))),
        MessageBody::File(media(FileState::Offered)),
        MessageBody::Call(CallRecord {
            kind: CallKind::Missed,
            minutes: 0,
        }),
        MessageBody::Stream(StreamRecord {
            title: "G".into(),
            live: true,
            watching: 2,
            minutes: 3,
        }),
        MessageBody::Deleted,
    ]
}

pub fn notices() -> Vec<ChatNotice> {
    vec![
        ChatNotice::Connected,
        ChatNotice::GroupCreated,
        ChatNotice::YouJoined,
        ChatNotice::MemberJoined("a".into()),
        ChatNotice::MemberAdded {
            by: Some("b".into()),
            who: "a".into(),
        },
        ChatNotice::MemberLeft("a".into()),
        ChatNotice::MemberRemoved {
            by: None,
            who: "a".into(),
        },
        ChatNotice::YouLeft,
        ChatNotice::YouRemoved("b".into()),
        ChatNotice::GroupDeleted,
        ChatNotice::GroupNameChanged("n".into()),
        ChatNotice::NameChanged {
            from: "a".into(),
            to: "b".into(),
        },
        ChatNotice::FriendDeleted,
        ChatNotice::GroupInvitation {
            group: "g".into(),
            sent: true,
        },
        ChatNotice::CodeChanged,
        ChatNotice::MemberBlocked {
            by: Some("b".into()),
            who: "a".into(),
            blocked: true,
        },
        ChatNotice::AddressSwitch {
            phase: SwitchPhase::Started,
            member: Some("a".into()),
        },
        ChatNotice::AddressSwitch {
            phase: SwitchPhase::Completed,
            member: None,
        },
        ChatNotice::RatchetSync {
            status: SyncStatus::Required,
            member: None,
        },
        ChatNotice::BadSignature,
        ChatNotice::Integrity(IntegrityError::Skipped { from: 3, to: 5 }),
        ChatNotice::Integrity(IntegrityError::BadId(9)),
        ChatNotice::Integrity(IntegrityError::BadHash),
        ChatNotice::Integrity(IntegrityError::Duplicate),
        ChatNotice::DecryptionError {
            error: DecryptError::TooManySkipped,
            count: 2,
        },
        ChatNotice::DirectInvitation,
    ]
}

pub fn deliveries() -> Vec<Delivery> {
    vec![
        Delivery::Sending,
        Delivery::Sent,
        Delivery::Delivered,
        Delivery::DeliveredBadHash,
        Delivery::Failed,
        Delivery::Received,
    ]
}

pub fn states() -> Vec<FileState> {
    vec![
        FileState::Offered,
        FileState::Receiving { percent: 1 },
        FileState::Received(PathBuf::from("/x")),
        FileState::Sending { percent: 2 },
        FileState::Sent,
        FileState::Failed,
        FileState::Cancelled,
    ]
}

fn friend() -> Friend {
    Friend {
        id: 1,
        name: "Bo".into(),
        ready: true,
        presence: Presence::Away,
        playing: Some(Playing {
            title: "G".into(),
            minutes: 4,
            live: false,
        }),
        last_seen: Some(10),
        about: "a".into(),
        paper: Some(1),
        since: Some(5),
        verified: true,
        blocked: false,
        muted: true,
        seen: vec!["G".into()],
        picture: Some(picture()),
    }
}

fn group() -> ChatGroup {
    ChatGroup {
        id: 2,
        name: "Gr".into(),
        membership: Membership::Joining,
        members: Some(3),
        member_list: vec![GroupMember {
            id: 1,
            name: "Bo".into(),
            friend: Some(1),
            me: false,
            owner: true,
            presence: Presence::Online,
            playing: None,
            blocked: true,
            picture: None,
        }],
        muted: false,
        voice: vec![1],
        link: Some("l".into()),
        picture: None,
        owner: true,
    }
}

pub fn requests() -> Vec<Request> {
    let chat = ChatRef::Direct(4);
    vec![
        Request::SetProfile(profile()),
        Request::SetPicture(Some(picture())),
        Request::SetGroupPicture {
            group: 2,
            picture: None,
        },
        Request::CreateInvite,
        Request::Connect("link".into()),
        Request::Accept(1),
        Request::Reject(1),
        Request::CreateAddress,
        Request::DeleteAddress,
        Request::Send {
            chat,
            text: "hi".into(),
            reply_to: Some(3),
            mentions: vec![("Bo".into(), 2)],
        },
        Request::SendFile {
            chat,
            path: PathBuf::from("/d/f"),
            kind: FileKind::Video,
            preview: Some(vec![9, 9]),
            duration: Some(3),
            text: String::new(),
        },
        Request::ReceiveFile { chat, item: 1 },
        Request::CancelFile { chat, item: 1 },
        Request::OpenFile { chat, item: 1 },
        Request::LocateFile {
            chat,
            item: 1,
            tag: 8,
        },
        Request::CreateGroupLink(2),
        Request::DeleteGroupLink(2),
        Request::Edit {
            chat,
            item: 1,
            text: "e".into(),
        },
        Request::Delete { chat, item: 1 },
        Request::React {
            chat,
            item: 1,
            emoji: "👍".into(),
            on: true,
        },
        Request::SetMuted { chat, on: true },
        Request::SetBlocked {
            friend: 1,
            on: true,
        },
        Request::SetMemberBlocked {
            group: 2,
            member: 3,
            on: false,
        },
        Request::ShowCode(1),
        Request::Verify {
            friend: 1,
            code: "12345 67890".into(),
        },
        Request::Members(2),
        Request::MarkRead(ChatRef::Group(2)),
        Request::History {
            chat,
            before: None,
            count: 30,
            tag: 1,
        },
        Request::Refresh,
        Request::NewGroup("g".into()),
        Request::AddMember {
            group: 2,
            friend: 1,
        },
        Request::JoinGroup(2),
        Request::LeaveGroup(2),
        Request::RemoveFriend(1),
        Request::Call(CallRequest::Extra {
            friend: 1,
            candidates: vec!["candidate:1".into()],
        }),
        Request::Stream(StreamRequest::Offer {
            friend: 1,
            sdp: "v=0".into(),
        }),
    ]
}

pub fn events() -> Vec<Event> {
    let chat = ChatRef::Group(2);
    vec![
        Event::Features {
            features: zaun_chat_wire::features_with(&[]),
            reactions: Some(vec!["👍".into()]),
        },
        Event::Ready(profile()),
        Event::Profile(profile()),
        Event::Chats {
            friends: vec![friend()],
            groups: vec![group()],
            requests: vec![FriendRequest {
                id: 1,
                name: "Cy".into(),
            }],
            chats: vec![ChatPreview {
                chat,
                name: "Gr".into(),
                last: Some(message(MessageBody::Text("x".into()))),
                unread: 2,
                muted: false,
            }],
        },
        Event::Invite("simplex:/i".into()),
        Event::Address(None),
        Event::Messages(vec![message(MessageBody::Text("x".into()))]),
        Event::Updated(vec![message(MessageBody::Deleted)]),
        Event::History {
            tag: 3,
            chat,
            messages: vec![message(MessageBody::Gif("g".into()))],
        },
        Event::Code {
            friend: 1,
            code: "1".into(),
        },
        Event::Verified {
            friend: 1,
            matched: true,
        },
        Event::Members {
            group: 2,
            members: group().member_list,
        },
        Event::Offline(true),
        Event::Preview {
            chat,
            last: Box::new(message(MessageBody::Text("p".into()))),
        },
        Event::OpenFile {
            path: PathBuf::from("/d/f"),
            kind: FileKind::Image,
        },
        Event::Located { tag: 1, path: None },
        Event::GroupLink {
            group: 2,
            link: Some("l".into()),
        },
        Event::Call(CallSignal::Invited {
            friend: 1,
            usable: true,
            key: Some(CallKey([7; 32])),
        }),
        Event::Stream(StreamSignal::Refused {
            friend: 1,
            detail: "no".into(),
        }),
        Event::Failed {
            request: RequestKind::Send,
            code: "ZN-CHT-005".into(),
            detail: "d".into(),
        },
        Event::Exited {
            status: Some(1),
            started: true,
        },
    ]
}
