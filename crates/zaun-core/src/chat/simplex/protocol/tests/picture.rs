//! Pictures (ADR-0038) against JSON recorded from chat core 7.0 through a local relay: alice
//! set a picture, changed it and removed it; bob saw her and her group's.

use super::*;
use crate::chat::picture;
use crate::model::Picture;

fn pic(name: &str) -> Value {
    let text = match name {
        "active_user_picture" => include_str!("../../fixtures/active_user_picture.json"),
        "profile_picture_resp" => include_str!("../../fixtures/profile_picture_resp.json"),
        "profile_picture_too_large" => {
            include_str!("../../fixtures/profile_picture_too_large.json")
        }
        "api_chats_picture" => include_str!("../../fixtures/api_chats_picture.json"),
        "members_picture_resp" => include_str!("../../fixtures/members_picture_resp.json"),
        "group_profile_picture_resp" => {
            include_str!("../../fixtures/group_profile_picture_resp.json")
        }
        "group_profile_not_owner" => include_str!("../../fixtures/group_profile_not_owner.json"),
        "contact_updated_picture" => include_str!("../../fixtures/contact_updated_picture.json"),
        "contact_updated_no_picture" => {
            include_str!("../../fixtures/contact_updated_no_picture.json")
        }
        other => panic!("no fixture {other}"),
    };
    serde_json::from_str(text).unwrap()
}

/// A codec started as a user who has a picture; returns it and the profile it reported.
fn started_with_picture() -> (Codec, ChatProfile) {
    let mut c = codec("alice", Vec::new());
    c.on_line(r#"{"t":"ready"}"#);
    c.on_line(&resp(1, &pic("active_user_picture")));
    c.on_line(&resp(2, &ok()));
    let out = c.on_line(&resp(3, &fixture("chat_started")));
    let Some(ChatEvent::Ready(p)) = out.events.iter().find(|e| matches!(e, ChatEvent::Ready(_)))
    else {
        panic!("{:?}", out.events)
    };
    let p = p.clone();
    (c, p)
}

/// Started (as bob), with the recorded list answered.
fn settled_bob() -> (Codec, ChatEvent) {
    let mut c = started();
    let out = c.on_line(&resp(4, &pic("api_chats_picture")));
    let none = serde_json::json!({ "error": { "type": "error", "errorType": { "type": "noContactLink" } } });
    c.on_line(&resp(5, &none));
    c.on_line(&resp(6, &no_servers()));
    let list = out
        .events
        .into_iter()
        .find(|e| matches!(e, ChatEvent::Chats { .. }))
        .expect("the list");
    (c, list)
}

/// The JSON of a `/_profile 1 …` or `/_group_profile #g …` command.
fn profile_of(cmd: &str) -> Value {
    serde_json::from_str(&cmd[cmd.find('{').unwrap()..]).unwrap()
}

fn a_picture() -> Picture {
    let img = image::RgbImage::from_fn(40, 40, |x, y| image::Rgb([x as u8 * 6, y as u8 * 6, 90]));
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 80)
        .encode_image(&img)
        .unwrap();
    picture::make(&jpeg, None).unwrap()
}

#[test]
fn the_users_picture_is_read_at_start() {
    let (_, p) = started_with_picture();
    let picture = p.picture.expect("a picture");
    assert!(picture.bytes().starts_with(b"\xff\xd8\xff"));
    assert_eq!(p.about, "Mostly co-op.");
}

#[test]
fn setting_the_picture_sends_it_in_the_profile_and_keeps_name_and_about() {
    let (mut c, _) = started_with_picture();
    let new = a_picture();
    let (corr, cmd) = cmds(&c.request(Request::SetPicture(Some(new.clone()))))[0].clone();
    let json = profile_of(&cmd);
    assert_eq!(json["displayName"], "alice");
    assert_eq!(json["shortDescr"], "Mostly co-op.");
    assert_eq!(json["image"], picture::data_uri(&new).as_str());
    assert!(json["image"].as_str().unwrap().len() <= picture::MAX_URI);
    // The engine's answer (recorded: alice's second picture) becomes the profile.
    let out = c.on_line(&resp(corr, &pic("profile_picture_resp")));
    let [ChatEvent::Profile(p)] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    assert!(p.picture.is_some());
    assert_eq!(p.name, "alice");
}

#[test]
fn removing_the_picture_drops_only_the_image() {
    let (mut c, _) = started_with_picture();
    let cmd = cmds(&c.request(Request::SetPicture(None)))[0].1.clone();
    let json = profile_of(&cmd);
    assert!(json.get("image").is_none(), "{cmd}");
    assert_eq!(json["shortDescr"], "Mostly co-op.");
}

#[test]
fn a_name_change_keeps_the_picture_even_one_not_answered_yet() {
    let (mut c, _) = started_with_picture();
    let profile = ChatProfile {
        name: "alice".into(),
        about: "New line".into(),
        ..ChatProfile::default()
    };
    let cmd = cmds(&c.request(Request::SetProfile(profile.clone())))[0]
        .1
        .clone();
    assert!(profile_of(&cmd)["image"].as_str().is_some(), "{cmd}");
    // A picture just sent, then a name change before its answer: the new picture stays.
    let new = a_picture();
    c.request(Request::SetPicture(Some(new.clone())));
    let cmd = cmds(&c.request(Request::SetProfile(profile)))[0].1.clone();
    assert_eq!(profile_of(&cmd)["image"], picture::data_uri(&new).as_str());
}

#[test]
fn a_picture_sent_before_a_renames_answer_carries_the_new_name() {
    let (mut c, _) = started_with_picture();
    c.request(Request::SetProfile(ChatProfile {
        name: "alice two".into(),
        about: "New line".into(),
        ..ChatProfile::default()
    }));
    let new = a_picture();
    let cmd = cmds(&c.request(Request::SetPicture(Some(new.clone()))))[0]
        .1
        .clone();
    let json = profile_of(&cmd);
    assert_eq!(json["displayName"], "alice two");
    assert_eq!(json["shortDescr"], "New line");
    assert_eq!(json["image"], picture::data_uri(&new).as_str());
}

#[test]
fn the_engine_refuses_a_picture_over_its_limit() {
    let (mut c, _) = started_with_picture();
    let (corr, _) = cmds(&c.request(Request::SetPicture(Some(a_picture()))))[0].clone();
    let out = c.on_line(&resp(corr, &pic("profile_picture_too_large")));
    assert!(
        matches!(
            out.events.as_slice(),
            [ChatEvent::Failed {
                request: RequestKind::SetProfile,
                ..
            }]
        ),
        "{:?}",
        out.events
    );
}

#[test]
fn friends_and_groups_carry_their_pictures_and_the_users_role() {
    let (_, list) = settled_bob();
    let ChatEvent::Chats {
        friends, groups, ..
    } = list
    else {
        unreachable!()
    };
    let alice = friends.iter().find(|f| f.name == "alice").expect("alice");
    assert!(alice.picture.is_some());
    // The engine's built-in contact (never connected) is not a friend.
    assert_eq!(friends.len(), 1);
    let squad = groups.iter().find(|g| g.name == "squad").expect("squad");
    assert!(squad.picture.is_some());
    assert_ne!(squad.picture, alice.picture);
    // Bob joined as a member: not an owner.
    assert!(!squad.owner);
}

#[test]
fn members_carry_their_pictures() {
    let (mut c, _) = settled_bob();
    let (corr, _) = cmds(&c.request(Request::Members(1)))[0].clone();
    let out = c.on_line(&resp(corr, &pic("members_picture_resp")));
    let [ChatEvent::Members { members, .. }] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    let alice = members.iter().find(|m| m.name == "alice").expect("alice");
    assert!(alice.picture.is_some() && alice.owner);
}

#[test]
fn a_friends_new_or_removed_picture_reads_the_list_again() {
    for update in ["contact_updated_picture", "contact_updated_no_picture"] {
        let (mut c, _) = settled_bob();
        let out = c.on_line(&event(&pic(update)));
        assert!(
            cmds(&out)
                .iter()
                .any(|(_, cmd)| cmd.starts_with("/_get chats 1")),
            "{update}: {:?}",
            out.lines
        );
    }
}

#[test]
fn a_friends_new_picture_reads_the_members_of_groups_already_read() {
    let (mut c, _) = settled_bob();
    let (corr, _) = cmds(&c.request(Request::Members(1)))[0].clone();
    c.on_line(&resp(corr, &pic("members_picture_resp")));
    let out = c.on_line(&event(&pic("contact_updated_picture")));
    assert!(
        cmds(&out).iter().any(|(_, cmd)| cmd == "/_members #1"),
        "{:?}",
        out.lines
    );
}

#[test]
fn a_bad_picture_drops_the_picture_not_the_friend() {
    let mut c = started();
    let mut list = pic("api_chats_picture");
    for chat in list["result"]["chats"].as_array_mut().unwrap() {
        if let Some(p) = chat["chatInfo"]["contact"]["profile"].as_object_mut() {
            p.insert("image".into(), "data:image/jpg;base64,aGVsbG8=".into());
        }
    }
    let out = c.on_line(&resp(4, &list));
    let Some(ChatEvent::Chats { friends, .. }) = out.events.first() else {
        panic!("{:?}", out.events)
    };
    let alice = friends
        .iter()
        .find(|f| f.name == "alice")
        .expect("alice kept");
    assert_eq!(alice.picture, None);
}

#[test]
fn a_group_picture_keeps_the_rest_of_the_groups_profile() {
    let (mut c, _) = settled_bob();
    let new = a_picture();
    let (corr, cmd) = cmds(&c.request(Request::SetGroupPicture {
        group: 1,
        picture: Some(new.clone()),
    }))[0]
        .clone();
    assert!(cmd.starts_with("/_group_profile #1 {"), "{cmd}");
    let json = profile_of(&cmd);
    assert_eq!(json["displayName"], "squad");
    assert_eq!(json["image"], picture::data_uri(&new).as_str());
    // Removing it drops only the image.
    let cmd = cmds(&c.request(Request::SetGroupPicture {
        group: 1,
        picture: None,
    }))[0]
        .1
        .clone();
    let json = profile_of(&cmd);
    assert!(json.get("image").is_none() && json["displayName"] == "squad");
    // Bob is not the owner: the engine refuses (recorded), and the page says so.
    let out = c.on_line(&resp(corr, &pic("group_profile_not_owner")));
    assert!(
        matches!(
            out.events.as_slice(),
            [ChatEvent::Failed {
                request: RequestKind::Group,
                code: ErrorCode::ChtRequestFailed,
                ..
            }]
        ),
        "{:?}",
        out.events
    );
}

#[test]
fn the_owners_group_picture_answer_reads_the_list_again() {
    let (mut c, _) = settled_bob();
    let (corr, _) = cmds(&c.request(Request::SetGroupPicture {
        group: 1,
        picture: Some(a_picture()),
    }))[0]
        .clone();
    let out = c.on_line(&resp(corr, &pic("group_profile_picture_resp")));
    assert!(
        cmds(&out)
            .iter()
            .any(|(_, cmd)| cmd.starts_with("/_get chats 1")),
        "{:?}",
        out.lines
    );
}

#[test]
fn a_group_not_in_the_list_fails_without_a_command() {
    let (mut c, _) = settled_bob();
    let out = c.request(Request::SetGroupPicture {
        group: 99,
        picture: None,
    });
    assert!(out.lines.is_empty());
    assert!(matches!(
        out.events.as_slice(),
        [ChatEvent::Failed {
            request: RequestKind::Group,
            ..
        }]
    ));
}
