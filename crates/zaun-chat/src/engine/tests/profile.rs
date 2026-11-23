//! The user's picture and profile out, the friend's profile and preferences in (codec: pictures, preferences).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use zaun_chat_wire::model::Picture;
use zaun_chat_wire::{Event, Request, RequestKind};

use super::T;
use crate::chat::image::ImageData;
use crate::chat::preferences::{AllowPref, FeatureAllowed};
use crate::chat::profile::Profile;
use crate::chat::{ChatEvent, parse_chat_messages};

fn jpeg(len: usize) -> Picture {
    let mut bytes = vec![0xff, 0xd8, 0xff, 0xe0];
    bytes.extend((0..len).map(|i| (i % 251) as u8));
    Picture {
        bytes,
        mime: "image/jpeg".into(),
    }
}

fn uri(p: &Picture) -> String {
    format!("data:image/jpg;base64,{}", STANDARD.encode(&p.bytes))
}

fn profile_sent(t: &T) -> Profile {
    let (_, body) = t.e.agent.sent().pop().unwrap();
    let [x] = parse_chat_messages(&body).unwrap().try_into().unwrap();
    let ChatEvent::Info { profile, .. } = x.event else {
        panic!("not an x.info")
    };
    profile
}

fn about(t: &mut T, name: &str, about: &str) -> Vec<Event> {
    t.req(Request::SetProfile(zaun_chat_wire::model::ChatProfile {
        name: name.into(),
        about: about.into(),
        ..Default::default()
    }))
}

pub(super) fn their_info(
    image: Option<&str>,
    prefs: impl FnOnce(&mut crate::chat::preferences::Preferences),
) -> ChatEvent {
    let mut profile = Profile::new("bob", "");
    profile.image = image.map(|i| ImageData(i.into()));
    if let Some(p) = profile.preferences.as_mut() {
        prefs(p);
    }
    ChatEvent::Info {
        profile,
        member_key: None,
    }
}

pub(super) fn pref(a: FeatureAllowed) -> Option<AllowPref> {
    Some(AllowPref { allow: a })
}

#[test]
fn setting_the_picture_sends_it_in_the_profile_and_keeps_name_and_about() {
    let mut t = T::started();
    let (_, conn) = t.friend("bob");
    about(&mut t, "alice", "Mostly co-op.");
    let new = jpeg(300);
    let out = t.req(Request::SetPicture(Some(new.clone())));
    let [Event::Profile(p)] = out.as_slice() else {
        panic!("{out:?}")
    };
    assert_eq!(
        (p.name.as_str(), p.about.as_str()),
        ("alice", "Mostly co-op.")
    );
    assert_eq!(p.picture.as_ref(), Some(&new));
    let sent = profile_sent(&t);
    assert_eq!(t.e.agent.sent().pop().unwrap().0, conn);
    assert_eq!(sent.display_name, "alice");
    assert_eq!(sent.short_descr.as_deref(), Some("Mostly co-op."));
    assert_eq!(sent.image, Some(ImageData(uri(&new))));
}

#[test]
fn removing_the_picture_drops_only_the_image() {
    let mut t = T::started();
    t.friend("bob");
    about(&mut t, "alice", "Mostly co-op.");
    t.req(Request::SetPicture(Some(jpeg(300))));
    let out = t.req(Request::SetPicture(None));
    let [Event::Profile(p)] = out.as_slice() else {
        panic!("{out:?}")
    };
    assert_eq!(p.picture, None);
    let sent = profile_sent(&t);
    assert_eq!(sent.image, None);
    assert_eq!(sent.short_descr.as_deref(), Some("Mostly co-op."));
}

#[test]
fn a_name_change_keeps_the_picture() {
    let mut t = T::started();
    t.friend("bob");
    let new = jpeg(300);
    t.req(Request::SetPicture(Some(new.clone())));
    about(&mut t, "alice two", "New line");
    let sent = profile_sent(&t);
    assert_eq!(sent.display_name, "alice two");
    assert_eq!(sent.image, Some(ImageData(uri(&new))));
}

#[test]
fn the_engine_refuses_a_picture_over_its_limit() {
    let mut t = T::started();
    t.friend("bob");
    let sends = t.e.agent.sent().len();
    let out = t.req(Request::SetPicture(Some(jpeg(10_000))));
    assert!(
        matches!(
            out.as_slice(),
            [Event::Failed {
                request: RequestKind::SetProfile,
                ..
            }]
        ),
        "{out:?}"
    );
    assert_eq!(t.e.agent.sent().len(), sends);
    assert_eq!(t.e.state.user.picture, None);
}

#[test]
fn a_picture_that_is_not_a_jpeg_or_png_is_refused() {
    let mut t = T::started();
    let out = t.req(Request::SetPicture(Some(Picture {
        bytes: b"hello".to_vec(),
        mime: "image/jpeg".into(),
    })));
    assert!(matches!(out.as_slice(), [Event::Failed { .. }]), "{out:?}");
}

#[test]
fn an_unchanged_profile_is_not_sent_again() {
    let mut t = T::started();
    t.friend("bob");
    about(&mut t, "alice", "Mostly co-op.");
    let sends = t.e.agent.sent().len();
    let out = about(&mut t, "alice", "Mostly co-op.");
    assert!(matches!(out.as_slice(), [Event::Profile(_)]));
    assert_eq!(t.e.agent.sent().len(), sends);
}

#[test]
fn the_profile_goes_to_every_reachable_friend_and_a_failing_relay_stops_none() {
    let mut t = T::started();
    let (_, a) = t.friend("bob");
    let (_, b) = t.friend("carol");
    t.e.agent.fail_next = Some(crate::engine::agent_port::AgentFailure::Network(
        "down".into(),
    ));
    let before = t.e.agent.sent().len();
    let out = t.req(Request::SetPicture(Some(jpeg(100))));
    assert!(matches!(out.as_slice(), [Event::Profile(_)]));
    let to: Vec<_> = t.e.agent.sent()[before..].iter().map(|(c, _)| *c).collect();
    assert_eq!(to, vec![a, b]);
}

#[test]
fn a_friends_picture_comes_with_the_list_and_a_bad_one_drops_only_the_picture() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    let pic = jpeg(200);
    let out = t.peer_says(&conn, 1, their_info(Some(&uri(&pic)), |_| {}));
    let Some(Event::Chats { friends, .. }) = out.last() else {
        panic!("{out:?}")
    };
    assert_eq!(
        friends.iter().find(|f| f.id == bob).unwrap().picture,
        Some(pic)
    );
    let out = t.peer_says(
        &conn,
        2,
        their_info(Some("data:image/jpg;base64,aGVsbG8="), |_| {}),
    );
    let Some(Event::Chats { friends, .. }) = out.last() else {
        panic!("{out:?}")
    };
    let f = friends.iter().find(|f| f.id == bob).unwrap();
    assert_eq!((f.picture.as_ref(), f.name.as_str()), (None, "bob"));
}

#[test]
fn a_picture_too_long_to_be_a_profiles_is_dropped() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    let long = uri(&jpeg(40_000));
    let out = t.peer_says(&conn, 1, their_info(Some(&long), |_| {}));
    let Some(Event::Chats { friends, .. }) = out.last() else {
        panic!("{out:?}")
    };
    assert_eq!(friends.iter().find(|f| f.id == bob).unwrap().picture, None);
}
