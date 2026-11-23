//! SetGroupPicture (codec: a_group_picture_keeps_the_rest_of_the_groups_profile, the_owners_group_picture_answer_reads_the_list_again, a_group_not_in_the_list_fails_without_a_command).

use zaun_chat_wire::model::Picture;
use zaun_chat_wire::{Event, Request, RequestKind};

use super::seed::profile;
use crate::chat::group::GroupMemberRole;
use crate::chat::{ChatEvent, parse_chat_messages};
use crate::engine::profile::picture_of;
use crate::engine::tests::T;

fn a_picture() -> Picture {
    let mut bytes = b"\xff\xd8\xff\xe0".to_vec();
    bytes.extend([7u8; 64]);
    Picture {
        bytes,
        mime: "image/jpeg".into(),
    }
}

fn set(t: &mut T, group: i64, picture: Option<Picture>) -> Vec<Event> {
    t.req(Request::SetGroupPicture { group, picture })
}

#[test]
fn a_group_picture_keeps_the_rest_of_the_groups_profile() {
    let mut t = T::started();
    let (g, members) = t.group("squad", &["bob"], GroupMemberRole::Owner);
    let mut group = t.e.group(g).unwrap();
    group.profile = profile("squad");
    group.profile.description = Some("night owls".into());
    t.e.save_group(group).unwrap();

    set(&mut t, g, Some(a_picture()));
    let after = t.e.group(g).unwrap().profile;
    assert_eq!(after.display_name, "squad");
    assert_eq!(after.description.as_deref(), Some("night owls"));
    assert_eq!(
        picture_of(&after.image.clone().unwrap().0),
        Some(a_picture())
    );

    let (conn, body) = t.e.agent.sent().pop().unwrap();
    assert_eq!(conn, members[0].1);
    let [x] = parse_chat_messages(&body).unwrap().try_into().unwrap();
    let ChatEvent::GrpInfo(sent) = x.event else {
        panic!("not an x.grp.info")
    };
    assert_eq!(sent, after);

    // Removing it drops only the image.
    set(&mut t, g, None);
    let removed = t.e.group(g).unwrap().profile;
    assert!(removed.image.is_none() && removed.description.as_deref() == Some("night owls"));
    assert_eq!(t.e.agent.sent().len(), 2);
}

#[test]
fn the_owners_group_picture_answer_reads_the_list_again() {
    let mut t = T::started();
    let (g, _) = t.group("squad", &["bob"], GroupMemberRole::Owner);
    let out = set(&mut t, g, Some(a_picture()));
    let Some(Event::Chats { groups, .. }) = out.iter().find(|e| matches!(e, Event::Chats { .. }))
    else {
        panic!("{out:?}")
    };
    assert_eq!(groups[0].picture, Some(a_picture()));
}

#[test]
fn a_member_who_is_not_the_owner_cannot_change_the_picture() {
    let mut t = T::started();
    let (g, _) = t.group("squad", &["bob"], GroupMemberRole::Admin);
    let out = set(&mut t, g, Some(a_picture()));
    assert!(matches!(
        out.as_slice(),
        [Event::Failed {
            request: RequestKind::Group,
            ..
        }]
    ));
    assert!(t.e.group(g).unwrap().profile.image.is_none());
    assert!(t.e.agent.sent().is_empty());
}

#[test]
fn a_group_not_in_the_list_fails_without_a_command() {
    let mut t = T::started();
    let calls = t.e.agent.calls.len();
    let out = set(&mut t, 99, None);
    assert!(matches!(
        out.as_slice(),
        [Event::Failed {
            request: RequestKind::Group,
            ..
        }]
    ));
    assert_eq!(t.e.agent.calls.len(), calls);
}

#[test]
fn a_picture_that_is_not_a_jpeg_or_png_is_refused() {
    let mut t = T::started();
    let (g, _) = t.group("squad", &["bob"], GroupMemberRole::Owner);
    let bad = Picture {
        bytes: b"GIF89a".to_vec(),
        mime: "image/gif".into(),
    };
    let out = set(&mut t, g, Some(bad));
    assert!(matches!(out.as_slice(), [Event::Failed { .. }]));
}
