//! Profiles both ways (codec: the_profile_carries_the_line_about_the_user_…, a_direct_chat_…_names_…).

use zaun_chat_wire::model::{ChatNotice, MessageBody};
use zaun_chat_wire::{Event, Request};

use super::{T, messages};
use crate::chat::profile::Profile;
use crate::chat::{ChatEvent, parse_chat_messages};

#[test]
fn a_new_name_is_told_and_other_profile_changes_are_not() {
    let mut t = T::started();
    let (_, conn) = t.friend("bob");
    let info = |name: &str, about: &str| {
        let mut profile = Profile::new(name, "");
        profile.short_descr = Some(about.into());
        ChatEvent::Info {
            profile,
            member_key: None,
        }
    };
    let out = t.peer_says(&conn, 1, info("bob", "co-op"));
    assert!(messages(&out).is_empty());
    let out = t.peer_says(&conn, 2, info("robert", "co-op"));
    assert_eq!(
        messages(&out)[0].body,
        MessageBody::Notice(ChatNotice::NameChanged {
            from: "bob".into(),
            to: "robert".into()
        })
    );
    assert_eq!(t.acks()[1], (2, None), "x.info asks for no receipt");
}

#[test]
fn the_profile_carries_the_line_about_the_user_and_goes_to_every_friend() {
    let mut t = T::started();
    let (_, conn) = t.friend("bob");
    let out = t.req(Request::SetProfile(zaun_chat_wire::model::ChatProfile {
        name: "alice".into(),
        about: " Speedrunner. ".into(),
        ..Default::default()
    }));
    let [Event::Profile(p)] = out.as_slice() else {
        panic!("{out:?}")
    };
    assert_eq!(p.about, "Speedrunner.");
    let (to, body) = t.e.agent.sent().pop().unwrap();
    assert_eq!(to, conn);
    let [x] = parse_chat_messages(&body).unwrap().try_into().unwrap();
    let ChatEvent::Info { profile, .. } = x.event else {
        panic!()
    };
    assert_eq!(profile.short_descr.as_deref(), Some("Speedrunner."));
    assert!(
        x.msg_id.is_some(),
        "after connecting, x.info has a message id"
    );
}
