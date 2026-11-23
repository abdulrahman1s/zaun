//! Sending and delivery (codec: sending_renders_…, delivery_updates_…, a_reply_quotes_…).

use zaun_chat_wire::model::{ChatRef, Delivery, MessageAuthor, MessageBody};
use zaun_chat_wire::{Event, Request, RequestKind};

use super::{T, messages};
use crate::chat::container::MsgContainer;
use crate::chat::{ChatEvent, parse_chat_messages};
use crate::engine::agent_port::{AgentEvent, AgentFailure};

fn send(chat: ChatRef, text: &str, reply_to: Option<i64>) -> Request {
    Request::Send {
        chat,
        text: text.into(),
        reply_to,
        mentions: vec![("bob".into(), 1)],
    }
}

#[test]
fn sending_makes_an_item_that_is_sent_and_then_delivered_by_receipt() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    let out = t.req(send(ChatRef::Direct(bob), "gg", None));
    let [m] = messages(&out).try_into().unwrap();
    assert_eq!(
        (m.author, m.body, m.delivery),
        (
            MessageAuthor::Me,
            MessageBody::Text("gg".into()),
            Delivery::Sending
        )
    );
    let (_, body) = t.e.agent.sent().pop().unwrap();
    let [x] = parse_chat_messages(&body).unwrap().try_into().unwrap();
    let ChatEvent::MsgNew(c) = x.event else {
        panic!()
    };
    assert!(c.mentions.is_empty(), "a direct chat carries no mentions");

    let out = t.agent(AgentEvent::Sent { conn, msg_id: 1 });
    assert_eq!(messages(&out)[0].delivery, Delivery::Sent);
    let out = t.agent(AgentEvent::Rcvd { conn, msg_id: 1 });
    let [d] = messages(&out).try_into().unwrap();
    assert_eq!((d.id, d.delivery), (m.id, Delivery::Delivered));
    assert!(
        t.agent(AgentEvent::Sent { conn, msg_id: 1 }).is_empty(),
        "never back to Sent"
    );
}

#[test]
fn a_message_the_relay_gives_up_on_is_failed() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    t.req(send(ChatRef::Direct(bob), "hi", None));
    let out = t.agent(AgentEvent::Merr {
        conn,
        msg_id: 1,
        err: AgentFailure::Other("AUTH".into()),
    });
    assert_eq!(messages(&out)[0].delivery, Delivery::Failed);
}

#[test]
fn a_reply_quotes_by_item_and_a_received_reply_names_what_it_answers() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    let theirs = messages(&t.peer_says(
        &conn,
        1,
        ChatEvent::MsgNew(MsgContainer::text("first\nsecond")),
    ))[0]
        .clone();
    let out = t.req(send(ChatRef::Direct(bob), "re", Some(theirs.id)));
    let q = messages(&out)[0].reply.clone().unwrap();
    assert_eq!(
        (q.id, q.author, q.text.as_str()),
        (Some(theirs.id), MessageAuthor::Friend, "first")
    );
    let (_, body) = t.e.agent.sent().pop().unwrap();
    let [x] = parse_chat_messages(&body).unwrap().try_into().unwrap();
    let ChatEvent::MsgNew(c) = x.event else {
        panic!()
    };
    let quote = c.quote.unwrap();
    assert!(
        !quote.msg_ref.sent,
        "the quoted message is the friend's, not the sender's"
    );

    // Bob answers the user's reply.
    let mine = parse_chat_messages(&body).unwrap().remove(0);
    let mut reply = MsgContainer::text("ok");
    reply.quote = Some(crate::chat::container::QuotedMsg {
        msg_ref: crate::chat::container::MsgRef {
            msg_id: mine.msg_id,
            sent_at: "2026-11-18T11:06:40.000Z".into(),
            sent: false,
            member_id: None,
        },
        content: crate::chat::content::MsgContent::Text { text: "re".into() },
    });
    let out = t.peer_says(&conn, 2, ChatEvent::MsgNew(reply));
    let q = messages(&out)[0].reply.clone().unwrap();
    assert_eq!((q.author, q.text.as_str()), (MessageAuthor::Me, "re"));
    assert!(q.id.is_some());
}

#[test]
fn sending_to_a_group_or_a_friend_not_connected_fails_and_sends_nothing() {
    let mut t = T::started();
    t.req(Request::Connect(super::handshake::INVITATION.into()));
    let pending = *t.e.state.friends.keys().next().unwrap();
    for chat in [
        ChatRef::Group(1),
        ChatRef::Direct(pending),
        ChatRef::Direct(99),
    ] {
        let out = t.req(send(chat, "x", None));
        assert!(
            matches!(
                &out[..],
                [Event::Failed {
                    request: RequestKind::Send,
                    ..
                }]
            ),
            "{out:?}"
        );
    }
    assert!(t.e.agent.sent().is_empty());
}
