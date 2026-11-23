//! Edits, deletions and reactions in a group, the user's and the members' (Subscriber.hs `groupMessageUpdate`, `groupMessageDelete`, `groupMsgReaction`).

use std::time::Duration;

use zaun_chat_wire::model::{ChatRef, MessageAuthor, MessageBody};
use zaun_chat_wire::{Request, RequestKind};

use super::super::{T, messages};
use super::messages::{failed, member_id, one, says, send, sent_to};
use crate::chat::ChatEvent;
use crate::chat::container::{MsgContainer, MsgMentions};
use crate::chat::content::MsgContent;
use crate::chat::group::GroupMemberRole;
use crate::chat::group_prefs::{FeatureEnabled, GroupFeature, GroupPreferences};
use crate::chat::ids::SharedMsgId;
use crate::chat::reaction::MsgReaction;

fn shared_of(t: &mut T, g: i64, item: i64) -> SharedMsgId {
    SharedMsgId(
        t.e.item(ChatRef::Group(g), item)
            .unwrap()
            .unwrap()
            .shared_id
            .unwrap(),
    )
}

fn update(msg_id: SharedMsgId, text: &str) -> ChatEvent {
    ChatEvent::MsgUpdate {
        msg_id,
        content: MsgContent::Text { text: text.into() },
        mentions: MsgMentions::new(),
        ttl: None,
        live: None,
        scope: None,
        as_group: None,
    }
}

fn del(msg_id: SharedMsgId, member_id: Option<crate::chat::ids::MemberId>) -> ChatEvent {
    ChatEvent::MsgDel {
        msg_id,
        member_id,
        scope: None,
        only_history: false,
    }
}

fn react(msg_id: SharedMsgId, author: crate::chat::ids::MemberId, add: bool) -> ChatEvent {
    ChatEvent::MsgReact {
        msg_id,
        member_id: Some(author),
        scope: None,
        reaction: MsgReaction::Emoji('👍'),
        add,
    }
}

fn set_role(t: &mut T, g: i64, member: i64, role: GroupMemberRole) {
    let mut m = t.e.state.members[&(g, member)].clone();
    m.role = role;
    t.e.save_member(m).unwrap();
}

fn set_prefs(t: &mut T, g: i64, f: impl FnOnce(&mut GroupPreferences)) {
    let mut gr = t.e.state.groups[&g].clone();
    let mut p = GroupPreferences::default_full();
    f(&mut p);
    gr.profile.group_preferences = Some(p);
    t.e.save_group(gr).unwrap();
}

#[test]
fn an_edit_in_a_group_goes_to_every_member_and_is_not_sent_as_the_group() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Member);
    let m = one(&t.req(send(g, "helo", Vec::new())));
    let u = one(&t.req(Request::Edit {
        chat: ChatRef::Group(g),
        item: m.id,
        text: "hello".into(),
    }));
    assert_eq!(
        (u.body, u.edited),
        (MessageBody::Text("hello".into()), true)
    );
    for (_, conn) in &ms {
        let last = sent_to(&t, conn).pop().unwrap();
        let ChatEvent::MsgUpdate { as_group, .. } = last else {
            panic!("{last:?}")
        };
        assert_eq!(as_group, Some(false));
    }
}

#[test]
fn deleting_in_a_group_is_broadcast_and_full_only_when_the_group_allows_it() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob"], GroupMemberRole::Member);
    let m = one(&t.req(send(g, "oops", Vec::new())));
    let d = one(&t.req(Request::Delete {
        chat: ChatRef::Group(g),
        item: m.id,
    }));
    assert_eq!(d.body, MessageBody::Deleted);
    assert!(matches!(
        sent_to(&t, &ms[0].1).pop(),
        Some(ChatEvent::MsgDel {
            member_id: None,
            ..
        })
    ));
    assert!(
        t.e.item(ChatRef::Group(g), m.id).unwrap().is_some(),
        "marked, kept"
    );

    set_prefs(&mut t, g, |p| {
        p.full_delete = Some(GroupFeature::new(FeatureEnabled::On));
    });
    let m = one(&t.req(send(g, "gone", Vec::new())));
    t.req(Request::Delete {
        chat: ChatRef::Group(g),
        item: m.id,
    });
    assert!(
        t.e.item(ChatRef::Group(g), m.id).unwrap().is_none(),
        "gone for good"
    );
}

#[test]
fn an_observer_cannot_send_or_edit_in_a_group() {
    let mut t = T::started();
    let (g, _) = t.group("team", &["bob"], GroupMemberRole::Observer);
    assert!(failed(&t.req(send(g, "hi", Vec::new())), RequestKind::Send));
    assert!(t.e.agent.sent().is_empty());
}

#[test]
fn a_members_edit_changes_only_their_own_message() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Member);
    let m = one(&says(&mut t, &ms[0].1, 1, MsgContainer::text("helo")));
    let s = shared_of(&mut t, g, m.id);
    let forged = t.peer_says(&ms[1].1, 2, update(s.clone(), "carol wrote this"));
    let made = messages(&forged);
    assert!(
        made.iter().all(|x| x.id != m.id),
        "carol's edit with bob's id is not an edit of bob's message"
    );
    let u = one(&t.peer_says(&ms[0].1, 3, update(s, "hello")));
    assert_eq!((u.id, u.edited), (m.id, true));
    assert_eq!(u.body, MessageBody::Text("hello".into()));
}

#[test]
fn a_member_deletes_their_own_message_within_78_hours() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob"], GroupMemberRole::Member);
    let old = one(&says(&mut t, &ms[0].1, 1, MsgContainer::text("old")));
    let recent_text = "recent";
    t.clock.advance(Duration::from_secs(79 * 3600));
    let recent = one(&says(&mut t, &ms[0].1, 2, MsgContainer::text(recent_text)));
    let (so, sr) = (
        shared_of(&mut t, g, old.id),
        shared_of(&mut t, g, recent.id),
    );
    assert!(messages(&t.peer_says(&ms[0].1, 3, del(so, None))).is_empty());
    let d = one(&t.peer_says(&ms[0].1, 4, del(sr, None)));
    assert_eq!((d.id, d.body), (recent.id, MessageBody::Deleted));
}

#[test]
fn a_moderator_deletes_a_members_message_but_not_one_of_a_higher_role() {
    let mut t = T::started();
    let names = ["bob", "carol", "dave", "erin"];
    let (g, ms) = t.group("team", &names, GroupMemberRole::Member);
    let (bob, carol, dave, erin) = (ms[0], ms[1], ms[2], ms[3]);
    set_role(&mut t, g, carol.0, GroupMemberRole::Moderator);
    let erins = one(&says(&mut t, &erin.1, 1, MsgContainer::text("spam")));
    let se = shared_of(&mut t, g, erins.id);
    let erin_id = member_id(&t, g, erin.0);
    let plain = t.peer_says(&dave.1, 2, del(se.clone(), Some(erin_id.clone())));
    assert!(
        messages(&plain).is_empty(),
        "a plain member cannot moderate"
    );
    let d = one(&t.peer_says(&carol.1, 3, del(se, Some(erin_id))));
    assert_eq!((d.id, d.body), (erins.id, MessageBody::Deleted));

    // Bob is the host here, an owner: above a moderator. The user is a plain member.
    let owners = one(&says(&mut t, &bob.1, 4, MsgContainer::text("owner says")));
    let so = shared_of(&mut t, g, owners.id);
    let bob_id = member_id(&t, g, bob.0);
    assert!(messages(&t.peer_says(&carol.1, 5, del(so, Some(bob_id)))).is_empty());
    let mine = one(&t.req(send(g, "hi", Vec::new())));
    let my_id = t.e.state.groups[&g].my_member_id.clone();
    let sm = shared_of(&mut t, g, mine.id);
    let d = one(&t.peer_says(&carol.1, 6, del(sm, Some(my_id))));
    assert_eq!(
        (d.id, d.author, d.body),
        (mine.id, MessageAuthor::Me, MessageBody::Deleted)
    );
}

#[test]
fn reactions_in_a_group_name_the_author_and_count_each_member_once() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Member);
    let m = one(&says(&mut t, &ms[0].1, 1, MsgContainer::text("gg")));
    let r = one(&t.req(Request::React {
        chat: ChatRef::Group(g),
        item: m.id,
        emoji: "👍".into(),
        on: true,
    }));
    assert!(r.reactions[0].mine);
    let Some(ChatEvent::MsgReact { member_id: a, .. }) = sent_to(&t, &ms[1].1).pop() else {
        panic!()
    };
    let bob_id = member_id(&t, g, ms[0].0);
    assert_eq!(a, Some(bob_id.clone()));

    let s = shared_of(&mut t, g, m.id);
    let r = one(&t.peer_says(&ms[1].1, 2, react(s.clone(), bob_id.clone(), true)));
    assert_eq!((r.reactions[0].count, r.reactions[0].mine), (2, true));
    let again = t.peer_says(&ms[1].1, 3, react(s.clone(), bob_id.clone(), true));
    assert!(
        messages(&again).is_empty(),
        "the same member's reaction counts once"
    );
    let r = one(&t.peer_says(&ms[1].1, 4, react(s, bob_id, false)));
    assert_eq!(r.reactions[0].count, 1);
}

#[test]
fn reactions_and_files_the_group_turned_off_are_dropped() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob"], GroupMemberRole::Member);
    let m = one(&says(&mut t, &ms[0].1, 1, MsgContainer::text("gg")));
    set_prefs(&mut t, g, |p| {
        p.reactions = Some(GroupFeature::new(FeatureEnabled::Off));
        p.files = Some(GroupFeature::new(FeatureEnabled::Off));
    });
    let s = shared_of(&mut t, g, m.id);
    let bob_id = member_id(&t, g, ms[0].0);
    assert!(messages(&t.peer_says(&ms[0].1, 2, react(s, bob_id, true))).is_empty());
    let mut c = MsgContainer::simple(MsgContent::File {
        text: String::new(),
    });
    c.file = Some(crate::chat::file::FileInvitation::xftp("a.pdf", 10));
    assert!(messages(&says(&mut t, &ms[0].1, 3, c)).is_empty());
    let out = t.req(Request::React {
        chat: ChatRef::Group(g),
        item: m.id,
        emoji: "👍".into(),
        on: true,
    });
    assert!(failed(&out, RequestKind::React));
}

#[test]
fn a_member_gives_an_item_at_most_three_reactions() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Member);
    let m = one(&says(&mut t, &ms[0].1, 1, MsgContainer::text("gg")));
    let s = shared_of(&mut t, g, m.id);
    let bob_id = member_id(&t, g, ms[0].0);
    let mut n = 1;
    for e in ['👍', '😀', '🚀', '✅'] {
        n += 1;
        let ev = ChatEvent::MsgReact {
            msg_id: s.clone(),
            member_id: Some(bob_id.clone()),
            scope: None,
            reaction: MsgReaction::Emoji(e),
            add: true,
        };
        t.peer_says(&ms[1].1, n, ev);
    }
    let item = t.e.item(ChatRef::Group(g), m.id).unwrap().unwrap();
    assert_eq!(item.reactions.len(), 3);
}
