//! Local requests (codec: history_asks_for_a_page_…, removing_a_friend_…, the_list_says_who_is_muted_…).

use zaun_chat_wire::model::ChatRef;
use zaun_chat_wire::{Event, Request, RequestKind};

use super::{Call, T};
use crate::chat::container::MsgContainer;
use crate::chat::{ChatEvent, parse_chat_messages};

#[test]
fn history_asks_for_a_page_and_returns_its_messages_oldest_first() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    for i in 0..5 {
        t.peer_says(
            &conn,
            i + 1,
            ChatEvent::MsgNew(MsgContainer::text(&format!("m{i}"))),
        );
    }
    let page = |t: &mut T, before| {
        let out = t.req(Request::History {
            chat: ChatRef::Direct(bob),
            before,
            count: 2,
            tag: 9,
        });
        let [
            Event::History {
                tag: 9, messages, ..
            },
        ] = out.as_slice()
        else {
            panic!("{out:?}")
        };
        messages.iter().map(|m| m.id).collect::<Vec<_>>()
    };
    let newest = page(&mut t, None);
    assert_eq!(newest.len(), 2);
    assert!(newest[0] < newest[1]);
    let older = page(&mut t, Some(newest[0]));
    assert!(older[1] < newest[0]);
}

#[test]
fn reading_a_conversation_clears_its_unread_count() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    t.peer_says(&conn, 1, ChatEvent::MsgNew(MsgContainer::text("a")));
    t.peer_says(&conn, 2, ChatEvent::MsgNew(MsgContainer::text("b")));
    let Event::Chats { chats, .. } = t.req(Request::Refresh).remove(0) else {
        panic!()
    };
    assert_eq!(chats[0].unread, 2);
    assert!(t.req(Request::MarkRead(ChatRef::Direct(bob))).is_empty());
    let Event::Chats { chats, .. } = t.req(Request::Refresh).remove(0) else {
        panic!()
    };
    assert_eq!(chats[0].unread, 0);
    assert!(!chats[0].last.as_ref().unwrap().unread);
}

#[test]
fn muting_is_local_and_shows_in_the_list() {
    let mut t = T::started();
    let (bob, _) = t.friend("bob");
    let sent = t.e.agent.sent().len();
    let out = t.req(Request::SetMuted {
        chat: ChatRef::Direct(bob),
        on: true,
    });
    let [Event::Chats { friends, chats, .. }] = out.as_slice() else {
        panic!("{out:?}")
    };
    assert!(friends[0].muted && chats[0].muted);
    assert_eq!(t.e.agent.sent().len(), sent);
}

#[test]
fn removing_a_friend_tells_them_then_deletes_the_connection_and_the_conversation() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    let out = t.req(Request::RemoveFriend(bob));
    let [Event::Chats { friends, .. }] = out.as_slice() else {
        panic!("{out:?}")
    };
    assert!(friends.is_empty());
    let (_, body) = t.e.agent.sent().pop().unwrap();
    let [x] = parse_chat_messages(&body).unwrap().try_into().unwrap();
    assert_eq!(x.event, ChatEvent::DirectDel);
    assert_eq!(t.e.agent.calls.last(), Some(&Call::Delete { conn }));
    let out = t.req(Request::History {
        chat: ChatRef::Direct(bob),
        before: None,
        count: 5,
        tag: 1,
    });
    assert!(matches!(
        &out[..],
        [Event::Failed {
            request: RequestKind::History,
            ..
        }]
    ));
}

#[test]
fn requests_not_carried_yet_answer_failed_with_their_kind() {
    let mut t = T::started();
    let cases = [
        (Request::CreateAddress, RequestKind::Address, "ZN-CHT-006"),
        (
            Request::NewGroup("g".into()),
            RequestKind::Group,
            "ZN-CHT-006",
        ),
        (
            Request::Call(zaun_chat_wire::model::CallRequest::Invite(1)),
            RequestKind::Call,
            "ZN-CHT-012",
        ),
    ];
    for (r, kind, code) in cases {
        let out = t.req(r);
        assert_eq!(
            out,
            vec![Event::Failed {
                request: kind,
                code: code.into(),
                detail: "not yet".into()
            }]
        );
    }
}
