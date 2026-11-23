use std::path::PathBuf;

use zaun_chat_wire::model as wm;
use zaun_chat_wire::{Event, FileKind, RequestKind};

use crate::chat::engine::ChatEvent;
use crate::chat::media::CallSignal;
use crate::chat::native::from_wire::{event, features};
use crate::clock::Timestamp;
use crate::error::ErrorCode;
use crate::model::{MessageBody, Presence};

/// A new `Event` variant fails to compile here until it has a sample in [`events`].
fn index(e: &Event) -> usize {
    match e {
        Event::Features { .. } => 0,
        Event::Ready(_) => 1,
        Event::Profile(_) => 2,
        Event::Chats { .. } => 3,
        Event::Invite(_) => 4,
        Event::Address(_) => 5,
        Event::Messages(_) => 6,
        Event::Updated(_) => 7,
        Event::History { .. } => 8,
        Event::Code { .. } => 9,
        Event::Verified { .. } => 10,
        Event::Members { .. } => 11,
        Event::Offline(_) => 12,
        Event::Preview { .. } => 13,
        Event::OpenFile { .. } => 14,
        Event::Located { .. } => 15,
        Event::GroupLink { .. } => 16,
        Event::Call(_) => 17,
        Event::Stream(_) => 18,
        Event::Failed { .. } => 19,
        Event::Exited { .. } => 20,
    }
}

fn text(at: i64) -> wm::ChatMessage {
    wm::ChatMessage {
        id: 7,
        chat: wm::ChatRef::Group(2),
        author: wm::MessageAuthor::Member("Bo".into()),
        body: wm::MessageBody::Text("hi".into()),
        at,
        delivery: wm::Delivery::Received,
        unread: true,
        reply: Some(wm::Quote {
            id: Some(3),
            author: wm::MessageAuthor::Me,
            text: "q".into(),
        }),
        reactions: vec![wm::Reaction {
            emoji: "👍".into(),
            count: 2,
            mine: true,
            who: vec!["Bo".into()],
        }],
        edited: true,
        mentions: vec![wm::Mention {
            name: "Ada".into(),
            member: 1,
            me: true,
        }],
    }
}

fn friend() -> wm::Friend {
    wm::Friend {
        id: 2,
        name: "Bo".into(),
        ready: true,
        presence: wm::Presence::Away,
        playing: Some(wm::Playing {
            title: "Doom".into(),
            minutes: 4,
            live: true,
        }),
        seen: vec!["Quake".into()],
        ..wm::Friend::default()
    }
}

fn events() -> Vec<Event> {
    let group = wm::ChatGroup {
        id: 4,
        name: "Raid".into(),
        member_list: vec![wm::GroupMember {
            id: 1,
            name: "Ada".into(),
            me: true,
            ..wm::GroupMember::default()
        }],
        ..wm::ChatGroup::default()
    };
    let media = wm::Media {
        name: "a.png".into(),
        size: 9,
        preview: Some(vec![1]),
        duration: Some(5),
        caption: "c".into(),
        local: Some(PathBuf::from("/chat/a.png")),
        state: wm::FileState::Receiving { percent: 40 },
    };
    let profile = wm::ChatProfile {
        name: "Ada".into(),
        paper: Some(3),
        status: wm::MyStatus::Away,
        ..wm::ChatProfile::default()
    };
    let mut v = vec![
        Event::Features {
            features: wm::ChatFeatures::default(),
            reactions: Some(vec!["👍".into()]),
        },
        Event::Ready(profile.clone()),
        Event::Profile(profile),
        Event::Chats {
            friends: vec![friend()],
            groups: vec![group],
            requests: vec![wm::FriendRequest {
                id: 1,
                name: "Cy".into(),
            }],
            chats: vec![],
        },
        Event::Invite("link".into()),
        Event::Address(None),
        Event::Messages(vec![]),
        Event::Updated(vec![]),
        Event::History {
            tag: 5,
            chat: wm::ChatRef::Direct(2),
            messages: vec![],
        },
        Event::Code {
            friend: 2,
            code: "1".into(),
        },
        Event::Verified {
            friend: 2,
            matched: true,
        },
        Event::Members {
            group: 4,
            members: vec![wm::GroupMember::default()],
        },
        Event::Offline(true),
        Event::Preview {
            chat: wm::ChatRef::Direct(2),
            last: Box::new(wm::ChatMessage {
                body: wm::MessageBody::Image(media),
                ..text(0)
            }),
        },
        Event::OpenFile {
            path: PathBuf::from("/chat/a"),
            kind: FileKind::Video,
        },
        Event::Located { tag: 1, path: None },
        Event::GroupLink {
            group: 4,
            link: Some("l".into()),
        },
        Event::Call(wm::CallSignal::Ended(2)),
        Event::Stream(wm::StreamSignal::Invited(2)),
        Event::Failed {
            request: RequestKind::Send,
            code: "ZN-CHT-006".into(),
            detail: "d".into(),
        },
        Event::Exited {
            status: Some(1),
            started: true,
        },
    ];
    v.sort_by_key(index);
    v
}

#[test]
fn every_event_variant_has_a_sample_and_converts() {
    let all = events();
    assert_eq!(
        all.iter().map(index).collect::<Vec<_>>(),
        (0..=20).collect::<Vec<_>>()
    );
    for e in all {
        // Variants and fields are named as core's, so what carries no time, key, picture or
        // code prints alike.
        let same = !matches!(
            e,
            Event::Failed { .. } | Event::Preview { .. } | Event::Messages(_) | Event::Chats { .. }
        );
        let want = format!("{e:?}");
        let got = format!("{:?}", event(e));
        if same {
            assert_eq!(got, want);
        }
    }
}

#[test]
fn a_message_keeps_every_field_and_its_time_in_milliseconds() {
    let ChatEvent::Messages(m) = event(Event::Messages(vec![text(1_700_000_000_123)])) else {
        panic!("not Messages");
    };
    assert_eq!(m[0].at, Timestamp::from_millis(1_700_000_000_123));
    assert_eq!(m[0].id, 7);
    assert_eq!(m[0].reactions[0].who, ["Bo"]);
    assert!(m[0].edited && m[0].unread && m[0].mentions[0].me);
    assert_eq!(m[0].body, MessageBody::Text("hi".into()));
}

#[test]
fn a_friends_times_and_presence_come_across() {
    let wf = wm::Friend {
        last_seen: Some(5_000),
        since: Some(9_000),
        ..friend()
    };
    let e = Event::Chats {
        friends: vec![wf],
        groups: vec![],
        requests: vec![],
        chats: vec![wm::ChatPreview {
            chat: wm::ChatRef::Direct(2),
            name: "Bo".into(),
            last: Some(text(42)),
            unread: 3,
            muted: true,
        }],
    };
    let ChatEvent::Chats { friends, chats, .. } = event(e) else {
        panic!("not Chats");
    };
    assert_eq!(friends[0].last_seen, Some(Timestamp::from_millis(5_000)));
    assert_eq!(friends[0].since, Some(Timestamp::from_millis(9_000)));
    assert_eq!(friends[0].presence, Presence::Away);
    assert_eq!(
        chats[0].last.as_ref().map(|m| m.at),
        Some(Timestamp::from_millis(42))
    );
    assert_eq!((chats[0].unread, chats[0].muted), (3, true));
}

#[test]
fn a_call_key_crosses_without_changing() {
    let sig = wm::CallSignal::Invited {
        friend: 2,
        usable: true,
        key: Some(wm::CallKey([9; 32])),
    };
    let ChatEvent::Call(CallSignal::Invited { key, .. }) = event(Event::Call(sig)) else {
        panic!("not Invited");
    };
    assert_eq!(key.map(|k| k.0), Some([9; 32]));
}

fn failed(code: &str) -> ChatEvent {
    event(Event::Failed {
        request: RequestKind::Connect,
        code: code.into(),
        detail: "why".into(),
    })
}

#[test]
fn a_failure_carries_the_error_code_the_helper_named() {
    assert_eq!(
        failed("ZN-CHT-005"),
        ChatEvent::Failed {
            request: crate::chat::engine::RequestKind::Connect,
            code: ErrorCode::ChtBadLink,
            detail: "why".into()
        }
    );
}

#[test]
fn an_unknown_error_code_becomes_the_generic_chat_failure() {
    let ChatEvent::Failed { code, detail, .. } = failed("ZN-NOPE-999") else {
        panic!("not Failed");
    };
    assert_eq!(code, ErrorCode::ChtRequestFailed);
    assert_eq!(detail, "why");
}

#[test]
fn the_features_are_those_the_wire_crate_reports() {
    let f = features(zaun_chat_wire::features_with(&[]));
    assert!(f.files && f.calls && f.share && f.mentions);
    assert!(!f.voice && !f.presence);
}
