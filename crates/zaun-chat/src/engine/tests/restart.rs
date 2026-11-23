//! What survives a restart, and a message that comes again (codec: the_list_has_friends_…).

use std::time::Duration;

use zaun_chat_wire::model::{ChatRef, Delivery};
use zaun_chat_wire::{Event, Request};

use super::{FakeAgent, KEY, T, messages};
use crate::chat::container::MsgContainer;
use crate::chat::{ChatEvent, ChatMessage, encode_chat_message};
use crate::engine::agent_port::AgentEvent;
use crate::engine::log::Log;
use crate::engine::machine::Engine;
use crate::store::Store;

#[test]
fn a_message_that_comes_again_is_kept_once_and_acked_again() {
    let mut t = T::started();
    let (_, conn) = t.friend("bob");
    let msg = ChatMessage::new(ChatEvent::MsgNew(MsgContainer::text("once"))).unwrap();
    let body = encode_chat_message(&msg, false).unwrap();
    let first = t.agent(AgentEvent::Msg {
        conn,
        msg_id: 4,
        body: body.clone(),
    });
    assert_eq!(messages(&first).len(), 1);
    let again = t.agent(AgentEvent::Msg {
        conn,
        msg_id: 4,
        body,
    });
    assert!(messages(&again).is_empty());
    assert_eq!(t.acks().len(), 2);
}

#[test]
fn friends_their_conversations_and_pending_deliveries_survive_a_restart() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    t.peer_says(&conn, 1, ChatEvent::MsgNew(MsgContainer::text("hi")));
    t.req(Request::Send {
        chat: ChatRef::Direct(bob),
        text: "yo".into(),
        reply_to: None,
        mentions: Vec::new(),
    });
    let T { e, clock, dir } = t;
    drop(e);
    clock.advance(Duration::from_secs(60));
    let store = Store::open(&dir.0.join("chat.db"), KEY).unwrap();
    let agent = FakeAgent::default();
    let mut e = Engine::new(store, agent, Box::new(clock.clone()), Log::discard());
    e.start(&dir.hello("alice")).unwrap();
    let out: Vec<Event> = std::iter::from_fn(|| e.pop_out()).collect();
    let Some(Event::Chats { friends, chats, .. }) = out.last() else {
        panic!("{out:?}")
    };
    assert_eq!((friends[0].name.as_str(), friends[0].ready), ("bob", true));
    assert_eq!(chats[0].unread, 1);
    assert!(chats[0].last.is_some());
    e.agent.queued.push(AgentEvent::Rcvd { conn, msg_id: 1 });
    e.pump();
    let out: Vec<Event> = std::iter::from_fn(|| e.pop_out()).collect();
    assert_eq!(messages(&out)[0].delivery, Delivery::Delivered);
    drop(dir);
}
