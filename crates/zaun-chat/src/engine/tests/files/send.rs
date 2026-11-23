//! Files: send.

use super::*;

#[test]
fn a_sent_picture_goes_as_an_image_with_its_file_and_shows_the_local_copy() {
    let (mut t, fake) = setup();
    fake.with(|s| s.description = known());
    let (bob, _) = t.friend("bob");
    let path = put(&t, "party.gif");
    let out = send(&mut t, bob, path.clone(), FileKind::Gif);
    let [m] = messages(&out).try_into().unwrap();
    let here = std::fs::canonicalize(&path).unwrap();
    let shown = MessageBody::Gif(here.to_string_lossy().into_owned());
    assert_eq!(m.body, shown, "the local copy shows from the start");
    let ChatEvent::MsgNew(c) = sent_events(&t).remove(0) else {
        panic!()
    };
    assert!(matches!(c.content, MsgContent::Image { .. }));
    let inv = c.file.unwrap();
    assert_eq!((inv.file_name.as_str(), inv.file_size), ("party.gif", 17));
    assert_eq!(inv.file_descr.unwrap().file_descr_part_no, 0);

    assert!(
        messages(&settle(&mut t)).is_empty(),
        "nothing changes to show"
    );
    let events = sent_events(&t);
    assert!(matches!(events[1], ChatEvent::MsgFileDescr { .. }));
}

#[test]
fn a_cancelled_sent_gif_keeps_its_name_and_size() {
    let (mut t, fake) = setup();
    fake.with(|s| {
        s.description = known();
        s.hold = true;
    });
    let (bob, _) = t.friend("bob");
    let m = messages(&send_named(&mut t, bob, "party.gif", FileKind::Gif)).remove(0);
    let out = t.req(Request::CancelFile {
        chat: m.chat,
        item: m.id,
    });
    let cancelled = messages(&out).remove(0);
    let x = media(&cancelled);
    assert_eq!(
        (x.name.as_str(), x.size, &x.state),
        ("party.gif", 17, &FileState::Cancelled)
    );
}

#[test]
fn a_file_deleted_while_it_uploads_is_not_described_to_the_friend() {
    let (mut t, fake) = setup();
    fake.with(|s| {
        s.description = known();
        s.hold = true;
    });
    let (bob, _) = t.friend("bob");
    let m = messages(&send_named(&mut t, bob, "a.bin", FileKind::File)).remove(0);
    t.req(Request::Delete {
        chat: m.chat,
        item: m.id,
    });
    let key = FileKey {
        friend: bob,
        item: m.id,
    };
    let (job, description) = (fake.with(|s| s.jobs[0]), known());
    let result = Ok(crate::engine::files::transfers::Sent {
        description,
        expires: None,
    });
    t.e.on_job(JobDone::File(FileDone::Uploaded { key, job, result }));
    assert!(
        !sent_events(&t)
            .iter()
            .any(|e| matches!(e, ChatEvent::MsgFileDescr { .. }))
    );
}

#[test]
fn the_description_follows_the_message_in_parts_of_14000_characters() {
    let (mut t, fake) = setup();
    fake.with(|s| s.description = "x".repeat(30_000));
    let (bob, _) = t.friend("bob");
    let m = messages(&send_named(&mut t, bob, "a.bin", FileKind::File)).remove(0);
    settle(&mut t);
    let shared = shared_of(&mut t, bob, m.id);
    let got: Vec<_> = sent_events(&t)
        .into_iter()
        .filter_map(|e| {
            let ChatEvent::MsgFileDescr {
                msg_id,
                file_descr,
                file_expires,
                ..
            } = e
            else {
                return None;
            };
            Some((msg_id.0, file_descr, file_expires))
        })
        .collect();
    let numbers: Vec<_> = got
        .iter()
        .map(|(_, d, _)| {
            (
                d.file_descr_text.len(),
                d.file_descr_part_no,
                d.file_descr_complete,
            )
        })
        .collect();
    assert_eq!(
        numbers,
        [(14000, 1, false), (14000, 2, false), (2000, 3, true)]
    );
    assert!(got.iter().all(|(id, _, _)| *id == shared));
    assert_eq!(got[0].2.as_deref(), Some("2027-01-15T08:00:00Z"));
}

#[test]
fn sending_says_what_the_file_is_and_reports_progress() {
    let (mut t, fake) = setup();
    fake.with(|s| {
        s.description = known();
        s.hold = true;
    });
    let (bob, _) = t.friend("bob");
    let jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];
    let out = t.req(Request::SendFile {
        chat: ChatRef::Direct(bob),
        path: put(&t, "clip.mp4"),
        kind: FileKind::Video,
        preview: Some(jpeg.clone()),
        duration: Some(7),
        text: "clip".into(),
    });
    let m = messages(&out).remove(0);
    assert!(matches!(m.body, MessageBody::Video(_)));
    assert_eq!(
        (media(&m).duration, media(&m).preview.clone()),
        (Some(7), Some(jpeg))
    );
    let ChatEvent::MsgNew(c) = sent_events(&t).remove(0) else {
        panic!()
    };
    let MsgContent::Video {
        image,
        duration,
        text,
    } = c.content
    else {
        panic!()
    };
    assert_eq!(
        (duration, text.as_str(), image.safe().is_empty()),
        (7, "clip", false)
    );

    let f = messages(&send_named(&mut t, bob, "rules.pdf", FileKind::File)).remove(0);
    let ChatEvent::MsgNew(c) = sent_events(&t).pop().unwrap() else {
        panic!()
    };
    assert_eq!(
        c.content,
        MsgContent::File {
            text: String::new()
        }
    );
    assert_ne!(f.id, m.id);

    let key = FileKey {
        friend: bob,
        item: m.id,
    };
    let job = t.e.files.active[&key].job;
    t.e.on_job(JobDone::File(FileDone::Progress {
        key,
        job,
        percent: 40,
    }));
    let out = t.out();
    assert_eq!(
        media(&messages(&out)[0]).state,
        FileState::Sending { percent: 40 }
    );
    t.e.on_job(JobDone::File(FileDone::Progress {
        key,
        job,
        percent: 30,
    }));
    assert!(t.out().is_empty(), "progress never goes back");
}

#[test]
fn a_file_outside_the_data_folder_or_empty_is_refused_and_nothing_is_sent() {
    let (mut t, _) = setup();
    let (bob, _) = t.friend("bob");
    let outside =
        std::env::temp_dir().join(format!("outside-{}", t.dir.0.display().to_string().len()));
    std::fs::write(&outside, b"secret").unwrap();
    assert!(failed(
        &send(&mut t, bob, outside.clone(), FileKind::File),
        RequestKind::SendFile
    ));
    std::fs::remove_file(outside).unwrap();
    let empty = t.dir.0.join("files").join("empty");
    std::fs::write(&empty, b"").unwrap();
    assert!(failed(
        &send(&mut t, bob, empty, FileKind::File),
        RequestKind::SendFile
    ));
    let gone = t.dir.0.join("files").join("gone");
    assert!(failed(
        &send(&mut t, bob, gone, FileKind::File),
        RequestKind::SendFile
    ));
    assert!(t.e.agent.sent().is_empty());
}

#[test]
fn a_failed_upload_marks_the_item_failed_and_sends_no_description() {
    let (mut t, fake) = setup();
    fake.with(|s| s.fail = true);
    let (bob, _) = t.friend("bob");
    send_named(&mut t, bob, "a.bin", FileKind::File);
    let out = settle(&mut t);
    assert_eq!(media(&messages(&out)[0]).state, FileState::Failed);
    assert!(
        !sent_events(&t)
            .iter()
            .any(|e| matches!(e, ChatEvent::MsgFileDescr { .. }))
    );
}

#[test]
fn too_many_transfers_at_once_are_refused() {
    let (mut t, fake) = setup();
    fake.with(|s| s.hold = true);
    let (bob, _) = t.friend("bob");
    for n in 0..crate::engine::files::MAX_ACTIVE {
        let out = send_named(&mut t, bob, &format!("f{n}"), FileKind::File);
        assert!(!failed(&out, RequestKind::SendFile));
    }
    let out = send_named(&mut t, bob, "one-more", FileKind::File);
    assert!(failed(&out, RequestKind::SendFile));
}
