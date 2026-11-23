//! Files: receive.

use super::*;

#[test]
fn a_small_received_picture_is_fetched_and_shows_its_file_once_here() {
    let (mut t, fake) = setup();
    fake.with(|s| s.save_as = "party.gif".into());
    let (bob, conn) = t.friend("bob");
    let m = peer_offers(&mut t, &conn, picture("party.gif", 200 * KB));
    let MessageBody::Image(media) = &m.body else {
        panic!("{:?}", m.body)
    };
    assert_eq!(
        (
            media.name.as_str(),
            media.caption.as_str(),
            media.preview.is_some()
        ),
        ("party.gif", "look", true)
    );
    // Small enough to be asked for by itself: on its way before the description is whole.
    let shared = shared_of(&mut t, bob, m.id);
    let text = known();
    let mut out = Vec::new();
    for (n, p) in parts(&shared, &text, 200).into_iter().enumerate() {
        out.extend(t.peer_says(&conn, 10 + n as i64, p));
    }
    assert_eq!(fake.with(|s| s.downloads.len()), 1);
    out.extend(settle(&mut t));
    let last = messages(&out).pop().unwrap();
    let dir = std::fs::canonicalize(t.dir.0.join("files")).unwrap();
    assert_eq!(
        last.body,
        MessageBody::Gif(dir.join("party.gif").to_string_lossy().into_owned())
    );
}

#[test]
fn a_large_picture_fetches_nothing_until_the_user_asks() {
    let (mut t, fake) = setup();
    fake.with(|s| s.save_as = "photo.jpg".into());
    let (bob, conn) = t.friend("bob");
    let m = peer_offers(&mut t, &conn, picture("photo.jpg", 2 * 1024 * KB));
    let shared = shared_of(&mut t, bob, m.id);
    for (n, p) in parts(&shared, &known(), 300).into_iter().enumerate() {
        t.peer_says(&conn, 10 + n as i64, p);
    }
    assert!(fake.with(|s| s.downloads.is_empty()));
    let out = t.req(Request::ReceiveFile {
        chat: m.chat,
        item: m.id,
    });
    assert_eq!(
        media(&messages(&out)[0]).state,
        FileState::Receiving { percent: 0 }
    );
    let out = settle(&mut t);
    let shown = messages(&out);
    let FileState::Received(at) = &media(&shown[0]).state else {
        panic!("{out:?}")
    };
    assert!(at.ends_with("photo.jpg"));
}

#[test]
fn a_file_server_the_user_does_not_know_is_not_fetched_from() {
    let (mut t, fake) = setup();
    let (bob, conn) = t.friend("bob");
    let stranger: XftpServer =
        "xftp://AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=@files.example.org"
            .parse()
            .unwrap();
    let m = peer_offers(&mut t, &conn, picture("p.jpg", 10 * KB));
    let shared = shared_of(&mut t, bob, m.id);
    let mut shown = Vec::new();
    for (n, p) in parts(&shared, &description(&stranger), 250)
        .into_iter()
        .enumerate()
    {
        shown.extend(t.peer_says(&conn, 10 + n as i64, p));
    }
    assert!(fake.with(|s| s.downloads.is_empty()));
    let last = messages(&shown).pop().unwrap();
    assert_eq!(
        media(&last).state,
        FileState::Offered,
        "back to offered, nothing broke"
    );
    let out = t.req(Request::ReceiveFile {
        chat: m.chat,
        item: m.id,
    });
    assert!(failed(&out, RequestKind::File));
    assert!(fake.with(|s| s.downloads.is_empty()));
}

#[test]
fn a_video_and_a_file_are_offered_and_a_file_can_be_cancelled() {
    let (mut t, fake) = setup();
    let (_, conn) = t.friend("bob");
    let video = MsgContent::Video {
        text: String::new(),
        image: jpeg(),
        duration: 7,
    };
    let v = peer_offers(&mut t, &conn, offered(video, "clip.mp4", 5000 * KB));
    assert!(matches!(v.body, MessageBody::Video(_)));
    assert_eq!(
        (media(&v).duration, media(&v).preview.is_some()),
        (Some(7), true)
    );
    let pdf = MsgContent::File {
        text: String::new(),
    };
    let f = peer_offers(&mut t, &conn, offered(pdf, "rules.pdf", 4 * KB));
    assert!(matches!(f.body, MessageBody::File(_)));
    assert_eq!(
        (media(&f).name.as_str(), media(&f).preview.is_none()),
        ("rules.pdf", true)
    );

    // Asked for before its description is whole: waiting, then cancelled by the user.
    let out = t.req(Request::ReceiveFile {
        chat: f.chat,
        item: f.id,
    });
    assert_eq!(
        media(&messages(&out)[0]).state,
        FileState::Receiving { percent: 0 }
    );
    let out = t.req(Request::CancelFile {
        chat: f.chat,
        item: f.id,
    });
    assert_eq!(media(&messages(&out)[0]).state, FileState::Cancelled);
    assert!(
        t.e.agent.sent().is_empty(),
        "nothing is sent for a file being received"
    );
    assert!(fake.with(|s| s.downloads.is_empty()));

    // Nothing to open before it is here; a message without a file is refused.
    for item in [f.id, 999] {
        let out = t.req(Request::OpenFile { chat: f.chat, item });
        assert!(failed(&out, RequestKind::File));
    }
}

#[test]
fn a_description_part_out_of_order_fails_the_file() {
    let (mut t, _) = setup();
    let (bob, conn) = t.friend("bob");
    let f = peer_offers(
        &mut t,
        &conn,
        offered(
            MsgContent::File {
                text: String::new(),
            },
            "r.pdf",
            4 * KB,
        ),
    );
    let shared = shared_of(&mut t, bob, f.id);
    let mut all = parts(&shared, &known(), 100);
    assert!(all.len() > 3);
    t.peer_says(&conn, 10, all.remove(0));
    let out = t.peer_says(&conn, 11, all.remove(1));
    assert_eq!(media(&messages(&out)[0]).state, FileState::Failed);
}

#[test]
fn a_description_for_no_message_or_a_text_message_is_ignored() {
    let (mut t, _) = setup();
    let (bob, conn) = t.friend("bob");
    let stray = parts(b"nothing", &known(), 300).remove(0);
    assert!(t.peer_says(&conn, 1, stray).is_empty());
    let text =
        messages(&t.peer_says(&conn, 2, ChatEvent::MsgNew(MsgContainer::text("hi")))).remove(0);
    let shared = shared_of(&mut t, bob, text.id);
    assert!(
        t.peer_says(&conn, 3, parts(&shared, &known(), 300).remove(0))
            .is_empty()
    );
}

#[test]
fn an_offer_of_no_bytes_makes_no_item_and_a_legacy_offer_is_shown_failed() {
    let (mut t, _) = setup();
    let (_, conn) = t.friend("bob");
    let none = offered(
        MsgContent::File {
            text: String::new(),
        },
        "x",
        0,
    );
    assert!(t.peer_says(&conn, 1, none).is_empty());
    let mut legacy = MsgContainer::simple(MsgContent::File {
        text: String::new(),
    });
    let mut inv = FileInvitation::xftp("old.bin", 10);
    inv.file_descr = None;
    legacy.file = Some(inv);
    let m = messages(&t.peer_says(&conn, 2, ChatEvent::MsgNew(legacy))).remove(0);
    assert_eq!(media(&m).state, FileState::Failed);
}

#[test]
fn a_file_name_from_a_stranger_is_made_safe_for_display() {
    let (mut t, _) = setup();
    let (_, conn) = t.friend("bob");
    let m = peer_offers(
        &mut t,
        &conn,
        offered(
            MsgContent::File {
                text: String::new(),
            },
            "../../etc/passwd",
            10,
        ),
    );
    assert_eq!(media(&m).name, "passwd");
}

#[test]
fn a_blocked_friends_file_is_never_kept_or_fetched() {
    let (mut t, fake) = setup();
    let (bob, conn) = t.friend("bob");
    t.req(Request::SetBlocked {
        friend: bob,
        on: true,
    });
    assert!(t.peer_says(&conn, 1, picture("p.jpg", KB)).is_empty());
    assert!(fake.with(|s| s.downloads.is_empty()));
}

#[test]
fn a_file_server_with_a_password_in_the_users_list_matches_its_description() {
    let (mut t, fake) = setup();
    fake.with(|s| s.save_as = "r.pdf".into());
    let server = preset_servers()[0].clone();
    let mut keyed = server.clone();
    keyed.basic_auth = Some("secret".into());
    t.e.files.servers = vec![keyed];
    let (bob, conn) = t.friend("bob");
    let f = peer_offers(
        &mut t,
        &conn,
        offered(
            MsgContent::File {
                text: String::new(),
            },
            "r.pdf",
            4 * KB,
        ),
    );
    let shared = shared_of(&mut t, bob, f.id);
    t.req(Request::ReceiveFile {
        chat: f.chat,
        item: f.id,
    });
    for (n, p) in parts(&shared, &description(&server), 400)
        .into_iter()
        .enumerate()
    {
        t.peer_says(&conn, 10 + n as i64, p);
    }
    assert_eq!(fake.with(|s| s.downloads.len()), 1);
}

#[test]
fn the_file_servers_are_the_hellos_xftp_relays_or_the_presets() {
    let own = "xftp://AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=@files.example.org";
    let mut t = T::new();
    let hello = crate::engine::tests::Dir::new().hello("alice");
    t.e.start(&hello).unwrap();
    assert_eq!(t.e.files.servers, preset_servers());
    let mut custom = t.dir.hello("alice");
    custom.relays = vec!["smp://x@relay.example".into(), own.into()];
    t.e.start(&custom).unwrap();
    assert_eq!(t.e.files.servers, vec![own.parse::<XftpServer>().unwrap()]);
}

#[test]
fn a_friends_edit_of_a_file_message_changes_its_caption_not_its_kind() {
    let (mut t, _) = setup();
    let (bob, conn) = t.friend("bob");
    let m = peer_offers(&mut t, &conn, picture("photo.jpg", 2 * 1024 * KB));
    let shared = shared_of(&mut t, bob, m.id);
    let edit = ChatEvent::MsgUpdate {
        msg_id: SharedMsgId(shared),
        content: MsgContent::Text {
            text: "better".into(),
        },
        mentions: Default::default(),
        ttl: None,
        live: None,
        scope: None,
        as_group: None,
    };
    let out = t.peer_says(&conn, 5, edit);
    let u = messages(&out).remove(0);
    assert_eq!((media(&u).caption.as_str(), u.edited), ("better", true));
    assert!(matches!(u.body, MessageBody::Image(_)));
}
