//! Messages in a group, both ways (codec: a_group_message_names_its_author, mentions_go_by_member_number_…, the_list_has_friends_groups_and_unread_counts, muting_…, a_blocked_member_is_hidden_locally_…).

use zaun_chat_wire::model::{ChatMessage, ChatRef, Delivery, Mention, MessageAuthor, MessageBody};
use zaun_chat_wire::{Event, Request, RequestKind};

use super::super::{T, messages};
use crate::chat::container::{MsgContainer, MsgMention, QuotedMsg};
use crate::chat::group::GroupMemberRole;
use crate::chat::ids::MemberId;
use crate::chat::{ChatEvent, parse_chat_messages};
use crate::engine::agent_port::{AgentEvent, ConnId};

pub fn send(group: i64, text: &str, mentions: Vec<(String, i64)>) -> Request {
    Request::Send {
        chat: ChatRef::Group(group),
        text: text.into(),
        reply_to: None,
        mentions,
    }
}

pub fn says(t: &mut T, conn: &ConnId, msg_id: i64, c: MsgContainer) -> Vec<Event> {
    t.peer_says(conn, msg_id, ChatEvent::MsgNew(c))
}

pub fn one(out: &[Event]) -> ChatMessage {
    let [m] = messages(out).try_into().unwrap();
    m
}

pub fn failed(out: &[Event], kind: RequestKind) -> bool {
    out.iter()
        .any(|e| matches!(e, Event::Failed { request, .. } if *request == kind))
}

/// Every event sent to `conn`, in order.
pub fn sent_to(t: &T, conn: &ConnId) -> Vec<ChatEvent> {
    t.e.agent
        .sent()
        .into_iter()
        .filter(|(c, _)| c == conn)
        .flat_map(|(_, b)| parse_chat_messages(&b).unwrap())
        .map(|m| m.event)
        .collect()
}

/// The one `x.msg.new` sent to `conn`.
pub fn only_new(t: &T, conn: &ConnId) -> MsgContainer {
    match sent_to(t, conn).as_slice() {
        [ChatEvent::MsgNew(c)] => c.clone(),
        other => panic!("{other:?}"),
    }
}

pub fn member_id(t: &T, g: i64, member: i64) -> MemberId {
    t.e.state.members[&(g, member)].member_id.clone()
}

fn unread(t: &mut T, g: i64) -> u32 {
    t.req(Request::Refresh)
        .iter()
        .find_map(|e| match e {
            Event::Chats { chats, .. } => chats
                .iter()
                .find(|c| c.chat == ChatRef::Group(g))
                .map(|c| c.unread),
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_group_message_names_its_author() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Member);
    let m = one(&says(
        &mut t,
        &ms[1].1,
        1,
        MsgContainer::text("gg in the group"),
    ));
    assert_eq!(m.chat, ChatRef::Group(g));
    assert_eq!(m.author, MessageAuthor::Member("carol".into()));
    assert_eq!(m.body, MessageBody::Text("gg in the group".into()));
    assert!(m.unread);
    let stored = t.e.item(ChatRef::Group(g), m.id).unwrap().unwrap();
    assert_eq!(stored.member, Some(ms[1].0));
}

#[test]
fn a_message_sent_in_a_group_is_one_item_that_goes_to_every_connected_member() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Owner);
    let m = one(&t.req(send(g, "hello team", Vec::new())));
    assert_eq!((m.author, m.chat), (MessageAuthor::Me, ChatRef::Group(g)));
    for (_, conn) in &ms {
        let c = only_new(&t, conn);
        assert_eq!(c.content.text(), "hello team");
    }
}

#[test]
fn mentions_go_by_member_number_and_a_mention_of_the_user_is_marked() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Member);
    let (bob, bob_conn) = ms[0];
    let m = one(&t.req(send(g, "@bob ready?", vec![("bob".into(), bob)])));
    assert_eq!(
        m.mentions,
        [Mention {
            name: "bob".into(),
            member: bob,
            me: false
        }]
    );
    let c = only_new(&t, &bob_conn);
    assert_eq!(c.mentions["bob"].member_id, member_id(&t, g, bob));

    let mine = t.e.state.groups[&g].my_member_id.clone();
    let mut c = MsgContainer::text("@alice look");
    c.mentions
        .insert("alice".into(), MsgMention { member_id: mine });
    let m = one(&says(&mut t, &ms[1].1, 1, c));
    assert_eq!(
        m.mentions,
        [Mention {
            name: "alice".into(),
            member: 0,
            me: true
        }]
    );
}

#[test]
fn a_name_of_two_words_is_quoted_for_the_apps_and_unquoted_for_the_page() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["Bob Smith", "carol"], GroupMemberRole::Member);
    let (bob, bob_conn) = ms[0];
    let m = one(&t.req(send(
        g,
        "@Bob Smith quoted",
        vec![("Bob Smith".into(), bob)],
    )));
    assert_eq!(m.body, MessageBody::Text("@Bob Smith quoted".into()));
    let c = only_new(&t, &bob_conn);
    assert_eq!(c.content.text(), "@'Bob Smith' quoted");

    let mut c = MsgContainer::text("@'Bob Smith' over here");
    let bob_id = member_id(&t, g, bob);
    c.mentions
        .insert("Bob Smith".into(), MsgMention { member_id: bob_id });
    let m = one(&says(&mut t, &ms[1].1, 1, c));
    assert_eq!(m.body, MessageBody::Text("@Bob Smith over here".into()));
    assert_eq!(m.mentions[0].member, bob);
}

#[test]
fn a_mention_not_in_the_text_or_of_a_member_twice_fails_and_sends_nothing() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Member);
    let bob = ms[0].0;
    let absent = t.req(send(g, "hi all", vec![("bob".into(), bob)]));
    let twice = t.req(send(
        g,
        "@bob and @bobby",
        vec![("bob".into(), bob), ("bobby".into(), bob)],
    ));
    assert!(failed(&absent, RequestKind::Send) && failed(&twice, RequestKind::Send));
    assert!(t.e.agent.sent().is_empty());
}

#[test]
fn a_reply_names_the_quoted_authors_member_id_and_a_received_reply_finds_it() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Member);
    let (bob, bob_conn) = ms[0];
    let theirs = one(&says(&mut t, &bob_conn, 1, MsgContainer::text("gg")));
    let out = t.req(Request::Send {
        chat: ChatRef::Group(g),
        text: "agreed".into(),
        reply_to: Some(theirs.id),
        mentions: Vec::new(),
    });
    let mine = one(&out);
    let q = mine.reply.unwrap();
    assert_eq!(
        (q.id, q.author, q.text.as_str()),
        (Some(theirs.id), MessageAuthor::Member("bob".into()), "gg")
    );
    let c = only_new(&t, &ms[1].1);
    let quote = c.quote.unwrap();
    assert_eq!(quote.msg_ref.member_id, Some(member_id(&t, g, bob)));
    assert!(!quote.msg_ref.sent);

    // Carol answers the user's message: the quote names the user's member id.
    let shared = t.e.item(ChatRef::Group(g), mine.id).unwrap().unwrap();
    let mut msg_ref = quote.msg_ref.clone();
    msg_ref.msg_id = shared.shared_id.map(crate::chat::ids::SharedMsgId);
    msg_ref.member_id = Some(t.e.state.groups[&g].my_member_id.clone());
    let q = QuotedMsg {
        msg_ref,
        content: c.content.clone(),
    };
    let content = crate::chat::content::MsgContent::Text { text: "+1".into() };
    let r = one(&says(&mut t, &ms[1].1, 2, MsgContainer::quote(q, content)));
    let q = r.reply.unwrap();
    assert_eq!((q.id, q.author), (Some(mine.id), MessageAuthor::Me));
}

#[test]
fn a_group_item_shows_the_furthest_any_member_got_and_an_error_leaves_it() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Owner);
    let m = one(&t.req(send(g, "hello", Vec::new())));
    assert_eq!(m.delivery, Delivery::Sending);
    let ids: Vec<i64> = (1..=2).collect();
    let err = t.agent(AgentEvent::Merr {
        conn: ms[0].1,
        msg_id: ids[0],
        err: crate::engine::agent_port::AgentFailure::Other("gone".into()),
    });
    assert!(
        messages(&err).is_empty(),
        "one member's error is not the item's"
    );
    let sent = one(&t.agent(AgentEvent::Sent {
        conn: ms[1].1,
        msg_id: ids[1],
    }));
    assert_eq!((sent.id, sent.delivery), (m.id, Delivery::Sent));
    let got = one(&t.agent(AgentEvent::Rcvd {
        conn: ms[1].1,
        msg_id: ids[1],
    }));
    assert_eq!(got.delivery, Delivery::Delivered);
}

#[test]
fn the_list_counts_a_groups_unread_and_reading_clears_it() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob"], GroupMemberRole::Member);
    says(&mut t, &ms[0].1, 1, MsgContainer::text("one"));
    says(&mut t, &ms[0].1, 2, MsgContainer::text("two"));
    assert_eq!(unread(&mut t, g), 2);
    t.req(Request::MarkRead(ChatRef::Group(g)));
    assert_eq!(unread(&mut t, g), 0);
    let page = t.req(Request::History {
        chat: ChatRef::Group(g),
        before: None,
        count: 10,
        tag: 7,
    });
    let [Event::History { messages, .. }] = page.as_slice() else {
        panic!("{page:?}")
    };
    assert_eq!(messages.len(), 2);
    assert!(messages.iter().all(|m| !m.unread));
}

#[test]
fn muting_a_group_keeps_its_count_and_still_marks_mentions_of_the_user() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob"], GroupMemberRole::Member);
    t.req(Request::SetMuted {
        chat: ChatRef::Group(g),
        on: true,
    });
    assert!(t.e.state.groups[&g].muted);
    let mut c = MsgContainer::text("@alice ping");
    let mine = t.e.state.groups[&g].my_member_id.clone();
    c.mentions
        .insert("alice".into(), MsgMention { member_id: mine });
    let m = one(&says(&mut t, &ms[0].1, 1, c));
    assert!(m.mentions[0].me && m.unread);
    assert_eq!(unread(&mut t, g), 1);
}

#[test]
fn a_blocked_member_is_hidden_locally_and_their_messages_collapse_without_counting() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Member);
    let (bob, conn) = ms[0];
    t.req(Request::SetMemberBlocked {
        group: g,
        member: bob,
        on: true,
    });
    assert!(t.e.state.members[&(g, bob)].blocked);
    assert!(t.e.agent.sent().is_empty(), "they are not told");
    let m = one(&says(
        &mut t,
        &conn,
        1,
        MsgContainer::text("you should not see this"),
    ));
    assert_eq!(m.body, MessageBody::Text("you should not see this".into()));
    assert!(
        !m.unread,
        "a blocked member's message does not count as unread"
    );
    assert_eq!(unread(&mut t, g), 0);
    let mut t = t.restarted();
    assert!(
        t.e.state.members[&(g, bob)].blocked,
        "kept in the member record"
    );
    let page = t.e.group_page(g, None, 10).unwrap();
    assert!(!page[0].unread);
}

#[test]
fn an_observers_message_is_dropped_and_a_message_arriving_twice_is_kept_once() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Member);
    let mut carol = t.e.state.members[&(g, ms[1].0)].clone();
    carol.role = GroupMemberRole::Observer;
    t.e.save_member(carol).unwrap();
    assert!(messages(&says(&mut t, &ms[1].1, 1, MsgContainer::text("hi"))).is_empty());

    let msg = crate::chat::ChatMessage::new(ChatEvent::MsgNew(MsgContainer::text("once"))).unwrap();
    let body = crate::chat::encode_chat_message(&msg, false).unwrap();
    for id in [2, 3] {
        t.agent(AgentEvent::Msg {
            conn: ms[0].1,
            msg_id: id,
            body: body.clone(),
        });
    }
    let all = t.e.group_page(g, None, 10).unwrap();
    assert_eq!(all.len(), 1);
}

#[test]
fn at_most_three_members_are_mentioned_in_a_message_sent() {
    let mut t = T::started();
    let names = ["bob", "carol", "dave", "erin"];
    let (g, ms) = t.group("team", &names, GroupMemberRole::Member);
    let mentions: Vec<(String, i64)> = names
        .iter()
        .zip(&ms)
        .map(|(n, (m, _))| ((*n).to_owned(), *m))
        .collect();
    let out = t.req(send(g, "@bob @carol @dave @erin", mentions));
    assert!(failed(&out, RequestKind::Send));
}

#[test]
fn only_an_owner_posts_as_the_group_and_a_blocked_members_mentions_are_dropped() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Member);
    let mut c = MsgContainer::text("as the group");
    c.as_group = Some(true);
    assert!(messages(&says(&mut t, &ms[1].1, 1, c)).is_empty());

    t.req(Request::SetMemberBlocked {
        group: g,
        member: ms[1].0,
        on: true,
    });
    let mut c = MsgContainer::text("@alice hi");
    let mine = t.e.state.groups[&g].my_member_id.clone();
    c.mentions
        .insert("alice".into(), MsgMention { member_id: mine });
    let m = one(&says(&mut t, &ms[1].1, 2, c));
    assert!(m.mentions.is_empty());
}
