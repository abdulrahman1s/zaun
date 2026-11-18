use std::collections::{BTreeSet, VecDeque};

use super::build::{Inputs, build, toast};
use super::pictures::{attach, referenced};
use super::snapshot::{
    BodyKind, CallLine, ChatKey, Link, MAX_CHATS, MAX_MESSAGES, OverlayLook, Status, TOAST_CHARS,
};
use crate::clock::Timestamp;
use crate::model::{
    Call, CallKind, CallLevels, CallLink, CallRecord, ChatGroup, ChatLive, ChatMessage, ChatNotice,
    ChatPreview, ChatProfile, ChatRef, ChatSnapshot, Delivery, Friend, GroupMember, MessageAuthor,
    MessageBody, Picture, Presence, Ringing,
};

fn msg(chat: ChatRef, id: i64, author: MessageAuthor, body: MessageBody) -> ChatMessage {
    ChatMessage {
        id,
        chat,
        author,
        body,
        at: Timestamp::from_millis(1_000 + id),
        delivery: Delivery::Received,
        unread: true,
        reply: None,
        reactions: Vec::new(),
        edited: false,
        mentions: Vec::new(),
    }
}

fn preview(chat: ChatRef, name: &str, unread: u32) -> ChatPreview {
    ChatPreview {
        chat,
        name: name.into(),
        last: None,
        unread,
        muted: false,
    }
}

fn ana_picture() -> Picture {
    Picture::new(vec![0x89, b'P', b'N', b'G', 1, 2, 3])
}

fn snap() -> ChatSnapshot {
    let mut chats = vec![
        preview(ChatRef::Direct(1), "Ana", 2),
        preview(ChatRef::Direct(2), "Blocked", 9),
        preview(ChatRef::Group(5), "Raid", 0),
    ];
    chats.extend((10..20).map(|i| preview(ChatRef::Direct(i), "x", 0)));
    ChatSnapshot {
        profile: Some(ChatProfile {
            name: "Rin".into(),
            paper: Some(4),
            ..ChatProfile::default()
        }),
        friends: vec![
            Friend {
                id: 1,
                name: "Ana".into(),
                paper: Some(3),
                presence: Presence::Online,
                picture: Some(ana_picture()),
                ..Friend::default()
            },
            Friend {
                id: 2,
                name: "Blocked".into(),
                blocked: true,
                ..Friend::default()
            },
        ],
        groups: vec![ChatGroup {
            id: 5,
            name: "Raid".into(),
            members: Some(4),
            member_list: vec![
                GroupMember {
                    id: 1,
                    name: "Bo".into(),
                    presence: Presence::Online,
                    ..GroupMember::default()
                },
                GroupMember {
                    id: 2,
                    name: "Rin".into(),
                    me: true,
                    ..GroupMember::default()
                },
            ],
            ..ChatGroup::default()
        }],
        chats,
        live: ChatLive {
            call: Some(Call {
                chat: ChatRef::Direct(1),
                since: Some(Timestamp::from_millis(77)),
                with: vec!["Ana".into()],
                speaking: vec!["Ana".into()],
                muted: true,
                deafened: false,
            }),
            link: CallLink::Interrupted,
            levels: CallLevels { me: 40, friend: 65 },
            ..ChatLive::default()
        },
        ..ChatSnapshot::default()
    }
}

fn inputs<'a>(
    chat: &'a ChatSnapshot,
    open: Option<(ChatRef, &'a VecDeque<ChatMessage>)>,
    toasts: &'a VecDeque<super::snapshot::Toast>,
) -> Inputs<'a> {
    Inputs {
        chat,
        open,
        toasts,
        look: OverlayLook::default(),
        game: Some("Lantern Keep".into()),
        can_share: true,
    }
}

#[test]
fn the_list_is_the_most_recent_conversations_without_blocked_friends() {
    let c = snap();
    let s = build(&inputs(&c, None, &VecDeque::new()));
    assert_eq!(s.chats.len(), MAX_CHATS);
    assert_eq!(s.chats[0].chat, ChatKey::Direct(1));
    assert_eq!(
        (s.chats[0].who.paper, s.chats[0].status, s.chats[0].unread),
        (Some(3), Some(Status::Online), 2)
    );
    assert_eq!(s.chats[0].who.picture, Some(ana_picture().key()));
    assert_eq!(
        (s.chats[1].chat, s.chats[1].status),
        (ChatKey::Group(5), None)
    );
    assert!(s.chats.iter().all(|c| c.who.name != "Blocked"));
    assert_eq!(s.game.as_deref(), Some("Lantern Keep"));
}

#[test]
fn the_call_lists_everyone_with_the_user_last_and_speaks_their_levels() {
    let c = snap();
    let call = build(&inputs(&c, None, &VecDeque::new()))
        .call
        .expect("call");
    assert_eq!(
        (call.name.as_str(), call.since, call.link),
        ("Ana", Some(77), Link::Interrupted)
    );
    assert!(call.muted && !call.deafened && call.can_share && !call.sharing);
    let names: Vec<&str> = call.people.iter().map(|p| p.who.name.as_str()).collect();
    assert_eq!(names, vec!["Ana", "Rin"]);
    assert!(call.people[0].speaking && call.people[0].level == 65);
    // Muted: the user's level is 0 and they do not speak.
    assert!(call.people[1].me && call.people[1].muted && !call.people[1].speaking);
}

#[test]
fn a_ringing_call_carries_the_caller_s_face() {
    let mut c = snap();
    c.live.ringing = Some(Ringing {
        chat: ChatRef::Direct(1),
        name: "Ana".into(),
        since: Timestamp::from_millis(9),
    });
    let r = build(&inputs(&c, None, &VecDeque::new()))
        .ringing
        .expect("ringing");
    assert_eq!(
        (r.chat, r.who.paper, r.since),
        (ChatKey::Direct(1), Some(3), 9)
    );
}

#[test]
fn the_open_group_carries_its_latest_messages_members_and_events() {
    let c = snap();
    let g = ChatRef::Group(5);
    let mut msgs: VecDeque<ChatMessage> = (0..40)
        .map(|i| {
            msg(
                g,
                i,
                MessageAuthor::Member("Bo".into()),
                MessageBody::Text(format!("m{i}")),
            )
        })
        .collect();
    msgs.push_back(msg(
        g,
        50,
        MessageAuthor::Member("Bo".into()),
        MessageBody::Notice(ChatNotice::MemberLeft("Kit".into())),
    ));
    msgs.push_back(msg(
        g,
        51,
        MessageAuthor::Me,
        MessageBody::Call(CallRecord {
            kind: CallKind::Ended,
            minutes: 38,
        }),
    ));
    let s = build(&inputs(&c, Some((g, &msgs)), &VecDeque::new()));
    let open = s.open.expect("open");
    assert_eq!(
        (open.who.name.as_str(), open.members),
        ("Raid", Some((4, 2)))
    );
    assert_eq!(open.messages.len(), MAX_MESSAGES);
    assert_eq!(open.messages[0].author.name, "Bo");
    let notice = &open.messages[MAX_MESSAGES - 2];
    assert_eq!(notice.kind, BodyKind::Notice);
    assert!(notice.notice.is_some());
    let call = &open.messages[MAX_MESSAGES - 1];
    assert_eq!(
        (call.me, call.call),
        (true, Some(CallLine::Ended { minutes: 38 }))
    );
    assert_eq!(call.author.name, "Rin");
}

#[test]
fn a_toast_names_the_member_and_the_group_and_is_cut_short() {
    let c = snap();
    let long = "a".repeat(500);
    let t = toast(
        &c,
        &msg(
            ChatRef::Group(5),
            1,
            MessageAuthor::Member("Bo".into()),
            MessageBody::Text(long),
        ),
        9,
    );
    assert_eq!(
        (t.id, t.who.name.as_str(), t.group.as_deref()),
        (9, "Bo", Some("Raid"))
    );
    assert_eq!(t.text.chars().count(), TOAST_CHARS);
    let d = toast(
        &c,
        &msg(
            ChatRef::Direct(1),
            2,
            MessageAuthor::Friend,
            MessageBody::Text("hi".into()),
        ),
        10,
    );
    assert_eq!(
        (d.who.name.as_str(), d.who.paper, d.group),
        ("Ana", Some(3), None)
    );
}

#[test]
fn a_picture_is_sent_once_and_again_only_after_it_left_the_screen() {
    let c = snap();
    let mut s = build(&inputs(&c, None, &VecDeque::new()));
    let key = ana_picture().key();
    assert!(referenced(&s).contains(&key));
    let mut held = BTreeSet::new();
    attach(&mut s, &c, &mut held);
    assert_eq!(s.pictures.len(), 1);
    assert_eq!(s.pictures[0].key, key);
    attach(&mut s, &c, &mut held);
    assert!(s.pictures.is_empty(), "the overlay holds it already");
    let mut gone = build(&inputs(&ChatSnapshot::default(), None, &VecDeque::new()));
    attach(&mut gone, &ChatSnapshot::default(), &mut held);
    assert!(held.is_empty());
    attach(&mut s, &c, &mut held);
    assert_eq!(s.pictures.len(), 1, "sent again after it left");
}
