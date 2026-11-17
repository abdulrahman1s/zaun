//! Friends end to end (ADR-0033): two real chat engines (`easyplay-friends` + libsimplex,
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
//! cargo test -p easyplay-core --test friends_e2e -- --ignored
//! ```

// Real processes against wall time, files in a temporary directory.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use easyplay_core::friends::{
    Engine, EngineConfig, EngineFactory, EventSink, FriendsEvent, Request, SimplexFactory,
};
use easyplay_core::model::{ChatRef, MessageAuthor, MessageBody};

const WAIT: Duration = Duration::from_secs(60);

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set (see the file's header)"))
}

fn factory(log: PathBuf) -> SimplexFactory {
    let helper =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/easyplay-friends");
    assert!(
        helper.is_file(),
        "build it first: cargo build -p easyplay-friends"
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

struct Peer {
    engine: Box<dyn Engine>,
    events: Receiver<FriendsEvent>,
    log: Arc<Mutex<Vec<FriendsEvent>>>,
}

impl Peer {
    fn start(f: &SimplexFactory, dir: PathBuf, name: &str, key: &str) -> Peer {
        let (tx, rx) = channel();
        let tx = Mutex::new(tx);
        let log = Arc::new(Mutex::new(Vec::new()));
        let log2 = Arc::clone(&log);
        let sink: EventSink = Arc::new(move |e: FriendsEvent| {
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
                        relay => vec![relay.to_owned()],
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
        p.wait(
            |e| matches!(e, FriendsEvent::Ready(_)).then_some(()),
            "ready",
        );
        p
    }

    fn wait<T>(&self, mut f: impl FnMut(&FriendsEvent) -> Option<T>, what: &str) -> T {
        let end = std::time::Instant::now() + WAIT;
        loop {
            let left = end.saturating_duration_since(std::time::Instant::now());
            match self.events.recv_timeout(left) {
                Ok(FriendsEvent::Failed {
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

    fn text(&self, wanted: &str) -> ChatRef {
        self.wait(
            |e| match e {
                FriendsEvent::Messages(m) => m
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
                FriendsEvent::Chats { friends, .. } => {
                    friends.iter().find(|f| f.ready).map(|f| f.id)
                }
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
    let f = factory(tmp.path().join("engine.log"));
    let (a_dir, b_dir) = (tmp.path().join("alice"), tmp.path().join("bob"));
    for d in [&a_dir, &b_dir] {
        std::fs::create_dir_all(d).unwrap();
    }
    let a = Peer::start(&f, a_dir.clone(), "alice", "alice-key");
    let b = Peer::start(&f, b_dir.clone(), "bob", "bob-key");

    a.engine.request(Request::CreateInvite);
    let link = a.wait(
        |e| match e {
            FriendsEvent::Invite(l) => Some(l.clone()),
            _ => None,
        },
        "an invitation",
    );
    b.engine.request(Request::Connect(link));
    // Both lists gain the other once the connection is ready.
    let wait_friend = |p: &Peer| {
        for _ in 0..30 {
            if let Some(id) = {
                p.engine.request(Request::Refresh);
                p.wait(
                    |e| match e {
                        FriendsEvent::Chats { friends, .. } => {
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
    });
    assert_eq!(b.text("hello from EasyPlay"), ChatRef::Direct(b_alice));
    b.engine.request(Request::Send {
        chat: ChatRef::Direct(b_alice),
        text: "أهلاً".into(),
    });
    a.text("أهلاً");

    a.engine.request(Request::NewGroup("squad".into()));
    let gid = a.wait(
        |e| match e {
            FriendsEvent::Chats { groups, .. } => {
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
            FriendsEvent::Chats { groups, .. } => {
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
    });
    let in_group = b.wait(
        |e| match e {
            FriendsEvent::Messages(m) => m
                .iter()
                .find(|m| m.body == MessageBody::Text("gg".into()))
                .cloned(),
            _ => None,
        },
        "the group message",
    );
    assert_eq!(in_group.author, MessageAuthor::Member("alice".into()));

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
            FriendsEvent::History {
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
        |e| matches!(e, FriendsEvent::Exited { .. }).then_some(()),
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
            FriendsEvent::History { tag: 2, messages, .. } => Some(messages.clone()),
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
        let is_bwrap = cmd.split(|b| *b == 0).next().is_some_and(|a| a.ends_with(b"bwrap"));
        if is_bwrap && cmd.windows(needle.len()).any(|w| w == needle) {
            let pid = rustix::process::Pid::from_raw(pid).unwrap();
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
            n += 1;
        }
    }
    n
}
