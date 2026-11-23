//! Two engines joined through what each one sends: alice's host side feeds bob's joiner side and back, so what one encodes the other must read.

use zaun_chat_wire::Request;
use zaun_chat_wire::model::ChatNotice;

use super::members_support::{events, link, notices};
use crate::chat::ChatEvent;
use crate::chat::group::GroupMemberRole;
use crate::chat::version::SUPPORTED_CHAT_VRANGE;
use crate::engine::agent_port::{AgentEvent, ConnId};
use crate::engine::records::{MemberCategory, MemberStatus};
use crate::engine::tests::{Call, T};

fn started_as(name: &str) -> T {
    let mut t = T::new();
    t.e.start(&t.dir.hello(name)).unwrap();
    t.out();
    t
}

fn last_sent(t: &T, conn: ConnId) -> Vec<u8> {
    t.e.agent
        .sent()
        .into_iter()
        .rfind(|(c, _)| *c == conn)
        .unwrap()
        .1
}

fn last_call(t: &T, pick: impl Fn(&Call) -> Option<Vec<u8>>) -> Vec<u8> {
    t.e.agent.calls.iter().rev().find_map(pick).unwrap()
}

#[test]
fn a_host_and_a_joiner_engine_complete_the_join_and_the_first_introduction_from_each_others_bytes()
{
    let mut host = T::started();
    let mut joiner = started_as("bob");
    let (bob, host_to_bob) = host.friend("bob");
    let (_, bob_to_alice) = joiner.friend("alice");
    let (g, carol) = host.group("team", &["carol", "erin"], GroupMemberRole::Owner);
    let (carol, carol_conn) = carol[0];
    host.req(Request::AddMember {
        group: g,
        friend: bob,
    });
    let h1 = host
        .e
        .state
        .members_of(g)
        .find(|m| m.friend == Some(bob))
        .unwrap()
        .conn
        .unwrap();
    host.agent(AgentEvent::Invitation {
        conn: h1,
        full: link(1),
        short: None,
    });
    let inv = last_sent(&host, host_to_bob);
    joiner.agent(AgentEvent::Msg {
        conn: bob_to_alice,
        msg_id: 1,
        body: inv,
    });
    let jg = *joiner.e.state.groups.keys().next().unwrap();
    joiner.req(Request::JoinGroup(jg));
    let acpt = last_call(&joiner, |c| match c {
        Call::Join { conn_info, .. } => Some(conn_info.clone()),
        _ => None,
    });
    host.agent(AgentEvent::Conf {
        conn: h1,
        conn_info: acpt,
    });
    let answer = last_call(&host, |c| match c {
        Call::Allow { conn, conn_info } if *conn == h1 => Some(conn_info.clone()),
        _ => None,
    });
    let j_host = joiner.e.state.groups[&jg].host.unwrap();
    let j1 = joiner.e.state.members[&(jg, j_host)].conn.unwrap();
    joiner.agent(AgentEvent::Info {
        conn: j1,
        conn_info: answer,
    });
    host.agent(AgentEvent::Con { conn: h1 });
    let out = joiner.agent(AgentEvent::Con { conn: j1 });
    assert_eq!(
        notices(&out),
        [
            ChatNotice::YouJoined,
            ChatNotice::MemberJoined("alice".into())
        ]
    );

    let hb = host
        .e
        .state
        .members_of(g)
        .find(|m| m.friend == Some(bob))
        .unwrap()
        .clone();
    assert_eq!(
        (hb.status, hb.profile.display_name.as_str()),
        (MemberStatus::Connected, "bob")
    );
    assert_eq!(hb.chat_v, Some(SUPPORTED_CHAT_VRANGE));
    assert_eq!(hb.member_id, joiner.e.state.groups[&jg].my_member_id);
    let ja = &joiner.e.state.members[&(jg, j_host)];
    assert_eq!(
        (ja.status, ja.profile.display_name.as_str()),
        (MemberStatus::Connected, "alice")
    );
    assert_eq!(ja.member_id, host.e.state.groups[&g].my_member_id);
    assert_eq!(joiner.e.state.groups[&jg].status, MemberStatus::Connected);

    // The host's introductions of carol and erin reach bob as one `=` batch; his link for
    // carol goes back to the host, who passes it to carol.
    let intros = last_sent(&host, h1);
    assert_eq!(intros[0], b'=');
    joiner.agent(AgentEvent::Msg {
        conn: j1,
        msg_id: 2,
        body: intros,
    });
    let jc = joiner
        .e
        .state
        .members_of(jg)
        .find(|m| m.member_id == host.e.state.members[&(g, carol)].member_id)
        .unwrap()
        .clone();
    let pre = joiner.e.state.members_of(jg);
    assert_eq!(pre.filter(|m| m.category == MemberCategory::Pre).count(), 2);
    assert_eq!(jc.member_id, host.e.state.members[&(g, carol)].member_id);
    joiner.agent(AgentEvent::Invitation {
        conn: jc.conn.unwrap(),
        full: link(9),
        short: None,
    });
    let mem_inv = last_sent(&joiner, j1);
    host.agent(AgentEvent::Msg {
        conn: h1,
        msg_id: 1,
        body: mem_inv,
    });
    let fwd = events(&last_sent(&host, carol_conn)).pop().unwrap();
    let ChatEvent::GrpMemFwd { member_info, intro } = fwd else {
        panic!("{fwd:?}")
    };
    assert_eq!(
        (member_info.member_id, intro.group_conn_req),
        (hb.member_id, link(9))
    );
}
