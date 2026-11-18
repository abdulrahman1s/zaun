//! Chat end to end (ADR-0033): two real chat engines (`easyplay-chat` + libsimplex,
//! under bwrap) connect through an SMP relay, exchange messages, form a group, and keep
//! their history across a restart.
//!
//! Ignored by default: it needs the engine and a relay. Run it in a network namespace that
//! has only loopback, with a local `smp-server` listening, so no public relay is touched:
//!
//! ```text
//! EASYPLAY_SIMPLEX_LIB=/…/libsimplex.so \
//! EASYPLAY_SIMPLEX_LIBRARY_PATH=/…/gmp/lib:/…/zlib/lib \
//! EASYPLAY_TEST_SMP='smp://<fingerprint>@127.0.0.1:5223' \   # or `preset` (public relays)
//! EASYPLAY_TEST_XFTP='xftp://<fingerprint>@127.0.0.1:5443' \ # optional: the file step
//! cargo test -p easyplay-core --test chat_e2e -- --ignored
//! ```
//!
//! Beyond the foundation it replies, reacts, edits and deletes for everyone, and with
//! `EASYPLAY_TEST_XFTP` (a local `xftp-server`) sends a GIF from outside the Chat folder the
//! way the service does (copied in first) and checks the friend's helper fetched it. With
//! `EASYPLAY_TEST_GIPHY=1` as well, that GIF is a real one: searched and downloaded from
//! Giphy through Signal's content proxy exactly as `ChatCommand::SendGif` does (this step
//! needs the internet; the chat itself stays on the local relay and file server).

// Real processes against wall time, files in a temporary directory.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use easyplay_core::chat::{
    ChatEvent, Engine, EngineConfig, EngineFactory, EventSink, FileKind, Request, RequestKind,
    SimplexFactory,
};
use easyplay_core::model::{
    ChatMessage, ChatRef, FileState, Media, MessageAuthor, MessageBody, Quote,
};

const WAIT: Duration = Duration::from_secs(60);

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set (see the file's header)"))
}

fn factory(log: PathBuf) -> SimplexFactory {
    let helper = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/easyplay-chat");
    assert!(
        helper.is_file(),
        "build it first: cargo build -p easyplay-chat"
    );
    // By its full path: the helper's environment has no `PATH` lookup.
    let path = std::env::var_os("PATH").unwrap_or_default();
    let bwrap = std::env::split_paths(&path)
        .map(|d| d.join("bwrap"))
        .find(|b| b.is_file());
    SimplexFactory {
        helper,
        library: env("EASYPLAY_SIMPLEX_LIB").into(),
        bwrap,
        library_path: std::env::var_os("EASYPLAY_SIMPLEX_LIBRARY_PATH"),
        log,
    }
}

/// The local relay's address on a loopback address nothing listens on.
fn dead_relay(relay: &str) -> Option<String> {
    // Any local port (agents running at once use their own).
    relay
        .split_once("@127.0.0.1:")
        .map(|(key, _)| format!("{key}@127.0.0.2:5999"))
}

struct Peer {
    engine: Box<dyn Engine>,
    events: Receiver<ChatEvent>,
    log: Arc<Mutex<Vec<ChatEvent>>>,
}

impl Peer {
    fn start(f: &SimplexFactory, dir: PathBuf, name: &str, key: &str) -> Peer {
        let (tx, rx) = channel();
        let tx = Mutex::new(tx);
        let log = Arc::new(Mutex::new(Vec::new()));
        let log2 = Arc::clone(&log);
        let sink: EventSink = Arc::new(move |e: ChatEvent| {
            log2.lock().unwrap().push(e.clone());
            let _ = tx.lock().unwrap().send(e);
        });
        let engine = f
            .start(
                EngineConfig {
                    dir,
                    key: key.into(),
                    name: name.into(),
                    relays: match env("EASYPLAY_TEST_SMP").as_str() {
                        // The engine's preset public relays: the path every user takes.
                        "preset" => Vec::new(),
                        // With a local relay, a second one that never answers: the relay
                        // check must disable it, or about half the new queues time out.
                        relay => std::iter::once(relay.to_owned())
                            .chain(dead_relay(relay))
                            .chain(std::env::var("EASYPLAY_TEST_XFTP").ok())
                            .collect(),
                    },
                },
                sink,
            )
            .expect("engine starts");
        let p = Peer {
            engine,
            events: rx,
            log,
        };
        p.wait(|e| matches!(e, ChatEvent::Ready(_)).then_some(()), "ready");
        p
    }

    fn wait<T>(&self, mut f: impl FnMut(&ChatEvent) -> Option<T>, what: &str) -> T {
        let end = std::time::Instant::now() + WAIT;
        loop {
            let left = end.saturating_duration_since(std::time::Instant::now());
            match self.events.recv_timeout(left) {
                Ok(ChatEvent::Failed {
                    request,
                    code,
                    detail,
                }) => {
                    eprintln!("(failed {request:?} {code:?}: {detail})");
                }
                Ok(e) => {
                    if let Some(t) = f(&e) {
                        return t;
                    }
                }
                Err(_) => panic!(
                    "timed out waiting for {what}; saw {:#?}",
                    self.log.lock().unwrap()
                ),
            }
        }
    }

    /// The first update of a message that `f` accepts.
    fn updated(&self, what: &str, mut f: impl FnMut(&ChatMessage) -> bool) -> ChatMessage {
        self.wait(
            |e| match e {
                ChatEvent::Updated(m) | ChatEvent::Messages(m) => m.iter().find(|m| f(m)).cloned(),
                _ => None,
            },
            what,
        )
    }

    fn text(&self, wanted: &str) -> ChatRef {
        self.wait(
            |e| match e {
                ChatEvent::Messages(m) => m
                    .iter()
                    .find(|m| m.body == MessageBody::Text(wanted.into()))
                    .map(|m| m.chat),
                _ => None,
            },
            wanted,
        )
    }

    /// The first friend (once the list has one that is ready).
    fn friend(&self) -> i64 {
        self.engine.request(Request::Refresh);
        self.wait(
            |e| match e {
                ChatEvent::Chats { friends, .. } => friends.iter().find(|f| f.ready).map(|f| f.id),
                _ => None,
            },
            "a ready friend",
        )
    }
}

#[test]
#[ignore = "needs libsimplex and a local SMP relay (see the file's header)"]
fn two_engines_talk_through_a_relay_and_keep_their_history() {
    let tmp = tempfile::tempdir().unwrap();
    // EasyPlay's own log, to see the relay check at work.
    let app_log = tmp.path().join("easyplay.log");
    let mut opts =
        easyplay_core::logging::LogOptions::new(easyplay_core::logging::ProcessRole::Cli);
    opts.file = Some(app_log.clone());
    let _log = easyplay_core::logging::init(&opts).unwrap();
    let f = factory(tmp.path().join("engine.log"));
    let (a_dir, b_dir) = (tmp.path().join("alice"), tmp.path().join("bob"));
    for d in [&a_dir, &b_dir] {
        std::fs::create_dir_all(d).unwrap();
    }
    let a = Peer::start(&f, a_dir.clone(), "alice", "alice-key");
    let b = Peer::start(&f, b_dir.clone(), "bob", "bob-key");

    // Several invitations in a row: each new queue goes to a random enabled relay, so
    // without the relay check some would land on the dead one and fail.
    let mut link = String::new();
    for i in 0..4 {
        a.engine.request(Request::CreateInvite);
        link = a.wait(
            |e| match e {
                ChatEvent::Invite(l) => Some(l.clone()),
                _ => None,
            },
            &format!("invitation {i}"),
        );
    }
    assert!(
        !a.log.lock().unwrap().iter().any(|e| matches!(
            e,
            ChatEvent::Failed {
                request: RequestKind::CreateInvite,
                ..
            }
        )),
        "an invitation failed"
    );
    if dead_relay(&env("EASYPLAY_TEST_SMP")).is_some() {
        // The check disabled the relay that does not answer (and only that one).
        let wait_until = std::time::Instant::now() + WAIT;
        loop {
            let log = std::fs::read_to_string(&app_log).unwrap_or_default();
            let disabled: Vec<&str> = log
                .lines()
                .filter(|l| l.contains("chat: relay") && l.contains("enabled=false"))
                .collect();
            if !disabled.is_empty() {
                assert!(
                    disabled.iter().all(|l| l.contains("127.0.0.2:5999")),
                    "{disabled:?}"
                );
                break;
            }
            assert!(
                std::time::Instant::now() < wait_until,
                "no relay was disabled:\n{log}"
            );
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    b.engine.request(Request::Connect(link));
    // Both lists gain the other once the connection is ready.
    let wait_friend = |p: &Peer| {
        for _ in 0..30 {
            if let Some(id) = {
                p.engine.request(Request::Refresh);
                p.wait(
                    |e| match e {
                        ChatEvent::Chats { friends, .. } => {
                            Some(friends.iter().find(|f| f.ready).map(|f| f.id))
                        }
                        _ => None,
                    },
                    "the list",
                )
            } {
                return id;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        panic!("no ready friend")
    };
    let a_bob = wait_friend(&a);
    let b_alice = wait_friend(&b);

    a.engine.request(Request::Send {
        chat: ChatRef::Direct(a_bob),
        text: "hello from EasyPlay".into(),
        reply_to: None,
        mentions: Vec::new(),
    });
    assert_eq!(b.text("hello from EasyPlay"), ChatRef::Direct(b_alice));
    b.engine.request(Request::Send {
        chat: ChatRef::Direct(b_alice),
        text: "أهلاً".into(),
        reply_to: None,
        mentions: Vec::new(),
    });
    a.text("أهلاً");

    // A profile picture (ADR-0038): made from a photo as the service makes it, taken by
    // alice's engine, and seen by bob once his list is read again.
    let photo = image::RgbImage::from_fn(1200, 800, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
    });
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90)
        .encode_image(&photo)
        .unwrap();
    let pic = easyplay_core::chat::picture::make(&jpeg, None).unwrap();
    a.engine.request(Request::SetPicture(Some(pic.clone())));
    let mine = a.wait(
        |e| match e {
            ChatEvent::Profile(p) => Some(p.picture.clone()),
            _ => None,
        },
        "alice's profile with the picture",
    );
    assert_eq!(mine.as_ref().map(|p| p.key()), Some(pic.key()));
    let seen = (0..30)
        .find_map(|_| {
            b.engine.request(Request::Refresh);
            let got = b.wait(
                |e| match e {
                    ChatEvent::Chats { friends, .. } => Some(
                        friends
                            .iter()
                            .find(|f| f.id == b_alice)
                            .and_then(|f| f.picture.clone()),
                    ),
                    _ => None,
                },
                "bob's list",
            );
            if got.is_none() {
                std::thread::sleep(Duration::from_millis(500));
            }
            got
        })
        .expect("bob sees alice's picture");
    assert_eq!(seen.bytes(), pic.bytes());

    // Reply, react, edit, delete for everyone.
    b.engine.request(Request::History {
        chat: ChatRef::Direct(b_alice),
        before: None,
        count: 20,
        tag: 9,
    });
    let b_hello = b.wait(
        |e| match e {
            ChatEvent::History {
                tag: 9, messages, ..
            } => messages
                .iter()
                .find(|m| m.body == MessageBody::Text("hello from EasyPlay".into()))
                .map(|m| m.id),
            _ => None,
        },
        "bob's copy of hello",
    );
    b.engine.request(Request::Send {
        chat: ChatRef::Direct(b_alice),
        text: "replying".into(),
        reply_to: Some(b_hello),
        mentions: Vec::new(),
    });
    let reply = a.updated("the reply", |m| {
        m.body == MessageBody::Text("replying".into())
    });
    let Some(Quote {
        id: Some(a_hello),
        author: MessageAuthor::Me,
        text,
    }) = reply.reply
    else {
        panic!("{reply:?}")
    };
    assert_eq!(text, "hello from EasyPlay");
    b.engine.request(Request::React {
        chat: ChatRef::Direct(b_alice),
        item: b_hello,
        emoji: "❤️".into(),
        on: true,
    });
    let reacted = a.updated("the reaction", |m| {
        m.id == a_hello && !m.reactions.is_empty()
    });
    assert_eq!(reacted.reactions[0].emoji, "❤️");
    assert!(!reacted.reactions[0].mine);
    a.engine.request(Request::Send {
        chat: ChatRef::Direct(a_bob),
        text: "typo".into(),
        reply_to: None,
        mentions: Vec::new(),
    });
    let a_typo = a
        .updated("alice's typo", |m| {
            m.body == MessageBody::Text("typo".into())
        })
        .id;
    let b_typo = b
        .updated("bob's typo", |m| m.body == MessageBody::Text("typo".into()))
        .id;
    a.engine.request(Request::Edit {
        chat: ChatRef::Direct(a_bob),
        item: a_typo,
        text: "fixed".into(),
    });
    let edited = b.updated("the edit", |m| m.id == b_typo && m.edited);
    assert_eq!(edited.body, MessageBody::Text("fixed".into()));
    a.engine.request(Request::Delete {
        chat: ChatRef::Direct(a_bob),
        item: a_typo,
    });
    b.updated("the deletion", |m| {
        m.id == b_typo && m.body == MessageBody::Deleted
    });

    // A GIF from outside the Chat folder, copied in first (the service's part).
    if std::env::var("EASYPLAY_TEST_XFTP").is_ok() {
        let gif = a_dir.join("files").join("sent").join("wave.gif");
        std::fs::create_dir_all(gif.parent().unwrap()).unwrap();
        let outside = tmp.path().join("wave.gif");
        let bytes = if std::env::var("EASYPLAY_TEST_GIPHY").is_ok() {
            giphy_gif()
        } else {
            b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x01D\x00;".to_vec()
        };
        std::fs::write(&outside, &bytes).unwrap();
        std::fs::copy(&outside, &gif).unwrap();
        a.engine.request(Request::SendFile {
            chat: ChatRef::Direct(a_bob),
            path: gif,
            kind: FileKind::Gif,
            preview: None,
            duration: None,
            text: String::new(),
        });
        let got = b.updated(
            "the GIF, fetched",
            |m| matches!(&m.body, MessageBody::Gif(p) if p.starts_with('/')),
        );
        let MessageBody::Gif(path) = got.body else {
            unreachable!()
        };
        assert!(path.starts_with(b_dir.to_str().unwrap()), "{path}");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            std::fs::read(&outside).unwrap()
        );

        // A 3 MB picture: offered with its preview, fetched when asked, identical.
        let photo = a_dir.join("files").join("sent").join("IMG_3mb.png");
        let mut seed = 0x2545_f491_u32;
        let img = image::RgbImage::from_fn(1024, 1024, |_, _| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let [r, g, b, _] = seed.to_le_bytes();
            image::Rgb([r, g, b])
        });
        img.save(&photo).unwrap();
        let photo_bytes = std::fs::read(&photo).unwrap();
        assert!(photo_bytes.len() > 3_000_000, "{}", photo_bytes.len());
        let preview = easyplay_core::service::flows::chat::preview_jpeg(
            &photo_bytes,
            easyplay_core::service::flows::chat::PREVIEW_BUDGET,
        );
        assert!(preview.is_some());
        a.engine.request(Request::SendFile {
            chat: ChatRef::Direct(a_bob),
            path: photo,
            kind: FileKind::Image,
            preview: preview.clone(),
            duration: None,
            text: String::new(),
        });
        let offered = b.updated(
            "the photo, offered",
            |m| matches!(&m.body, MessageBody::Image(x) if x.name == "IMG_3mb.png"),
        );
        let MessageBody::Image(x) = &offered.body else {
            unreachable!()
        };
        assert_eq!(
            x.state,
            FileState::Offered,
            "too large to be fetched by itself"
        );
        assert_eq!(x.preview, preview);
        assert_eq!(x.size, photo_bytes.len() as u64);
        b.engine.request(Request::ReceiveFile {
            chat: offered.chat,
            item: offered.id,
        });
        let got = b.updated("the photo, fetched", |m| {
            m.id == offered.id
                && matches!(&m.body, MessageBody::Image(x) if matches!(x.state, FileState::Received(_)))
        });
        let MessageBody::Image(Media {
            state: FileState::Received(path),
            ..
        }) = got.body
        else {
            unreachable!()
        };
        assert!(path.starts_with(&b_dir), "{path:?}");
        assert_eq!(std::fs::read(&path).unwrap(), photo_bytes);
    }

    a.engine.request(Request::NewGroup("squad".into()));
    let gid = a.wait(
        |e| match e {
            ChatEvent::Chats { groups, .. } => {
                groups.iter().find(|g| g.name == "squad").map(|g| g.id)
            }
            _ => None,
        },
        "the group",
    );
    a.engine.request(Request::AddMember {
        group: gid,
        friend: a_bob,
    });
    let b_gid = b.wait(
        |e| match e {
            ChatEvent::Chats { groups, .. } => {
                groups.iter().find(|g| g.name == "squad").map(|g| g.id)
            }
            _ => None,
        },
        "the invitation to the group",
    );
    b.engine.request(Request::JoinGroup(b_gid));
    std::thread::sleep(Duration::from_secs(3));
    a.engine.request(Request::Send {
        chat: ChatRef::Group(gid),
        text: "gg".into(),
        reply_to: None,
        mentions: Vec::new(),
    });
    let in_group = b.wait(
        |e| match e {
            ChatEvent::Messages(m) => m
                .iter()
                .find(|m| m.body == MessageBody::Text("gg".into()))
                .cloned(),
            _ => None,
        },
        "the group message",
    );
    assert_eq!(in_group.author, MessageAuthor::Member("alice".into()));

    // The owner gives the group a picture (ADR-0038); the member's list shows it, and the
    // member list shows alice's own picture.
    a.engine.request(Request::SetGroupPicture {
        group: gid,
        picture: Some(pic.clone()),
    });
    let group_pic = (0..30)
        .find_map(|_| {
            b.engine.request(Request::Refresh);
            let got = b.wait(
                |e| match e {
                    ChatEvent::Chats { groups, .. } => Some(
                        groups
                            .iter()
                            .find(|g| g.id == b_gid)
                            .and_then(|g| g.picture.clone()),
                    ),
                    _ => None,
                },
                "bob's list with the group",
            );
            if got.is_none() {
                std::thread::sleep(Duration::from_millis(500));
            }
            got
        })
        .expect("bob sees the group's picture");
    assert_eq!(group_pic.bytes(), pic.bytes());
    b.engine.request(Request::Members(b_gid));
    let alice_in_group = b.wait(
        |e| match e {
            ChatEvent::Members { members, .. } => {
                members.iter().find(|m| m.name == "alice").cloned()
            }
            _ => None,
        },
        "the members",
    );
    assert_eq!(
        alice_in_group.picture.map(|p| p.key()),
        Some(pic.key()),
        "alice's member picture"
    );

    // Stop bob and start him again from the same folder and key: history is there.
    drop(b);
    std::thread::sleep(Duration::from_secs(2));
    let b = Peer::start(&f, b_dir.clone(), "ignored", "bob-key");
    let friend = b.friend();
    b.engine.request(Request::History {
        chat: ChatRef::Direct(friend),
        before: None,
        count: 20,
        tag: 1,
    });
    let history = b.wait(
        |e| match e {
            ChatEvent::History {
                tag: 1, messages, ..
            } => Some(messages.clone()),
            _ => None,
        },
        "history",
    );
    assert!(
        history
            .iter()
            .any(|m| m.body == MessageBody::Text("hello from EasyPlay".into())),
        "{history:#?}"
    );
    // Killed outright (power cut, OOM killer): the history survives that too.
    let killed = kill_sandbox_of(&b_dir);
    assert!(killed > 0, "bob's sandbox was not found");
    b.wait(
        |e| matches!(e, ChatEvent::Exited { .. }).then_some(()),
        "bob's engine to stop",
    );
    drop(b);
    let b = Peer::start(&f, b_dir.clone(), "ignored", "bob-key");
    let friend = b.friend();
    b.engine.request(Request::History {
        chat: ChatRef::Direct(friend),
        before: None,
        count: 20,
        tag: 2,
    });
    let history = b.wait(
        |e| match e {
            ChatEvent::History {
                tag: 2, messages, ..
            } => Some(messages.clone()),
            _ => None,
        },
        "history after a kill",
    );
    assert!(
        history
            .iter()
            .any(|m| m.body == MessageBody::Text("hello from EasyPlay".into()))
    );
    drop(b);

    // The database files are not readable by other users.
    for f in std::fs::read_dir(&b_dir).unwrap() {
        let f = f.unwrap();
        use std::os::unix::fs::PermissionsExt as _;
        let mode = f.metadata().unwrap().permissions().mode() & 0o777;
        assert_eq!(mode & 0o077, 0, "{:?} is {mode:o}", f.path());
    }
}

/// SIGKILL every `bwrap` whose command line names `dir` (the helper dies with it).
/// A real GIF of at most 1 MiB: Giphy's first sendable "wave" result, through Signal's
/// content proxy with the default key (`easyplay_core::gifs`, as the service does).
fn giphy_gif() -> Vec<u8> {
    use easyplay_core::artwork::http::{HttpClient, UreqTransport};
    use easyplay_core::gifs;
    let http = HttpClient::new(
        Arc::new(UreqTransport::new()),
        Arc::new(easyplay_core::clock::SystemClock::new()),
    )
    .for_hosts_via(
        gifs::HOSTS,
        gifs::proxy(gifs::DEFAULT_PROXY).unwrap().as_ref(),
    )
    .unwrap();
    let found = gifs::search(&http, gifs::DEFAULT_KEY, "wave");
    assert_eq!(found.problem, None);
    let gif = found
        .gifs
        .iter()
        .find(|g| g.sendable())
        .expect("a sendable GIF");
    let bytes = gifs::download(&http, &gif.send_url).unwrap();
    assert!(bytes.len() as u64 <= gifs::MAX_SEND);
    eprintln!("giphy: {} ({} bytes)", gif.id, bytes.len());
    bytes
}

fn kill_sandbox_of(dir: &std::path::Path) -> usize {
    let needle = dir.as_os_str().as_encoded_bytes();
    let mut n = 0;
    for e in std::fs::read_dir("/proc").unwrap().flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<i32>().ok()) else {
            continue;
        };
        let Ok(cmd) = std::fs::read(e.path().join("cmdline")) else {
            continue;
        };
        let is_bwrap = cmd
            .split(|b| *b == 0)
            .next()
            .is_some_and(|a| a.ends_with(b"bwrap"));
        if is_bwrap && cmd.windows(needle.len()).any(|w| w == needle) {
            let pid = rustix::process::Pid::from_raw(pid).unwrap();
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
            n += 1;
        }
    }
    n
}
