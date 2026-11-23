//! Edit, Delete, React (codec: an_edit_updates_…, deleting_for_everyone_…, reactions_carry_…).

use std::time::Duration;

use zaun_chat_wire::model::{ChatMessage, ChatRef, MessageAuthor, MessageBody};
use zaun_chat_wire::{Event, Request, RequestKind};

use super::{T, messages};
use crate::chat::container::MsgContainer;
use crate::chat::reaction::MsgReaction;
use crate::chat::{ChatEvent, parse_chat_messages};
use crate::engine::agent_port::ConnId;

fn said(t: &mut T, bob: i64, text: &str) -> ChatMessage {
    let out = t.req(Request::Send {
        chat: ChatRef::Direct(bob),
        text: text.into(),
        reply_to: None,
        mentions: Vec::new(),
    });
    messages(&out).pop().unwrap()
}

fn last_sent(t: &T) -> ChatEvent {
    let (_, body) = t.e.agent.sent().pop().unwrap();
    parse_chat_messages(&body).unwrap().pop().unwrap().event
}

fn failed(out: &[Event], kind: RequestKind) -> bool {
    out.iter()
        .any(|e| matches!(e, Event::Failed { request, .. } if *request == kind))
}

fn react(t: &mut T, bob: i64, item: i64, emoji: &str, on: bool) -> Vec<Event> {
    t.req(Request::React {
        chat: ChatRef::Direct(bob),
        item,
        emoji: emoji.into(),
        on,
    })
}

fn theirs(t: &mut T, conn: &ConnId, text: &str) -> ChatMessage {
    messages(&t.peer_says(conn, 1, ChatEvent::MsgNew(MsgContainer::text(text))))[0].clone()
}

#[test]
fn an_edit_sends_the_new_text_and_marks_the_item_edited() {
    let mut t = T::started();
    let (bob, _) = t.friend("bob");
    let m = said(&mut t, bob, "hello bob");
    let out = t.req(Request::Edit {
        chat: ChatRef::Direct(bob),
        item: m.id,
        text: "hello bob (edited)".into(),
    });
    let [u] = messages(&out).try_into().unwrap();
    assert_eq!(
        (u.id, u.body, u.edited),
        (m.id, MessageBody::Text("hello bob (edited)".into()), true)
    );
    let ChatEvent::MsgUpdate {
        content, mentions, ..
    } = last_sent(&t)
    else {
        panic!()
    };
    assert_eq!(content.text(), "hello bob (edited)");
    assert!(mentions.is_empty());
}

#[test]
fn only_the_users_own_messages_from_the_last_day_can_be_edited() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    let m = theirs(&mut t, &conn, "mine to keep");
    let edit = |item| Request::Edit {
        chat: ChatRef::Direct(bob),
        item,
        text: "x".into(),
    };
    assert!(failed(&t.req(edit(m.id)), RequestKind::Edit));
    let old = said(&mut t, bob, "old");
    t.clock.advance(Duration::from_secs(25 * 3600));
    assert!(failed(&t.req(edit(old.id)), RequestKind::Edit));
}

#[test]
fn deleting_for_everyone_sends_the_deletion_and_keeps_the_item_marked_deleted() {
    let mut t = T::started();
    let (bob, _) = t.friend("bob");
    let m = said(&mut t, bob, "oops");
    let out = t.req(Request::Delete {
        chat: ChatRef::Direct(bob),
        item: m.id,
    });
    let [d] = messages(&out).try_into().unwrap();
    assert_eq!((d.id, d.body), (m.id, MessageBody::Deleted));
    assert!(matches!(
        last_sent(&t),
        ChatEvent::MsgDel {
            member_id: None,
            only_history: false,
            ..
        }
    ));
    let again = t.req(Request::Delete {
        chat: ChatRef::Direct(bob),
        item: m.id,
    });
    assert!(failed(&again, RequestKind::Delete), "deleted once");
}

#[test]
fn reactions_carry_only_the_engines_emoji_and_count_the_users_own() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    let m = theirs(&mut t, &conn, "gg");
    let [r] = messages(&react(&mut t, bob, m.id, "❤️", true))
        .try_into()
        .unwrap();
    assert_eq!(
        (
            r.reactions[0].emoji.as_str(),
            r.reactions[0].count,
            r.reactions[0].mine
        ),
        ("❤️", 1, true)
    );
    let ChatEvent::MsgReact { reaction, add, .. } = last_sent(&t) else {
        panic!()
    };
    assert_eq!((reaction, add), (MsgReaction::Emoji('\u{2764}'), true));

    assert!(
        failed(&react(&mut t, bob, m.id, "❤️", true), RequestKind::React),
        "already added"
    );
    let sends = t.e.agent.sent().len();
    assert!(failed(
        &react(&mut t, bob, m.id, "🎮", true),
        RequestKind::React
    ));
    assert_eq!(t.e.agent.sent().len(), sends, "never reaches the friend");

    let [r] = messages(&react(&mut t, bob, m.id, "❤️", false))
        .try_into()
        .unwrap();
    assert!(r.reactions.is_empty());
    assert_eq!(r.author, MessageAuthor::Friend);
}

#[test]
fn a_message_takes_at_most_three_of_the_users_reactions() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    let m = theirs(&mut t, &conn, "gg");
    for e in ["👍", "😀", "🚀"] {
        assert!(!failed(
            &react(&mut t, bob, m.id, e, true),
            RequestKind::React
        ));
    }
    assert!(failed(
        &react(&mut t, bob, m.id, "✅", true),
        RequestKind::React
    ));
}
