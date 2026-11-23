//! Files: cancel.

use super::*;

#[test]
fn cancelling_a_sent_file_tells_the_friend_and_a_late_upload_result_changes_nothing() {
    let (mut t, fake) = setup();
    fake.with(|s| {
        s.description = known();
        s.hold = true;
    });
    let (bob, _) = t.friend("bob");
    let m = messages(&send_named(&mut t, bob, "a.bin", FileKind::File)).remove(0);
    let shared = shared_of(&mut t, bob, m.id);
    let out = t.req(Request::CancelFile {
        chat: m.chat,
        item: m.id,
    });
    assert_eq!(media(&messages(&out)[0]).state, FileState::Cancelled);
    assert!(fake.with(|s| s.flags[0].1.load(std::sync::atomic::Ordering::Relaxed)));
    let last = sent_events(&t).pop().unwrap();
    assert_eq!(
        last,
        ChatEvent::FileCancel {
            msg_id: SharedMsgId(shared)
        }
    );

    let key = FileKey {
        friend: bob,
        item: m.id,
    };
    let done = FileDone::Uploaded {
        key,
        job: fake.with(|s| s.jobs[0]),
        result: Err("cancelled".into()),
    };
    t.e.on_job(JobDone::File(done));
    assert!(t.out().is_empty());
    let out = t.req(Request::CancelFile {
        chat: m.chat,
        item: m.id,
    });
    assert!(failed(&out, RequestKind::File), "already cancelled");
}

#[test]
fn a_cancelled_received_file_can_be_asked_for_again() {
    let (mut t, fake) = setup();
    fake.with(|s| s.save_as = "r.pdf".into());
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
    t.req(Request::ReceiveFile {
        chat: f.chat,
        item: f.id,
    });
    t.req(Request::CancelFile {
        chat: f.chat,
        item: f.id,
    });
    let shared = shared_of(&mut t, bob, f.id);
    for (n, p) in parts(&shared, &known(), 400).into_iter().enumerate() {
        t.peer_says(&conn, 10 + n as i64, p);
    }
    assert!(
        fake.with(|s| s.downloads.is_empty()),
        "cancelled: the parts are only kept"
    );
    t.req(Request::ReceiveFile {
        chat: f.chat,
        item: f.id,
    });
    assert_eq!(fake.with(|s| s.downloads.len()), 1);
    let out = settle(&mut t);
    assert!(matches!(
        media(&messages(&out)[0]).state,
        FileState::Received(_)
    ));
}

#[test]
fn the_sender_cancelling_stops_the_fetch_and_later_parts_are_ignored() {
    let (mut t, fake) = setup();
    fake.with(|s| s.hold = true);
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
    for (n, p) in parts(&shared, &known(), 400).into_iter().enumerate() {
        t.peer_says(&conn, 10 + n as i64, p);
    }
    assert_eq!(fake.with(|s| s.downloads.len()), 1);
    let out = t.peer_says(
        &conn,
        30,
        ChatEvent::FileCancel {
            msg_id: SharedMsgId(shared.clone()),
        },
    );
    assert_eq!(media(&messages(&out)[0]).state, FileState::Cancelled);
    assert!(fake.with(|s| s.flags[0].1.load(std::sync::atomic::Ordering::Relaxed)));
    let out = t.req(Request::ReceiveFile {
        chat: f.chat,
        item: f.id,
    });
    assert!(failed(&out, RequestKind::File), "the sender took it back");
    let again = ChatEvent::FileCancel {
        msg_id: SharedMsgId(shared),
    };
    assert!(t.peer_says(&conn, 31, again).is_empty());
}

#[test]
fn a_result_of_an_older_run_of_the_same_file_changes_nothing() {
    let (mut t, fake) = setup();
    fake.with(|s| {
        s.hold = true;
        s.save_as = "r.pdf".into();
    });
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
    for (n, p) in parts(&shared, &known(), 400).into_iter().enumerate() {
        t.peer_says(&conn, 10 + n as i64, p);
    }
    t.req(Request::ReceiveFile {
        chat: f.chat,
        item: f.id,
    });
    t.req(Request::CancelFile {
        chat: f.chat,
        item: f.id,
    });
    t.req(Request::ReceiveFile {
        chat: f.chat,
        item: f.id,
    });
    let jobs = fake.with(|s| s.jobs.clone());
    assert_eq!(jobs.len(), 2);
    let key = FileKey {
        friend: bob,
        item: f.id,
    };
    let old = FileDone::Downloaded {
        key,
        job: jobs[0],
        result: Err("cancelled".into()),
    };
    t.e.on_job(JobDone::File(old));
    assert!(t.out().is_empty(), "the old run's end is not the new run's");
    let got = t.dir.0.join("files").join("r.pdf");
    std::fs::write(&got, b"x").unwrap();
    let new = FileDone::Downloaded {
        key,
        job: jobs[1],
        result: Ok(got.clone()),
    };
    t.e.on_job(JobDone::File(new));
    let out = t.out();
    assert!(matches!(
        media(&messages(&out)[0]).state,
        FileState::Received(_)
    ));
    assert!(got.exists());
}

#[test]
fn removing_a_friend_stops_their_transfers_and_deletes_their_files() {
    let (mut t, fake) = setup();
    fake.with(|s| {
        s.description = known();
        s.hold = true;
    });
    let (bob, _) = t.friend("bob");
    let path = put(&t, "clip.mp4");
    send(&mut t, bob, path.clone(), FileKind::File);
    let flag = fake.with(|s| std::sync::Arc::clone(&s.flags[0].1));
    t.req(Request::RemoveFriend(bob));
    assert!(
        flag.load(std::sync::atomic::Ordering::Relaxed),
        "the upload is stopped"
    );
    assert!(!path.exists(), "the copy in the engine's folder is deleted");
    let left =
        t.e.store
            .keys_with_prefix(
                crate::engine::records::NS,
                &crate::engine::records::keys::file_prefix(bob),
            )
            .unwrap();
    assert!(left.is_empty(), "no file record is left behind");
}
