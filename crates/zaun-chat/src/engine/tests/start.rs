//! The start sequence (codec: start_up_…, a_first_start_…, requests_made_while_starting_…).

use zaun_chat_wire::{Event, Request};

use super::{Call, T};
use crate::engine::agent_port::AgentEvent;

#[test]
fn start_up_reports_features_then_ready_then_subscribes_then_the_list() {
    let mut t = T::new();
    t.e.start(&t.dir.hello("alice")).unwrap();
    let out = t.out();
    assert!(matches!(out[0], Event::Features { reactions: Some(ref r), .. } if r.len() == 8));
    let Event::Ready(p) = &out[1] else {
        panic!("{out:?}")
    };
    assert_eq!(p.name, "alice");
    assert!(matches!(out[2], Event::Chats { .. }));
    assert_eq!(out.len(), 3);
    assert_eq!(t.e.agent.calls, vec![Call::SubscribeAll]);
}

#[test]
fn a_first_start_creates_the_user_with_the_given_name_and_a_later_one_keeps_it() {
    let mut t = T::started();
    t.req(Request::SetProfile(zaun_chat_wire::model::ChatProfile {
        name: "  Alice  ".into(),
        ..Default::default()
    }));
    drop(t.e);
    let store = crate::store::Store::open(&t.dir.0.join("chat.db"), super::KEY).unwrap();
    let mut e = crate::engine::Engine::new(
        store,
        super::FakeAgent::default(),
        Box::new(t.clock.clone()),
        crate::engine::log::Log::discard(),
    );
    e.start(&t.dir.hello("someone else")).unwrap();
    e.pop_out();
    let Some(Event::Ready(p)) = e.pop_out() else {
        panic!()
    };
    assert_eq!(p.name, "Alice");
}

#[test]
fn what_subscribing_reports_comes_before_the_list() {
    let mut t = T::new();
    t.e.agent.on_subscribe = vec![AgentEvent::Down {
        server: "smp1".into(),
    }];
    t.e.start(&t.dir.hello("alice")).unwrap();
    let out = t.out();
    assert_eq!(out[2], Event::Offline(true));
    assert!(matches!(out[3], Event::Chats { .. }));
}

#[test]
fn changes_to_who_the_user_talks_to_read_the_list_once_more_at_most() {
    let mut t = T::started();
    let (_, a) = t.friend("bob");
    let (_, b) = t.friend("carol");
    for conn in [a, b] {
        let conn_info = super::x_info("renamed");
        t.e.agent.queued.push(AgentEvent::Info { conn, conn_info });
    }
    t.e.on_request(Request::Refresh);
    let lists = t
        .out()
        .into_iter()
        .filter(|e| matches!(e, Event::Chats { .. }))
        .count();
    assert_eq!(lists, 1, "one list for all the changes");
}
