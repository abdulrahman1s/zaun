//! Invitations and connecting (codec: an_invitation_…, connecting_…, the_users_own_link_…).

use zaun_chat_wire::model::{ChatNotice, MessageBody};
use zaun_chat_wire::{Event, Request, RequestKind};

use super::{Call, T, messages, x_info};
use crate::engine::agent_port::{AgentEvent, AgentFailure};

pub const INVITATION: &str = "https://smp5.simplex.im/i#esW_-BXJI9zs4iGRV2SMHUzqpFv3QRe6/KaKvW0lOkJzcvIRVW-l0uomDrjYKjiaOkPIkxSvOB0o";
const ADDRESS: &str = "https://smp6.simplex.im/a#0sPWloEa5zFH0sfc_vANe4RUUebBQSGcvTKpKRH300s";

fn failed(out: &[Event]) -> (RequestKind, String) {
    match out {
        [Event::Failed { request, code, .. }] => (*request, code.clone()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_invitation_is_shared_as_its_short_link_and_falls_back_to_the_full_one() {
    let mut t = T::started();
    for (short, shown) in [
        (Some("https://r/i#s"), "https://r/i#s"),
        (None, "simplex:/f"),
    ] {
        t.req(Request::CreateInvite);
        let conn = t.e.state.inviting.unwrap();
        let out = t.agent(AgentEvent::Invitation {
            conn,
            full: "simplex:/f".into(),
            short: short.map(Into::into),
        });
        assert_eq!(out, vec![Event::Invite(shown.into())]);
    }
}

#[test]
fn only_one_invitation_is_made_at_a_time() {
    let mut t = T::started();
    t.req(Request::CreateInvite);
    t.req(Request::CreateInvite);
    assert_eq!(
        t.e.agent
            .calls
            .iter()
            .filter(|c| **c == Call::CreateInvitation)
            .count(),
        1
    );
}

#[test]
fn an_invitation_whose_relay_does_not_answer_is_no_connection() {
    let mut t = T::started();
    t.e.agent.fail_next = Some(AgentFailure::Network("timeout".into()));
    let out = t.req(Request::CreateInvite);
    assert_eq!(
        failed(&out),
        (RequestKind::CreateInvite, "ZN-CHT-004".into())
    );
    assert!(t.e.state.inviting.is_none(), "a new one can be asked for");
}

#[test]
fn someone_answering_the_invitation_gets_our_profile_and_becomes_a_ready_friend() {
    let mut t = T::started();
    t.req(Request::CreateInvite);
    let conn = t.e.state.inviting.unwrap();
    let out = t.agent(AgentEvent::Conf {
        conn,
        conn_info: x_info("bob"),
    });
    let Some(Call::Allow { conn_info, .. }) = t.e.agent.calls.last() else {
        panic!()
    };
    assert!(String::from_utf8_lossy(conn_info).contains("\"displayName\":\"alice\""));
    let Some(Event::Chats { friends, .. }) = out.last() else {
        panic!("{out:?}")
    };
    assert_eq!((friends[0].name.as_str(), friends[0].ready), ("bob", false));
    let out = t.agent(AgentEvent::Con { conn });
    assert_eq!(
        messages(&out)[0].body,
        MessageBody::Notice(ChatNotice::Connected)
    );
    let Some(Event::Chats { friends, .. }) = out.last() else {
        panic!("{out:?}")
    };
    assert!(friends[0].ready);
}

#[test]
fn connecting_joins_with_our_profile_and_a_bad_link_never_reaches_the_network() {
    let mut t = T::started();
    let out = t.req(Request::Connect(format!("  {INVITATION} ")));
    assert!(matches!(out.as_slice(), [Event::Chats { .. }]), "{out:?}");
    let Some(Call::Join { conn_info, .. }) = t.e.agent.calls.last() else {
        panic!()
    };
    assert!(String::from_utf8_lossy(conn_info).contains("x.info"));
    let calls = t.e.agent.calls.len();
    for bad in ["not a link", "", "https://example.com/x"] {
        let out = t.req(Request::Connect(bad.into()));
        assert_eq!(failed(&out), (RequestKind::Connect, "ZN-CHT-005".into()));
    }
    assert_eq!(t.e.agent.calls.len(), calls);
}

#[test]
fn the_users_own_link_and_a_known_one_connect_nothing() {
    let mut t = T::started();
    t.req(Request::CreateInvite);
    let conn = t.e.state.inviting.unwrap();
    t.agent(AgentEvent::Invitation {
        conn,
        full: "simplex:/x".into(),
        short: Some(INVITATION.into()),
    });
    let out = t.req(Request::Connect(INVITATION.into()));
    assert_eq!(failed(&out), (RequestKind::Connect, "ZN-CHT-005".into()));

    let mut t = T::started();
    t.req(Request::Connect(INVITATION.into()));
    let calls = t.e.agent.calls.len();
    let out = t.req(Request::Connect(INVITATION.into()));
    assert!(matches!(out.as_slice(), [Event::Chats { .. }]), "{out:?}");
    assert_eq!(t.e.agent.calls.len(), calls);
}

#[test]
fn group_links_are_refused_as_links_for_now() {
    let mut t = T::started();
    let out = t.req(Request::Connect(ADDRESS.replace("/a#", "/g#")));
    assert_eq!(failed(&out), (RequestKind::Connect, "ZN-CHT-005".into()));
}

#[test]
fn a_join_the_relay_refuses_is_reported_as_a_bad_link_and_can_be_removed() {
    let mut t = T::started();
    t.req(Request::Connect(INVITATION.into()));
    let conn = *t.e.state.conns.keys().next().unwrap();
    let err = AgentFailure::Other("AUTH".into());
    let out = t.agent(AgentEvent::Err { conn, err });
    let (kind, code) = failed(&out);
    assert_eq!((kind, code.as_str()), (RequestKind::Connect, "ZN-CHT-005"));
    let id = *t.e.state.friends.keys().next().unwrap();
    t.req(Request::RemoveFriend(id));
    assert!(t.e.state.friends.is_empty());
}

#[test]
fn our_invitation_pasted_back_after_it_was_answered_connects_nothing() {
    let mut t = T::started();
    t.req(Request::CreateInvite);
    let conn = t.e.state.inviting.unwrap();
    let short = Some(INVITATION.into());
    t.agent(AgentEvent::Invitation {
        conn,
        full: "simplex:/x".into(),
        short,
    });
    t.agent(AgentEvent::Conf {
        conn,
        conn_info: x_info("bob"),
    });
    let out = t.req(Request::Connect(INVITATION.into()));
    assert_eq!(failed(&out), (RequestKind::Connect, "ZN-CHT-005".into()));
}
