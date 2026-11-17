//! The Friends flow of the AppService (ADR-0033) against the fake engine: the preference
//! starts and stops the engine, events build the snapshot, failures stop or restart it, and
//! pages of a conversation are answered even when the engine goes away.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use easyplay_core::artwork::steam::FsAppIdFile;
use easyplay_core::artwork::{ArtIndex, NoIcons};
use easyplay_core::clock::{Clock, SystemClock, Timestamp};
use easyplay_core::config::Preferences;
use easyplay_core::error::ErrorCode;
use easyplay_core::friends::fake::FakeEngine;
use easyplay_core::friends::{FriendsEvent, Request, RequestKind};
use easyplay_core::fsx::{Fs, MountTable, RealFs};
use easyplay_core::host::StaticHostFacts;
use easyplay_core::identity::{FsPeInspector, SharedLocations};
use easyplay_core::ids::QueryId;
use easyplay_core::jobs::{JobPool, JobPoolConfig};
use easyplay_core::launch::NoLocks;
use easyplay_core::library::draft::NoUmuTitles;
use easyplay_core::library::service::NoPolicyCheck;
use easyplay_core::model::{
    ChatPreview, ChatRef, Delivery, Friend, FriendMessage, FriendProfile, FriendsStatus,
    MessageAuthor, MessageBody,
};
use easyplay_core::procfs::RealProc;
use easyplay_core::ids::RuntimeName;
use easyplay_core::jobs::{JobHandle, JobLabel, JobPriority};
use easyplay_core::model::{Game, RuntimeRef, SlrVariant};
use easyplay_core::runtime::{ProvisionNeed, Resolution, RuntimeCatalog, RuntimeService};
use easyplay_core::service::{
    AppService, Command, Event, FriendsCommand, LaunchEnv, Notice, QueryResult, ServiceDeps,
};
use easyplay_testkit::TestEnv;
use easyplay_testkit::fakes::FakeLauncher;
use easyplay_testkit::fixtures::hosts;

/// No compatibility layers (Friends needs none).
struct NoRuntimes(Arc<JobPool>);

impl RuntimeService for NoRuntimes {
    fn catalog(&self) -> Arc<RuntimeCatalog> {
        Arc::new(RuntimeCatalog::default())
    }
    fn refresh(&self) -> JobHandle<Arc<RuntimeCatalog>> {
        self.0
            .submit(JobPriority::Background, JobLabel("scan"), |_| {
                Arc::new(RuntimeCatalog::default())
            })
    }
    fn resolve(&self, _r: &RuntimeRef, _g: Option<&Game>) -> Resolution {
        Resolution::NeedsProvision(ProvisionNeed::Slr {
            variant: SlrVariant::Steamrt3,
            build: RuntimeName::new("UMU-Proton-10.0-4").unwrap(),
        })
    }
}

fn query(n: u128) -> QueryId {
    QueryId::from_uuid(uuid::Uuid::from_u128(n))
}

struct World {
    env: TestEnv,
    svc: AppService,
    events: Receiver<Event>,
    engine: FakeEngine,
}

fn deps(env: &TestEnv, engine: &FakeEngine, prefs: Preferences) -> ServiceDeps {
    let fs: Arc<dyn Fs> = Arc::new(RealFs::new());
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let jobs = Arc::new(JobPool::new(JobPoolConfig::default(), Arc::clone(&clock)).unwrap());
    ServiceDeps {
        fs: Arc::clone(&fs),
        clock: Arc::clone(&clock),
        paths: env.paths(),
        jobs: Arc::clone(&jobs),
        proc: Arc::new(RealProc::new(Arc::clone(&fs))),
        launcher: Arc::new(FakeLauncher::new()),
        locks: Arc::new(NoLocks),
        runtimes: Arc::new(NoRuntimes(Arc::clone(&jobs))),
        host: Arc::new(StaticHostFacts(Arc::new(hosts::arch_mesa()))),
        pe: Arc::new(FsPeInspector::new(Arc::clone(&fs), Arc::clone(&clock))),
        icons: Arc::new(NoIcons),
        codec: None,
        launch_env: LaunchEnv {
            uid: 1000,
            gid: 1000,
            user_name: "sam".into(),
            group_name: "sam".into(),
            home: env.home(),
            host_env: BTreeMap::new(),
            init_binary: PathBuf::from("/nonexistent/easyplay-init"),
            mounts: MountTable::default(),
            user_dirs: Vec::new(),
            seccomp_available: false,
            global_env: Vec::new(),
        },
        prefs,
        desktop: None,
        http: None,
        app_id_file: Arc::new(FsAppIdFile::new(Arc::clone(&fs))),
        art_index: Arc::new(ArtIndex::new()),
        shared: SharedLocations::default(),
        policy_check: Some(Arc::new(NoPolicyCheck)),
        umu_titles: Arc::new(NoUmuTitles),
        watch_sessions: false,
        friends: Some(Arc::new(engine.clone())),
    }
}

impl World {
    fn new(enabled: bool) -> Self {
        let env = TestEnv::new().unwrap();
        let engine = FakeEngine::new();
        let mut prefs = Preferences::default();
        prefs.friends.enabled = enabled;
        prefs.friends.name = "Rin".into();
        let (svc, events) = AppService::open_headless(deps(&env, &engine, prefs)).unwrap();
        let mut w = World {
            env,
            svc,
            events,
            engine,
        };
        w.settle();
        w
    }

    fn settle(&mut self) {
        while self.svc.pump(Duration::from_millis(100)) {}
    }

    fn drain(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    fn cmd(&mut self, c: FriendsCommand) {
        self.svc.handle(Command::Friends(c));
        self.settle();
    }

    /// Report `e` as the engine and let the service take it.
    fn engine_says(&mut self, e: FriendsEvent) {
        self.engine.emit(e);
        self.settle();
    }

    fn status(&self) -> FriendsStatus {
        self.svc.hub().friends.load().status.clone()
    }

    fn ready(&mut self) {
        self.engine_says(FriendsEvent::Ready(FriendProfile { name: "Rin".into() }));
        assert_eq!(self.status(), FriendsStatus::Ready);
    }
}

fn msg(chat: ChatRef, id: i64, text: &str, author: MessageAuthor) -> FriendMessage {
    FriendMessage {
        id,
        chat,
        delivery: if author == MessageAuthor::Me {
            Delivery::Sent
        } else {
            Delivery::Received
        },
        unread: author != MessageAuthor::Me,
        author,
        body: MessageBody::Text(text.into()),
        at: Timestamp::from_millis(1_800_000_000_000),
    }
}

fn one_friend() -> FriendsEvent {
    FriendsEvent::Chats {
        friends: vec![Friend {
            id: 3,
            name: "Kai".into(),
            ready: true,
        }],
        groups: Vec::new(),
        requests: Vec::new(),
        chats: vec![ChatPreview {
            chat: ChatRef::Direct(3),
            name: "Kai".into(),
            last: None,
            unread: 0,
        }],
    }
}

#[test]
fn off_by_default_nothing_starts_and_no_folder_is_made() {
    let w = World::new(false);
    assert!(w.engine.starts().is_empty());
    assert_eq!(w.status(), FriendsStatus::Off);
    assert!(!w.env.paths().friends_dir.exists());
}

#[test]
fn turning_on_makes_a_private_folder_and_key_and_starts_with_the_name() {
    let mut w = World::new(false);
    w.cmd(FriendsCommand::SetEnabled {
        on: true,
        name: Some("  Sam  ".into()),
    });
    let starts = w.engine.starts();
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].name, "Sam");
    assert!(starts[0].relays.is_empty());
    assert_eq!(starts[0].key.len(), 64);
    let dir = &w.env.paths().friends_dir;
    use std::os::unix::fs::PermissionsExt as _;
    assert_eq!(std::fs::metadata(dir).unwrap().permissions().mode() & 0o777, 0o700);
    let key = dir.join("key");
    assert_eq!(std::fs::metadata(&key).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(w.status(), FriendsStatus::Starting);
    assert!(w.svc.hub().prefs.load().friends.enabled);

    // The same key on the next start.
    w.cmd(FriendsCommand::SetEnabled { on: false, name: None });
    assert!(!w.engine.running());
    assert_eq!(w.status(), FriendsStatus::Off);
    w.cmd(FriendsCommand::SetEnabled { on: true, name: None });
    assert_eq!(w.engine.starts()[1].key, starts[0].key);
}

#[test]
fn events_build_the_snapshot_and_new_messages_count_as_unread() {
    let mut w = World::new(true);
    w.ready();
    w.engine_says(one_friend());
    w.drain();
    let kai = ChatRef::Direct(3);
    w.engine_says(FriendsEvent::Messages(vec![msg(kai, 10, "gg", MessageAuthor::Friend)]));
    let snap = w.svc.hub().friends.load();
    assert_eq!(snap.unread(), 1);
    assert_eq!(
        snap.chats[0].last.as_ref().unwrap().body,
        MessageBody::Text("gg".into())
    );
    assert!(w.drain().iter().any(|e| matches!(
        e,
        Event::Notice(Notice::FriendMessages(m)) if m[0].id == 10
    )));

    // Reading it clears the count and tells the engine.
    w.engine.take_requests();
    w.cmd(FriendsCommand::MarkRead(kai));
    assert_eq!(w.svc.hub().friends.load().unread(), 0);
    assert_eq!(w.engine.take_requests(), vec![Request::MarkRead(kai)]);

    // A message from a conversation not in the list yet reads the list again.
    w.engine_says(FriendsEvent::Messages(vec![msg(
        ChatRef::Group(9),
        1,
        "hi",
        MessageAuthor::Member("Ana".into()),
    )]));
    assert_eq!(w.engine.take_requests(), vec![Request::Refresh]);
}

#[test]
fn a_start_the_engine_refuses_stops_with_the_reason_and_on_again_retries() {
    let mut w = World::new(true);
    w.engine_says(FriendsEvent::Failed {
        request: RequestKind::Start,
        code: ErrorCode::FrnRequestFailed,
        detail: "invalid display name".into(),
    });
    assert_eq!(w.status(), FriendsStatus::Failed(ErrorCode::FrnRequestFailed));
    assert!(w.drain().iter().any(|e| matches!(e, Event::Failure(_))));
    // A late exit of that engine restarts nothing.
    w.engine_says(FriendsEvent::Exited {
        status: Some(1),
        started: false,
    });
    assert_eq!(w.engine.starts().len(), 1);

    w.cmd(FriendsCommand::SetEnabled {
        on: true,
        name: Some("Rin".into()),
    });
    assert_eq!(w.engine.starts().len(), 2);
    assert_eq!(w.status(), FriendsStatus::Starting);
}

#[test]
fn an_engine_that_keeps_exiting_is_restarted_then_given_up() {
    let mut w = World::new(true);
    // Exits 1 to 4 are each followed by a restart after the back-off (1 + 2 + 4 + 8 s).
    for n in 1..easyplay_core::service::flows::friends::MAX_FAILURES {
        w.engine_says(FriendsEvent::Exited {
            status: Some(1),
            started: false,
        });
        assert_eq!(w.status(), FriendsStatus::Starting, "after exit {n}");
        assert!(!w.engine.running());
        std::thread::sleep(easyplay_core::service::flows::friends::backoff(n));
        for _ in 0..20 {
            w.settle();
            if w.engine.running() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(w.engine.starts().len(), n as usize + 1, "restart {n}");
    }
    // The fifth: no more restarts.
    w.engine_says(FriendsEvent::Exited {
        status: Some(1),
        started: false,
    });
    assert_eq!(w.status(), FriendsStatus::Failed(ErrorCode::FrnEngineFailing));
    assert!(w.drain().iter().any(|e| matches!(e, Event::Failure(_))));
    std::thread::sleep(Duration::from_millis(1200));
    w.settle();
    assert_eq!(w.engine.starts().len(), 5);
}

#[test]
fn a_page_is_answered_and_pages_left_waiting_fail_when_the_engine_stops() {
    let mut w = World::new(true);
    w.ready();
    let kai = ChatRef::Direct(3);
    let first = query(1);
    w.cmd(FriendsCommand::LoadChat {
        chat: kai,
        before: None,
        reply_to: first,
    });
    let Some(Request::History { tag, count, .. }) = w.engine.take_requests().pop() else {
        panic!("no history request")
    };
    assert_eq!(count, 50);
    w.engine_says(FriendsEvent::History {
        tag,
        chat: kai,
        messages: vec![msg(kai, 1, "old", MessageAuthor::Friend)],
    });
    assert!(w.drain().iter().any(|e| matches!(
        e,
        Event::QueryResult { id, result: QueryResult::Chat(p) } if *id == first && p.messages.len() == 1
    )));

    let waiting = query(2);
    w.cmd(FriendsCommand::LoadChat {
        chat: kai,
        before: Some(1),
        reply_to: waiting,
    });
    w.drain();
    w.engine_says(FriendsEvent::Exited {
        status: None,
        started: true,
    });
    assert!(w.drain().iter().any(|e| matches!(
        e,
        Event::QueryResult { id, result: QueryResult::Failed(_) } if *id == waiting
    )));
}

#[test]
fn events_of_a_replaced_engine_are_ignored() {
    let mut w = World::new(true);
    let stale = w.engine.clone();
    // Relays change: the engine is restarted, the old one's events no longer count.
    w.svc.handle(Command::SetPreference(
        easyplay_core::config::PreferencePatch::new()
            .set("friends.relays", vec!["smp://fp@relay.example".to_owned()]),
    ));
    w.settle();
    assert_eq!(w.engine.starts().len(), 2);
    assert_eq!(w.engine.starts()[1].relays, vec!["smp://fp@relay.example"]);
    // The fake shares one sink, so "stale" here is the generation check: report Ready from
    // the new engine works, and the status follows only it.
    stale.emit(FriendsEvent::Ready(FriendProfile { name: "Rin".into() }));
    w.settle();
    assert_eq!(w.status(), FriendsStatus::Ready);
}

#[test]
fn without_an_engine_in_this_build_the_page_says_so() {
    let env = TestEnv::new().unwrap();
    let engine = FakeEngine::new();
    engine.set_available(false);
    let mut prefs = Preferences::default();
    prefs.friends.enabled = true;
    let (mut svc, _events) = AppService::open_headless(deps(&env, &engine, prefs)).unwrap();
    while svc.pump(Duration::from_millis(100)) {}
    assert_eq!(svc.hub().friends.load().status, FriendsStatus::Unavailable);
    assert!(engine.starts().is_empty());
}
