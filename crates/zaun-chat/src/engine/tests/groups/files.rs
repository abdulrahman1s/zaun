//! Files in a group: one upload with a description for each member, members' offers fetched, cancelled, opened and located as a friend's are (Subscriber.hs `SFDONE` group branch, `groupMessageFileDescription`, `xFileCancelGroup`).

use zaun_chat_wire::model::{ChatRef, FileState, MessageAuthor};
use zaun_chat_wire::{Event, FileKind, Request};

use super::super::files::{known, media, parts, picture, put, settle, setup};
use super::super::{T, messages};
use super::messages::{one, sent_to};
use crate::chat::ChatEvent;
use crate::chat::group::GroupMemberRole;
use crate::engine::files::transfers::SentToMembers;
use crate::engine::files::{FileDone, FileKey};
use crate::engine::jobs::JobDone;

fn shared_of(t: &mut T, g: i64, item: i64) -> Vec<u8> {
    t.e.item(ChatRef::Group(g), item)
        .unwrap()
        .unwrap()
        .shared_id
        .unwrap()
}

/// The description text a connection got, its parts joined.
fn descr_sent_to(t: &T, conn: &crate::engine::agent_port::ConnId) -> String {
    sent_to(t, conn)
        .into_iter()
        .filter_map(|e| match e {
            ChatEvent::MsgFileDescr { file_descr, .. } => Some(file_descr.file_descr_text),
            _ => None,
        })
        .collect()
}

#[test]
fn a_file_in_a_group_is_uploaded_once_and_each_member_gets_a_description_of_their_own() {
    let (mut t, fake) = setup();
    fake.with(|s| s.hold = true);
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Owner);
    let path = put(&t, "clip.pdf");
    let out = t.req(Request::SendFile {
        chat: ChatRef::Group(g),
        path,
        kind: FileKind::File,
        preview: None,
        duration: None,
        text: "for you all".into(),
    });
    let m = one(&out);
    for (_, conn) in &ms {
        let c = super::messages::only_new(&t, conn);
        assert_eq!(c.file.unwrap().file_name, "clip.pdf");
    }
    let key = FileKey::of(ChatRef::Group(g), m.id);
    assert_eq!(fake.with(|s| s.uploads.clone()), [key]);
    let job = fake.with(|s| s.jobs[0]);
    let result = Ok(SentToMembers {
        descriptions: vec!["for bob".into(), "for carol".into()],
        expires: None,
    });
    t.e.on_job(JobDone::File(FileDone::UploadedForMembers {
        key,
        job,
        result,
    }));
    let done = t.out();
    assert_eq!(media(&messages(&done)[0]).state, FileState::Sent);
    assert_eq!(descr_sent_to(&t, &ms[0].1), "for bob");
    assert_eq!(descr_sent_to(&t, &ms[1].1), "for carol");
}

#[test]
fn a_members_picture_is_fetched_once_its_description_is_whole_and_only_theirs_counts() {
    let (mut t, fake) = setup();
    fake.with(|s| s.save_as = "p.jpg".into());
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Member);
    let (bob, carol) = (ms[0].1, ms[1].1);
    let m = messages(&t.peer_says(&bob, 1, picture("p.jpg", 2048))).remove(0);
    assert_eq!(m.author, MessageAuthor::Member("bob".into()));
    let shared = shared_of(&mut t, g, m.id);
    for (n, p) in parts(&shared, &known(), 9000).into_iter().enumerate() {
        t.peer_says(&carol, 10 + n as i64, p);
    }
    assert!(
        fake.with(|s| s.downloads.is_empty()),
        "carol's parts are not bob's"
    );
    for (n, p) in parts(&shared, &known(), 9000).into_iter().enumerate() {
        t.peer_says(&bob, 20 + n as i64, p);
    }
    assert_eq!(
        fake.with(|s| s.downloads.clone()),
        [FileKey::of(ChatRef::Group(g), m.id)]
    );
    let got = settle(&mut t);
    assert!(matches!(
        media(&messages(&got)[0]).state,
        FileState::Received(_)
    ));
    let located = t.req(Request::LocateFile {
        chat: ChatRef::Group(g),
        item: m.id,
        tag: 3,
    });
    assert!(matches!(
        located[..],
        [Event::Located { path: Some(_), .. }]
    ));
}

#[test]
fn a_blocked_members_picture_is_not_fetched_by_itself() {
    let (mut t, fake) = setup();
    let (g, ms) = t.group("team", &["bob"], GroupMemberRole::Member);
    t.req(Request::SetMemberBlocked {
        group: g,
        member: ms[0].0,
        on: true,
    });
    let m = messages(&t.peer_says(&ms[0].1, 1, picture("p.jpg", 2048))).remove(0);
    let shared = shared_of(&mut t, g, m.id);
    for (n, p) in parts(&shared, &known(), 9000).into_iter().enumerate() {
        t.peer_says(&ms[0].1, 2 + n as i64, p);
    }
    assert!(fake.with(|s| s.downloads.is_empty()));
}

#[test]
fn cancelling_a_file_in_a_group_tells_every_member_and_a_members_cancel_stops_theirs() {
    let (mut t, fake) = setup();
    fake.with(|s| s.hold = true);
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Owner);
    let path = put(&t, "big.bin");
    let mine = one(&t.req(Request::SendFile {
        chat: ChatRef::Group(g),
        path,
        kind: FileKind::File,
        preview: None,
        duration: None,
        text: String::new(),
    }));
    t.req(Request::CancelFile {
        chat: ChatRef::Group(g),
        item: mine.id,
    });
    for (_, conn) in &ms {
        assert!(matches!(
            sent_to(&t, conn).pop(),
            Some(ChatEvent::FileCancel { .. })
        ));
    }

    let theirs = one(&t.peer_says(&ms[0].1, 1, picture("q.jpg", 5_000_000)));
    let shared = shared_of(&mut t, g, theirs.id);
    let msg_id = crate::chat::ids::SharedMsgId(shared);
    let c = one(&t.peer_says(&ms[0].1, 2, ChatEvent::FileCancel { msg_id }));
    assert_eq!(
        (c.id, media(&c).state.clone()),
        (theirs.id, FileState::Cancelled)
    );
}

#[test]
fn a_members_large_file_is_received_when_asked_and_opened_from_here() {
    let (mut t, fake) = setup();
    fake.with(|s| s.save_as = "big.jpg".into());
    let (g, ms) = t.group("team", &["bob"], GroupMemberRole::Member);
    let m = one(&t.peer_says(&ms[0].1, 1, picture("big.jpg", 5_000_000)));
    assert_eq!(media(&m).state, FileState::Offered);
    let chat = ChatRef::Group(g);
    t.req(Request::ReceiveFile { chat, item: m.id });
    let shared = shared_of(&mut t, g, m.id);
    for (n, p) in parts(&shared, &known(), 9000).into_iter().enumerate() {
        t.peer_says(&ms[0].1, 2 + n as i64, p);
    }
    let got = settle(&mut t);
    assert!(matches!(
        media(&messages(&got)[0]).state,
        FileState::Received(_)
    ));
    let opened = t.req(Request::OpenFile { chat, item: m.id });
    assert!(matches!(opened[..], [Event::OpenFile { .. }]));
}

#[test]
fn a_groups_upload_with_too_few_descriptions_fails_and_one_cut_short_by_a_restart_too() {
    let (mut t, fake) = setup();
    fake.with(|s| s.hold = true);
    let (g, _) = t.group("team", &["bob", "carol"], GroupMemberRole::Owner);
    let send = |t: &mut T, name: &str| {
        let path = put(t, name);
        one(&t.req(Request::SendFile {
            chat: ChatRef::Group(g),
            path,
            kind: FileKind::File,
            preview: None,
            duration: None,
            text: String::new(),
        }))
    };
    let first = send(&mut t, "a.bin");
    let key = FileKey::of(ChatRef::Group(g), first.id);
    let job = fake.with(|s| s.jobs[0]);
    let result = Ok(SentToMembers {
        descriptions: vec!["only one".into()],
        expires: None,
    });
    t.e.on_job(JobDone::File(FileDone::UploadedForMembers {
        key,
        job,
        result,
    }));
    assert_eq!(media(&messages(&t.out())[0]).state, FileState::Failed);

    let second = send(&mut t, "b.bin");
    let mut t = t.restarted();
    let page = t.e.group_page(g, None, 10).unwrap();
    let shown = page.iter().find(|m| m.id == second.id).unwrap();
    assert_eq!(
        media(shown).state,
        FileState::Failed,
        "the sweep finds the group's file"
    );
}
