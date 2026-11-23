//! Files: locate.

use super::*;

#[test]
fn locating_a_file_for_a_copy_answers_at_once_even_when_it_is_not_here() {
    let (mut t, fake) = setup();
    fake.with(|s| s.save_as = "photo.jpg".into());
    let (bob, conn) = t.friend("bob");
    let m = peer_offers(&mut t, &conn, picture("photo.jpg", 2 * 1024 * KB));
    let out = t.req(Request::LocateFile {
        chat: m.chat,
        item: m.id,
        tag: 5,
    });
    assert_eq!(out, vec![Event::Located { tag: 5, path: None }]);
    let shared = shared_of(&mut t, bob, m.id);
    for (n, p) in parts(&shared, &known(), 300).into_iter().enumerate() {
        t.peer_says(&conn, 10 + n as i64, p);
    }
    t.req(Request::ReceiveFile {
        chat: m.chat,
        item: m.id,
    });
    settle(&mut t);
    let out = t.req(Request::LocateFile {
        chat: m.chat,
        item: m.id,
        tag: 6,
    });
    let [
        Event::Located {
            tag: 6,
            path: Some(p),
        },
    ] = out.as_slice()
    else {
        panic!("{out:?}")
    };
    assert!(p.ends_with("photo.jpg"));
    let out = t.req(Request::OpenFile {
        chat: m.chat,
        item: m.id,
    });
    assert_eq!(
        out,
        vec![Event::OpenFile {
            path: p.clone(),
            kind: FileKind::Image
        }]
    );
    // A message that is no file, and a file deleted since: nothing here, no failure.
    std::fs::remove_file(p).unwrap();
    let out = t.req(Request::LocateFile {
        chat: m.chat,
        item: m.id,
        tag: 7,
    });
    assert_eq!(out, vec![Event::Located { tag: 7, path: None }]);
    let out = t.req(Request::LocateFile {
        chat: m.chat,
        item: 999,
        tag: 8,
    });
    assert_eq!(out, vec![Event::Located { tag: 8, path: None }]);
}

#[test]
fn a_restart_with_a_transfer_half_done_ends_it_without_a_crash() {
    let (mut t, fake) = setup();
    fake.with(|s| s.hold = true);
    let (bob, conn) = t.friend("bob");
    let sent = messages(&send_named(&mut t, bob, "a.bin", FileKind::File)).remove(0);
    let got = peer_offers(
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
    let shared = shared_of(&mut t, bob, got.id);
    t.req(Request::ReceiveFile {
        chat: got.chat,
        item: got.id,
    });
    for (n, p) in parts(&shared, &known(), 400).into_iter().enumerate() {
        t.peer_says(&conn, 10 + n as i64, p);
    }
    assert_eq!(fake.with(|s| s.downloads.len()), 1);

    let hello = t.dir.hello("alice");
    let store = crate::store::Store::open(&t.dir.0.join("chat.db"), KEY).unwrap();
    drop(std::mem::replace(
        &mut t.e,
        crate::engine::machine::Engine::new(
            store,
            FakeAgent::default(),
            Box::new(t.clock.clone()),
            crate::engine::log::Log::discard(),
        ),
    ));
    t.e.start(&hello).unwrap();
    let out = t.out();
    let states: Vec<_> = messages(&out)
        .iter()
        .map(|m| media(m).state.clone())
        .collect();
    assert!(states.contains(&FileState::Failed), "{states:?}");
    assert!(states.contains(&FileState::Offered), "{states:?}");
    let _ = sent;
}
