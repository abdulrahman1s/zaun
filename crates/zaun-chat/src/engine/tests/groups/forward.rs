//! Forwarding: what an admin passes on and how, and forwarded messages taken from their authors (Subscriber.hs `xGrpMsgForward`, delivery jobs; Messages/Batch.hs; fact sheet §11 "Forwarding").

use zaun_chat_wire::model::{ChatRef, MessageAuthor, MessageBody};

use super::super::{T, messages};
use super::messages::{member_id, one};
use crate::chat::batch::encode_binary_batch;
use crate::chat::container::MsgContainer;
use crate::chat::element::{encode_forward_element, parse_element};
use crate::chat::forward::{FwdSender, GrpMsgForward};
use crate::chat::group::GroupMemberRole;
use crate::chat::ids::MemberId;
use crate::chat::limits::{MAX_BATCH_ELEMENT_COUNT, MAX_FORWARD_BATCH_LENGTH};
use crate::chat::{ChatEvent, ChatMessage, parse_chat_elements};
use crate::engine::agent_port::{AgentEvent, ConnId};
use crate::engine::groups::fwd_batch::{forward_batches, fwd_member_name};
use crate::engine::groups::relay::forwardable;
use crate::engine::records::{MemberCategory, MemberStatus};

/// 2026-11-01T10:00:00Z.
const AT_SECS: i64 = 1_793_527_200;

pub fn fwd(member_id: MemberId, name: &str) -> GrpMsgForward {
    GrpMsgForward {
        sender: FwdSender::Member {
            member_id,
            member_name: name.into(),
        },
        broker_ts: "2026-11-01T10:00:00Z".into(),
    }
}

/// One `=` batch holding a `>` element: `inner` forwarded with header `f`.
pub fn forwarded(f: &GrpMsgForward, inner: &[u8]) -> Vec<u8> {
    encode_binary_batch(&[encode_forward_element(f, inner).unwrap()]).unwrap()
}

pub fn json(event: ChatEvent) -> Vec<u8> {
    ChatMessage::new(event).unwrap().to_json().unwrap()
}

pub fn from_host(
    t: &mut T,
    host: &ConnId,
    msg_id: i64,
    body: Vec<u8>,
) -> Vec<zaun_chat_wire::Event> {
    t.agent(AgentEvent::Msg {
        conn: *host,
        msg_id,
        body,
    })
}

#[test]
fn the_element_an_admin_passes_on_names_its_author_and_goes_as_a_binary_batch() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Admin);
    let (bob, carol) = (ms[0], ms[1]);
    let body = json(ChatEvent::MsgNew(MsgContainer::text("hi all")));
    let p = parse_element(&body).unwrap();
    let el = t.e.relay_element(g, bob.0, &p).unwrap();
    t.e.forward_to(g, &[carol.0], &[el]).unwrap();
    let [(to, sent)] = t.e.agent.sent().try_into().unwrap();
    assert_eq!((to, sent[0]), (carol.1, b'='));
    let mut parsed = parse_chat_elements(&sent);
    assert_eq!(parsed.len(), 1);
    let got = parsed.remove(0).unwrap();
    let FwdSender::Member {
        member_id: who,
        member_name,
    } = got.forward.unwrap().sender
    else {
        panic!()
    };
    assert_eq!(
        (who, member_name.as_str()),
        (member_id(&t, g, bob.0), "bob")
    );
    assert_eq!(got.message.event, p.message.event);
}

#[test]
fn only_an_admin_passes_on_and_only_the_events_7_1_4_forwards() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob"], GroupMemberRole::Moderator);
    let p = parse_element(&json(ChatEvent::MsgNew(MsgContainer::text("x")))).unwrap();
    assert!(t.e.relay_element(g, ms[0].0, &p).is_none());
    assert!(forwardable(&ChatEvent::GrpLeave));
    assert!(!forwardable(&ChatEvent::GrpMemConAll {
        member_id: MemberId(b"m".to_vec())
    }));
    let mut c = MsgContainer::text("inline");
    let mut inv = crate::chat::file::FileInvitation::xftp("a", 1);
    inv.file_inline = Some("sent".into());
    c.file = Some(inv);
    assert!(
        !forwardable(&ChatEvent::MsgNew(c)),
        "an inline file is not forwarded"
    );
}

#[test]
fn forward_batches_keep_to_255_elements_and_the_length_and_drop_what_cannot_fit() {
    let small = vec![b'{'; 10];
    let (batches, dropped) = forward_batches(&vec![small; MAX_BATCH_ELEMENT_COUNT + 45]);
    assert_eq!((batches.len(), dropped), (2, 0));
    assert_eq!(batches[0][1], 255);
    assert_eq!(batches[1][1], 45);
    let big = vec![b'{'; MAX_FORWARD_BATCH_LENGTH];
    let third = vec![b'{'; MAX_FORWARD_BATCH_LENGTH / 3 - 10];
    let els = [third.clone(), big, third.clone(), third.clone(), third];
    let (batches, dropped) = forward_batches(&els);
    assert_eq!((batches.len(), dropped), (2, 1));
    assert!(batches.iter().all(|b| b.len() <= MAX_FORWARD_BATCH_LENGTH));
    assert_eq!(fwd_member_name("abcdefghijklmnopq"), "abcdefghijklmnop…");
}

#[test]
fn a_message_the_host_forwards_is_its_authors_at_the_time_the_forward_names() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Member);
    let (bob, carol) = (ms[0], ms[1]);
    let f = fwd(member_id(&t, g, carol.0), "carol");
    let body = forwarded(&f, &json(ChatEvent::MsgNew(MsgContainer::text("via bob"))));
    let m = one(&from_host(&mut t, &bob.1, 1, body));
    assert_eq!(m.author, MessageAuthor::Member("carol".into()));
    assert_eq!((m.at, m.unread), (AT_SECS * 1000, true));
    let stored = t.e.item(ChatRef::Group(g), m.id).unwrap().unwrap();
    assert_eq!(stored.member, Some(carol.0));
}

#[test]
fn a_forward_from_someone_not_admin_or_naming_the_user_is_dropped() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol", "dave"], GroupMemberRole::Member);
    let f = fwd(member_id(&t, g, ms[2].0), "dave");
    let body = forwarded(&f, &json(ChatEvent::MsgNew(MsgContainer::text("x"))));
    assert!(
        messages(&from_host(&mut t, &ms[1].1, 1, body)).is_empty(),
        "carol is a member"
    );
    let me = t.e.state.groups[&g].my_member_id.clone();
    let body = forwarded(
        &fwd(me, "alice"),
        &json(ChatEvent::MsgNew(MsgContainer::text("x"))),
    );
    assert!(messages(&from_host(&mut t, &ms[0].1, 2, body)).is_empty());
}

#[test]
fn only_the_host_or_the_authors_inviter_forwards_a_known_member() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol", "dave"], GroupMemberRole::Member);
    let (carol, dave) = (ms[1], ms[2]);
    let mut c = t.e.state.members[&(g, carol.0)].clone();
    c.role = GroupMemberRole::Admin;
    t.e.save_member(c).unwrap();
    let f = fwd(member_id(&t, g, dave.0), "dave");
    let say = |text: &str| forwarded(&f, &json(ChatEvent::MsgNew(MsgContainer::text(text))));
    assert!(messages(&from_host(&mut t, &carol.1, 1, say("no"))).is_empty());
    let mut d = t.e.state.members[&(g, dave.0)].clone();
    d.invited_by = Some(carol.0);
    t.e.save_member(d).unwrap();
    let m = one(&from_host(&mut t, &carol.1, 2, say("yes")));
    assert_eq!(m.author, MessageAuthor::Member("dave".into()));
}

#[test]
fn an_unknown_author_becomes_a_member_with_status_unknown_but_not_for_a_leave() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob"], GroupMemberRole::Member);
    let stranger = MemberId(b"0123456789abcdef".to_vec());
    let leave = forwarded(&fwd(stranger.clone(), "eve"), &json(ChatEvent::GrpLeave));
    from_host(&mut t, &ms[0].1, 1, leave);
    assert!(t.e.member_by_id(g, &stranger).is_none());
    let body = forwarded(
        &fwd(stranger.clone(), ""),
        &json(ChatEvent::MsgNew(MsgContainer::text("hi"))),
    );
    let m = one(&from_host(&mut t, &ms[0].1, 2, body));
    let eve = t.e.member_by_id(g, &stranger).unwrap().clone();
    assert_eq!(
        (eve.status, eve.category, eve.role.clone()),
        (
            MemberStatus::Unknown,
            MemberCategory::Pre,
            GroupMemberRole::Author
        )
    );
    assert_eq!(eve.profile.display_name, "MDEyMzQ");
    assert_eq!(m.author, MessageAuthor::Member("MDEyMzQ".into()));
}

#[test]
fn the_json_forward_of_older_hosts_is_taken_and_one_nested_in_a_forward_is_not() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Member);
    let carol = member_id(&t, g, ms[1].0);
    let inner = ChatMessage::new(ChatEvent::MsgNew(MsgContainer::text("old style"))).unwrap();
    let event = ChatEvent::GrpMsgForward {
        fwd: fwd(carol.clone(), "carol"),
        msg: Box::new(inner.clone()),
    };
    let m = one(&t.peer_says(&ms[0].1, 1, event.clone()));
    assert_eq!(m.body, MessageBody::Text("old style".into()));
    let nested = forwarded(&fwd(carol, "carol"), &json(event));
    assert!(messages(&from_host(&mut t, &ms[0].1, 2, nested)).is_empty());
}

#[test]
fn history_from_the_host_keeps_each_items_time_and_author_and_counts_as_unread() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Member);
    let host = member_id(&t, g, ms[0].0);
    let older = GrpMsgForward {
        broker_ts: "2026-10-01T08:30:00Z".into(),
        ..fwd(host, "bob")
    };
    let carol = fwd(member_id(&t, g, ms[1].0), "carol");
    let els = [
        encode_forward_element(
            &older,
            &json(ChatEvent::MsgNew(MsgContainer::text("first"))),
        )
        .unwrap(),
        encode_forward_element(
            &carol,
            &json(ChatEvent::MsgNew(MsgContainer::text("second"))),
        )
        .unwrap(),
    ];
    let out = from_host(&mut t, &ms[0].1, 1, encode_binary_batch(&els).unwrap());
    let got = messages(&out);
    let shown: Vec<(MessageAuthor, i64)> = got.iter().map(|m| (m.author.clone(), m.at)).collect();
    assert_eq!(
        shown,
        [
            (MessageAuthor::Member("bob".into()), 1_790_843_400_000),
            (MessageAuthor::Member("carol".into()), AT_SECS * 1000),
        ]
    );
    assert_eq!(t.e.state.groups[&g].unread, 2);
}

#[test]
fn an_admin_passes_on_only_what_it_took_from_a_sender_not_pending_and_never_a_forward() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol", "dave"], GroupMemberRole::Admin);
    let (bob, carol, dave) = (ms[0].0, ms[1].0, ms[2].0);
    let el = || parse_element(&json(ChatEvent::MsgNew(MsgContainer::text("x")))).unwrap();
    assert!(t.e.take_element(g, bob, el()).unwrap().is_some());

    let mut c = t.e.state.members[&(g, carol)].clone();
    c.role = GroupMemberRole::Observer;
    t.e.save_member(c).unwrap();
    assert!(
        t.e.take_element(g, carol, el()).is_err(),
        "refused, so not passed on"
    );

    let mut d = t.e.state.members[&(g, dave)].clone();
    d.status = MemberStatus::PendingApproval;
    t.e.save_member(d).unwrap();
    assert!(t.e.take_element(g, dave, el()).unwrap().is_none());

    // Bob is the host (an owner): what he forwards is taken, not passed on again.
    let f = fwd(member_id(&t, g, dave), "dave");
    let body = forwarded(&f, &json(ChatEvent::MsgNew(MsgContainer::text("y"))));
    let mut parsed = parse_chat_elements(&body);
    let p = parsed.remove(0).unwrap();
    assert!(t.e.take_element(g, bob, p).unwrap().is_none());
}

#[test]
fn a_forward_of_a_removed_member_is_ignored() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Member);
    let mut c = t.e.state.members[&(g, ms[1].0)].clone();
    c.status = MemberStatus::Removed;
    t.e.save_member(c).unwrap();
    let f = fwd(member_id(&t, g, ms[1].0), "carol");
    let body = forwarded(&f, &json(ChatEvent::MsgNew(MsgContainer::text("late"))));
    assert!(messages(&from_host(&mut t, &ms[0].1, 1, body)).is_empty());
}
