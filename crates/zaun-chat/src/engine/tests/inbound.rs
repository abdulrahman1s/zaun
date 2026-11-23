//! What arrives (codec: a_received_message_…, a_blocked_friend_…, the_relays_going_away_…).

use zaun_chat_wire::model::{ChatNotice, ChatRef, MessageAuthor, MessageBody};
use zaun_chat_wire::{Event, Request};

use super::{T, messages};
use crate::chat::container::MsgContainer;
use crate::chat::content::MsgContent;
use crate::chat::ids::SharedMsgId;
use crate::chat::reaction::MsgReaction;
use crate::chat::{ChatEvent, ChatMessage};
use crate::engine::agent_port::AgentEvent;

#[test]
fn a_received_message_is_unread_text_from_the_friend_acked_with_a_receipt() {
    let mut t = T::started();
    let (_, conn) = t.friend("bob");
    let out = t.peer_says(&conn, 7, ChatEvent::MsgNew(MsgContainer::text("مرحبا")));
    let [m] = messages(&out).try_into().unwrap();
    assert_eq!(
        (m.author, m.body, m.unread),
        (
            MessageAuthor::Friend,
            MessageBody::Text("مرحبا".into()),
            true
        )
    );
    assert_eq!(t.acks(), vec![(7, Some(Vec::new()))]);
}

#[test]
fn a_blocked_friend_is_kept_in_local_data_and_their_new_messages_are_deleted_unseen() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    let out = t.req(Request::SetBlocked {
        friend: bob,
        on: true,
    });
    let Some(Event::Chats { friends, chats, .. }) = out.last() else {
        panic!("{out:?}")
    };
    assert!(
        friends[0].blocked && chats.is_empty(),
        "in Blocked, not in the list"
    );
    let out = t.peer_says(&conn, 3, ChatEvent::MsgNew(MsgContainer::text("hey")));
    assert!(messages(&out).is_empty());
    assert_eq!(
        t.acks(),
        vec![(3, Some(Vec::new()))],
        "acknowledged as for everyone"
    );
    let out = t.req(Request::History {
        chat: ChatRef::Direct(bob),
        before: None,
        count: 10,
        tag: 1,
    });
    let Event::History { messages, .. } = &out[0] else {
        panic!()
    };
    assert!(
        messages
            .iter()
            .all(|m| m.author != MessageAuthor::Friend || matches!(m.body, MessageBody::Notice(_)))
    );
}

#[test]
fn an_edit_a_deletion_and_a_reaction_update_the_friends_item() {
    let mut t = T::started();
    let (_, conn) = t.friend("bob");
    let msg = ChatMessage::new(ChatEvent::MsgNew(MsgContainer::text("helo"))).unwrap();
    let id = msg.msg_id.clone().unwrap();
    let body = crate::chat::encode_chat_message(&msg, false).unwrap();
    t.agent(AgentEvent::Msg {
        conn,
        msg_id: 1,
        body,
    });
    let shared = || SharedMsgId(id.0.clone());
    let edit = ChatEvent::MsgUpdate {
        msg_id: shared(),
        content: MsgContent::Text {
            text: "hello".into(),
        },
        mentions: Default::default(),
        ttl: None,
        live: None,
        scope: None,
        as_group: None,
    };
    let m = messages(&t.peer_says(&conn, 2, edit))[0].clone();
    assert_eq!(
        (m.body, m.edited),
        (MessageBody::Text("hello".into()), true)
    );
    let react = ChatEvent::MsgReact {
        msg_id: shared(),
        member_id: None,
        scope: None,
        reaction: MsgReaction::Emoji('\u{2764}'),
        add: true,
    };
    let m = messages(&t.peer_says(&conn, 3, react))[0].clone();
    assert_eq!(
        (m.reactions[0].emoji.as_str(), m.reactions[0].count),
        ("❤️", 1)
    );
    let del = ChatEvent::MsgDel {
        msg_id: shared(),
        member_id: None,
        scope: None,
        only_history: false,
    };
    let m = messages(&t.peer_says(&conn, 4, del))[0].clone();
    assert_eq!(m.body, MessageBody::Deleted);
    assert!(m.reactions.is_empty());
}

#[test]
fn a_friend_who_deletes_the_contact_leaves_a_notice_and_the_connection_goes() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    let out = t.peer_says(&conn, 5, ChatEvent::DirectDel);
    assert_eq!(
        messages(&out)[0].body,
        MessageBody::Notice(ChatNotice::FriendDeleted)
    );
    let Some(Event::Chats { friends, .. }) = out.last() else {
        panic!("{out:?}")
    };
    assert!(!friends[0].ready);
    assert_eq!(t.acks(), vec![(5, None)]);
    assert!(t.e.agent.calls.contains(&super::Call::Delete { conn }));
    assert!(t.e.state.friends.contains_key(&bob));
}

#[test]
fn unknown_events_and_bad_bodies_are_ignored_but_acked() {
    let mut t = T::started();
    let (_, conn) = t.friend("bob");
    let unknown = ChatEvent::Unknown {
        event: "x.future".into(),
        params: Default::default(),
    };
    assert!(t.peer_says(&conn, 1, unknown).is_empty());
    assert!(
        t.agent(AgentEvent::Msg {
            conn,
            msg_id: 2,
            body: b"\x00garbage".to_vec()
        })
        .is_empty()
    );
    assert_eq!(t.acks(), vec![(1, None), (2, None)]);
}

#[test]
fn the_relays_going_away_and_coming_back_is_reported_once_each() {
    let mut t = T::started();
    let up = |s: &str| AgentEvent::Up { server: s.into() };
    let down = |s: &str| AgentEvent::Down { server: s.into() };
    assert!(t.agent(up("r1")).is_empty(), "online from the start");
    assert_eq!(t.agent(down("r1")), vec![Event::Offline(true)]);
    assert!(t.agent(down("r1")).is_empty());
    assert_eq!(t.agent(up("r1")), vec![Event::Offline(false)]);
    t.agent(up("r2"));
    assert!(
        t.agent(down("r1")).is_empty(),
        "one relay of several down is not offline"
    );
}
