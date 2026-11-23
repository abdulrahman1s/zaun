//! A ratchet re-sync reaching `Agreed` (Subscriber.hs `RSYNC`): a verified friend's code changed.

use zaun_chat_wire::model::{ChatNotice, ChatRef, MessageBody};
use zaun_chat_wire::{Event, Request};

use super::Call;
use super::{T, messages};
use crate::agent::resync::SyncState;
use crate::chat::group::GroupMemberRole;
use crate::engine::agent_port::{AgentEvent, AgentFailure, ConnId};

fn verified_friend(t: &mut T, name: &str) -> (i64, ConnId) {
    t.e.agent.ratchet_ad = b"ratchet ad one".to_vec();
    let (id, conn) = t.friend(name);
    let Event::Code { code, .. } = t.req(Request::ShowCode(id)).remove(0) else {
        panic!()
    };
    t.req(Request::Verify { friend: id, code });
    (id, conn)
}

fn listed_verified(t: &mut T, id: i64) -> bool {
    let Event::Chats { friends, .. } = t.req(Request::Refresh).remove(0) else {
        panic!()
    };
    friends.iter().find(|f| f.id == id).unwrap().verified
}

fn sync(t: &mut T, conn: ConnId, state: SyncState) -> Vec<Event> {
    t.agent(AgentEvent::RatchetSync { conn, state })
}

#[test]
fn an_agreed_resync_ends_a_friends_verification_and_says_the_code_changed() {
    let mut t = T::started();
    let (bob, conn) = verified_friend(&mut t, "bob");
    assert!(listed_verified(&mut t, bob));
    let out = sync(&mut t, conn, SyncState::Agreed);
    let [m] = messages(&out).try_into().unwrap();
    assert_eq!(m.body, MessageBody::Notice(ChatNotice::CodeChanged));
    assert_eq!(m.chat, ChatRef::Direct(bob));
    assert!(!listed_verified(&mut t, bob));
}

#[test]
fn an_agreed_resync_of_an_unverified_friend_shows_nothing() {
    let mut t = T::started();
    let (_, conn) = t.friend("bob");
    assert!(messages(&sync(&mut t, conn, SyncState::Agreed)).is_empty());
}

#[test]
fn other_sync_states_keep_the_verification() {
    let mut t = T::started();
    let (bob, conn) = verified_friend(&mut t, "bob");
    for state in [
        SyncState::Allowed,
        SyncState::Required,
        SyncState::Started,
        SyncState::Ok,
    ] {
        assert!(messages(&sync(&mut t, conn, state)).is_empty(), "{state:?}");
    }
    assert!(listed_verified(&mut t, bob));
}

#[test]
fn an_agreed_resync_with_a_group_member_shows_nothing() {
    let mut t = T::started();
    let (_, members) = t.group("team", &["bob"], GroupMemberRole::Owner);
    let out = sync(&mut t, members[0].1, SyncState::Agreed);
    assert!(messages(&out).is_empty());
    assert!(!out.iter().any(|e| matches!(e, Event::Failed { .. })));
}

fn resyncs(t: &T) -> Vec<ConnId> {
    let c = t.e.agent.calls.iter();
    c.filter_map(|c| match c {
        Call::SynchronizeRatchet { conn } => Some(*conn),
        _ => None,
    })
    .collect()
}

#[test]
fn a_required_resync_is_started_at_once_since_nothing_can_be_sent_until_it_is() {
    let mut t = T::started();
    let (_, conn) = t.friend("bob");
    sync(&mut t, conn, SyncState::Required);
    assert_eq!(resyncs(&t), [conn]);
}

#[test]
fn only_a_required_state_starts_a_resync() {
    let mut t = T::started();
    let (_, conn) = t.friend("bob");
    for state in [
        SyncState::Ok,
        SyncState::Allowed,
        SyncState::Started,
        SyncState::Agreed,
    ] {
        sync(&mut t, conn, state);
    }
    assert!(resyncs(&t).is_empty());
}

#[test]
fn a_resync_the_agent_refuses_is_only_logged() {
    let mut t = T::started();
    let (_, conn) = t.friend("bob");
    t.e.agent.fail_next = Some(AgentFailure::Other("not duplex".into()));
    let out = sync(&mut t, conn, SyncState::Required);
    assert!(!out.iter().any(|e| matches!(e, Event::Failed { .. })));
}
