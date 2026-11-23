//! A friend's chat preferences and what they decide (Types/Preferences.hs `prefEnabled`, `featureAllowed`).

use zaun_chat_wire::model::{ChatRef, MessageBody};
use zaun_chat_wire::{Event, Request, RequestKind};

use super::profile::{pref, their_info};
use super::{T, messages};
use crate::chat::container::MsgContainer;
use crate::chat::ids::SharedMsgId;
use crate::chat::preferences::FeatureAllowed;
use crate::chat::reaction::MsgReaction;
use crate::chat::{ChatEvent, parse_chat_messages};
use crate::engine::records::friend::{Feature, PrefEnabled};

#[test]
fn a_friends_preferences_are_kept_from_their_profile_and_a_missing_one_is_the_default() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    let on = |t: &T, f| t.e.state.friends[&bob].enabled(f);
    let yes = PrefEnabled {
        for_user: true,
        for_contact: true,
    };
    let off = PrefEnabled {
        for_user: false,
        for_contact: false,
    };
    assert_eq!(on(&t, Feature::Reactions), yes);
    assert_eq!(on(&t, Feature::FullDelete), off);
    t.peer_says(
        &conn,
        1,
        their_info(None, |p| {
            p.reactions = pref(FeatureAllowed::No);
            p.calls = pref(FeatureAllowed::No);
            p.full_delete = pref(FeatureAllowed::Always);
            p.voice = None;
        }),
    );
    assert_eq!(on(&t, Feature::Reactions), off);
    assert_eq!(on(&t, Feature::Calls), off);
    assert_eq!(
        on(&t, Feature::Voice),
        yes,
        "a missing preference reads as the default"
    );
    assert_eq!(
        on(&t, Feature::FullDelete),
        PrefEnabled {
            for_user: true,
            for_contact: false
        },
        "(no, always) is the user's only"
    );
}

#[test]
fn preferences_decide_for_each_side_as_prefenabled_does() {
    use FeatureAllowed::{Always, No, Yes};
    let mut t = T::started();
    let (bob, _) = t.friend("bob");
    let mut f = t.e.state.friends[&bob].clone();
    let mut with = |user_default_of: Feature, theirs| {
        let mut p = crate::chat::preferences::Preferences::default_sent();
        p.files = pref(theirs);
        p.reactions = pref(theirs);
        f.prefs = Some(p);
        f.enabled(user_default_of)
    };
    // The user's files are `always`, reactions `yes`.
    assert_eq!(
        with(Feature::Files, No),
        PrefEnabled {
            for_user: false,
            for_contact: true
        }
    );
    assert_eq!(
        with(Feature::Files, Yes),
        PrefEnabled {
            for_user: true,
            for_contact: true
        }
    );
    assert_eq!(
        with(Feature::Reactions, Always),
        PrefEnabled {
            for_user: true,
            for_contact: true
        }
    );
    assert_eq!(
        with(Feature::Reactions, No),
        PrefEnabled {
            for_user: false,
            for_contact: false
        }
    );
}

fn shared_of(t: &mut T, friend: i64, item: i64) -> SharedMsgId {
    let it = t.e.item(friend, item).unwrap().unwrap();
    SharedMsgId(it.shared_id.unwrap())
}

fn sent_text(t: &mut T, friend: i64, text: &str) -> zaun_chat_wire::model::ChatMessage {
    messages(&t.req(Request::Send {
        chat: ChatRef::Direct(friend),
        text: text.into(),
        reply_to: None,
        mentions: Vec::new(),
    }))[0]
        .clone()
}

#[test]
fn reactions_are_refused_when_the_friend_turned_them_off() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    let m =
        messages(&t.peer_says(&conn, 1, ChatEvent::MsgNew(MsgContainer::text("gg"))))[0].clone();
    t.peer_says(
        &conn,
        2,
        their_info(None, |p| p.reactions = pref(FeatureAllowed::No)),
    );
    let sends = t.e.agent.sent().len();
    let out = t.req(Request::React {
        chat: ChatRef::Direct(bob),
        item: m.id,
        emoji: "\u{1f44d}".into(),
        on: true,
    });
    assert!(
        matches!(
            out.as_slice(),
            [Event::Failed {
                request: RequestKind::React,
                ..
            }]
        ),
        "{out:?}"
    );
    assert_eq!(t.e.agent.sent().len(), sends);
}

#[test]
fn a_reaction_from_a_friend_who_turned_them_off_is_ignored() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    let m = sent_text(&mut t, bob, "mine");
    let id = shared_of(&mut t, bob, m.id);
    let react = |emoji| ChatEvent::MsgReact {
        msg_id: id.clone(),
        member_id: None,
        scope: None,
        reaction: MsgReaction::Emoji(emoji),
        add: true,
    };
    let out = t.peer_says(&conn, 1, react('\u{1f44d}'));
    assert_eq!(messages(&out)[0].reactions.len(), 1, "allowed by default");
    t.peer_says(
        &conn,
        2,
        their_info(None, |p| p.reactions = pref(FeatureAllowed::No)),
    );
    let out = t.peer_says(&conn, 3, react('\u{1f602}'));
    assert!(messages(&out).is_empty(), "{out:?}");
}

#[test]
fn deleting_removes_the_item_for_good_when_the_preferences_allow_full_delete() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    let kept = sent_text(&mut t, bob, "kept");
    let other = sent_text(&mut t, bob, "other");
    t.peer_says(
        &conn,
        1,
        their_info(None, |p| p.full_delete = pref(FeatureAllowed::Always)),
    );
    let out = t.req(Request::Delete {
        chat: ChatRef::Direct(bob),
        item: kept.id,
    });
    let [d] = messages(&out).try_into().unwrap();
    assert_eq!((d.id, d.body), (kept.id, MessageBody::Deleted));
    assert!(
        t.e.item(bob, kept.id).unwrap().is_none(),
        "gone from the store"
    );
    let ids: Vec<_> =
        t.e.items_page(bob, None, 50)
            .unwrap()
            .iter()
            .map(|m| m.id)
            .collect();
    assert!(!ids.contains(&kept.id) && ids.contains(&other.id));
    assert!(matches!(
        parse_chat_messages(&t.e.agent.sent().pop().unwrap().1).unwrap()[0].event,
        ChatEvent::MsgDel { .. }
    ));
}

#[test]
fn deleting_only_marks_the_item_while_the_friend_has_not_allowed_full_delete() {
    let mut t = T::started();
    let (bob, _) = t.friend("bob");
    let m = sent_text(&mut t, bob, "oops");
    t.req(Request::Delete {
        chat: ChatRef::Direct(bob),
        item: m.id,
    });
    assert_eq!(
        t.e.item(bob, m.id).unwrap().unwrap().body,
        MessageBody::Deleted
    );
}

#[test]
fn a_friends_delete_only_marks_the_item_while_the_users_full_delete_is_off() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    let m =
        messages(&t.peer_says(&conn, 1, ChatEvent::MsgNew(MsgContainer::text("oops"))))[0].clone();
    t.peer_says(
        &conn,
        2,
        their_info(None, |p| p.full_delete = pref(FeatureAllowed::Always)),
    );
    let id = shared_of(&mut t, bob, m.id);
    let out = t.peer_says(
        &conn,
        3,
        ChatEvent::MsgDel {
            msg_id: id,
            member_id: None,
            scope: None,
            only_history: false,
        },
    );
    assert_eq!(messages(&out)[0].body, MessageBody::Deleted);
    assert!(t.e.item(bob, m.id).unwrap().is_some(), "kept, marked");
}
