//! Groups (Phase 4): the seed every group test starts from, and the rules of the foundation.

mod admin;
mod admin_member;
mod files;
mod forward;
mod forward_signed;
mod leave;
mod links;
mod links_host;
mod links_join;
mod members;
mod members_intro;
mod members_join;
mod members_list;
mod members_peers;
mod members_pipe;
mod members_support;
mod messages;
mod messages_change;
mod picture;
mod seed;

use zaun_chat_wire::model::{ChatRef, Membership, MessageAuthor, MessageBody};
use zaun_chat_wire::{Event, Request};

use super::T;
use crate::chat::group::GroupMemberRole;
use crate::engine::records::ItemRecord;

#[test]
fn a_seeded_group_is_listed_with_its_members_counted_and_the_users_role() {
    let mut t = T::started();
    let (g, members) = t.group("team", &["bob", "carol"], GroupMemberRole::Owner);
    assert_eq!(members.len(), 2);
    let out = t.req(Request::Refresh);
    let list = out.iter().find_map(|e| match e {
        Event::Chats { groups, chats, .. } => Some((groups.clone(), chats.clone())),
        _ => None,
    });
    let (groups, chats) = list.unwrap();
    let [team] = groups.try_into().unwrap();
    assert_eq!(
        (team.id, team.name.as_str(), team.members, team.owner),
        (g, "team", Some(3), true)
    );
    assert_eq!(team.membership, Membership::Member);
    assert!(chats.iter().any(|c| c.chat == ChatRef::Group(g)));
}

#[test]
fn a_group_and_a_friend_with_the_same_number_keep_their_items_apart() {
    let mut t = T::started();
    let (bob, _) = t.friend("bob");
    let (g, _) = t.group("team", &["carol"], GroupMemberRole::Member);
    assert_eq!(bob, g, "the two counters start alike, which is the point");
    let said = |t: &mut T, chat, text: &str| {
        let it = ItemRecord::new(MessageAuthor::Me, MessageBody::Text(text.into()), 1);
        t.e.add_item(chat, &it).unwrap()
    };
    said(&mut t, ChatRef::Direct(bob), "to bob");
    said(&mut t, ChatRef::Group(g), "to the team");
    let direct = t.e.items_page(ChatRef::Direct(bob), None, 10).unwrap();
    let group = t.e.items_page(ChatRef::Group(g), None, 10).unwrap();
    let texts = |ms: &[zaun_chat_wire::model::ChatMessage]| -> Vec<MessageBody> {
        ms.iter().map(|m| m.body.clone()).collect()
    };
    assert!(texts(&direct).contains(&MessageBody::Text("to bob".into())));
    assert!(!texts(&direct).contains(&MessageBody::Text("to the team".into())));
    assert_eq!(texts(&group), [MessageBody::Text("to the team".into())]);
}

#[test]
fn a_group_and_its_members_come_back_after_a_restart() {
    let mut t = T::started();
    let (g, members) = t.group("team", &["bob"], GroupMemberRole::Admin);
    let (_, conn) = members[0];
    let mut t = t.restarted();
    assert_eq!(t.e.state.groups[&g].profile.display_name, "team");
    assert_eq!(t.e.state.member_of(&conn), Some((g, members[0].0)));
    assert!(
        t.req(Request::Refresh)
            .iter()
            .any(|e| matches!(e, Event::Chats { groups, .. } if groups.len() == 1))
    );
}
