//! The Chat flow of the AppService (ADR-0033) against the fake engine: the preference
//! starts and stops the engine, events build the snapshot, failures stop or restart it, and
//! pages of a conversation are answered even when the engine goes away.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use zaun_core::artwork::steam::FsAppIdFile;
use zaun_core::artwork::{ArtIndex, NoIcons};
use zaun_core::chat::fake::FakeEngine;
use zaun_core::chat::{ChatEvent, FileKind, Request, RequestKind};
use zaun_core::clock::{Clock, SystemClock, Timestamp};
use zaun_core::config::Preferences;
use zaun_core::error::ErrorCode;
use zaun_core::fsx::{Fs, MountTable, RealFs};
use zaun_core::host::StaticHostFacts;
use zaun_core::identity::{FsPeInspector, SharedLocations};
use zaun_core::ids::QueryId;
use zaun_core::ids::RuntimeName;
use zaun_core::jobs::{JobHandle, JobLabel, JobPriority};
use zaun_core::jobs::{JobPool, JobPoolConfig};
use zaun_core::launch::NoLocks;
use zaun_core::library::draft::NoUmuTitles;
use zaun_core::library::service::NoPolicyCheck;
use zaun_core::model::{
    ChatFeatures, ChatGroup, ChatMessage, ChatPreview, ChatProfile, ChatRef, ChatStatus, Delivery,
    Friend, GroupMember, MessageAuthor, MessageBody,
};
use zaun_core::model::{Game, RuntimeRef, SlrVariant};
use zaun_core::procfs::RealProc;
use zaun_core::runtime::{ProvisionNeed, Resolution, RuntimeCatalog, RuntimeService};
use zaun_core::service::{
    AppService, ChatCommand, ClipKind, Command, Event, LaunchEnv, Notice, QueryResult, ServiceDeps,
};
use zaun_testkit::TestEnv;
use zaun_testkit::fakes::FakeLauncher;
use zaun_testkit::fixtures::hosts;

/// No compatibility layers (Chat needs none).
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
            init_binary: PathBuf::from("/nonexistent/zaun-init"),
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
        chat: Some(Arc::new(engine.clone())),
        overlay: None,
        media: None,
        updates: None,
    }
}

impl World {
    fn new(enabled: bool) -> Self {
        World::in_env(TestEnv::new().unwrap(), enabled)
    }

    fn in_env(env: TestEnv, enabled: bool) -> Self {
        let engine = FakeEngine::new();
        let mut prefs = Preferences::default();
        prefs.chat.enabled = enabled;
        prefs.chat.name = "Rin".into();
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

    fn cmd(&mut self, c: ChatCommand) {
        self.svc.handle(Command::Chat(c));
        self.settle();
    }

    /// Report `e` as the engine and let the service take it.
    fn engine_says(&mut self, e: ChatEvent) {
        self.engine.emit(e);
        self.settle();
    }

    fn status(&self) -> ChatStatus {
        self.svc.hub().chat.load().status.clone()
    }

    /// The next request to the engine, waiting for a file prepared on its own thread.
    fn next_request(&mut self) -> Request {
        for _ in 0..200 {
            self.settle();
            let mut r = self.engine.take_requests();
            if !r.is_empty() {
                return r.remove(0);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("no request")
    }

    fn ready(&mut self) {
        self.engine_says(ChatEvent::Ready(ChatProfile {
            name: "Rin".into(),
            ..ChatProfile::default()
        }));
        assert_eq!(self.status(), ChatStatus::Ready);
    }
}

fn msg(chat: ChatRef, id: i64, text: &str, author: MessageAuthor) -> ChatMessage {
    ChatMessage {
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
        reply: None,
        reactions: Vec::new(),
        edited: false,
        mentions: Vec::new(),
    }
}

fn one_friend() -> ChatEvent {
    ChatEvent::Chats {
        friends: vec![Friend {
            id: 3,
            name: "Kai".into(),
            ready: true,
            ..Friend::default()
        }],
        groups: Vec::new(),
        requests: Vec::new(),
        chats: vec![ChatPreview {
            chat: ChatRef::Direct(3),
            name: "Kai".into(),
            last: None,
            unread: 0,
            muted: false,
        }],
    }
}

#[test]
fn off_by_default_nothing_starts_and_no_folder_is_made() {
    let w = World::new(false);
    assert!(w.engine.starts().is_empty());
    assert_eq!(w.status(), ChatStatus::Off);
    assert!(!w.env.paths().chat_dir.exists());
}

/// Settles until the snapshot's `old_data` is `want` (the measuring runs on the job pool).
fn old_data_becomes(w: &mut World, want: Option<u64>) {
    for _ in 0..100 {
        w.settle();
        if w.svc.hub().chat.load().old_data == want {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("old_data is {:?}", w.svc.hub().chat.load().old_data);
}

#[test]
fn the_old_engines_databases_are_offered_for_deletion_and_nothing_else_goes() {
    let env = TestEnv::new().unwrap();
    let dir = env.paths().chat_dir;
    std::fs::create_dir_all(dir.join("files")).unwrap();
    std::fs::create_dir_all(dir.join("tmp")).unwrap();
    std::fs::write(dir.join("simplex_chat.db"), [0u8; 100]).unwrap();
    std::fs::write(dir.join("simplex_agent.db-wal"), [0u8; 20]).unwrap();
    for keep in ["chat.db", "key", "files/photo.png"] {
        std::fs::write(dir.join(keep), b"x").unwrap();
    }
    let mut w = World::in_env(env, false);
    old_data_becomes(&mut w, Some(120));
    w.cmd(ChatCommand::ForgetOldData);
    old_data_becomes(&mut w, None);
    assert!(!dir.join("simplex_chat.db").exists());
    assert!(!dir.join("tmp").exists(), "an empty scratch folder goes");
    for keep in ["chat.db", "key", "files/photo.png"] {
        assert!(dir.join(keep).exists(), "{keep} stays");
    }
}

#[test]
fn without_old_data_there_is_nothing_to_offer() {
    let mut w = World::new(true);
    old_data_becomes(&mut w, None);
    w.ready();
    assert_eq!(w.svc.hub().chat.load().old_data, None);
}

#[test]
fn turning_on_makes_a_private_folder_and_key_and_starts_with_the_name() {
    let mut w = World::new(false);
    w.cmd(ChatCommand::SetEnabled {
        on: true,
        name: Some("  Sam  ".into()),
    });
    let starts = w.engine.starts();
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].name, "Sam");
    assert!(starts[0].relays.is_empty());
    assert_eq!(starts[0].key.len(), 64);
    let dir = &w.env.paths().chat_dir;
    use std::os::unix::fs::PermissionsExt as _;
    assert_eq!(
        std::fs::metadata(dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let key = dir.join("key");
    assert_eq!(
        std::fs::metadata(&key).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(w.status(), ChatStatus::Starting);
    assert!(w.svc.hub().prefs.load().chat.enabled);

    // The same key on the next start.
    w.cmd(ChatCommand::SetEnabled {
        on: false,
        name: None,
    });
    assert!(!w.engine.running());
    assert_eq!(w.status(), ChatStatus::Off);
    w.cmd(ChatCommand::SetEnabled {
        on: true,
        name: None,
    });
    assert_eq!(w.engine.starts()[1].key, starts[0].key);
}

#[test]
fn events_build_the_snapshot_and_new_messages_count_as_unread() {
    let mut w = World::new(true);
    w.ready();
    w.engine_says(one_friend());
    w.drain();
    let kai = ChatRef::Direct(3);
    w.engine_says(ChatEvent::Messages(vec![msg(
        kai,
        10,
        "gg",
        MessageAuthor::Friend,
    )]));
    let snap = w.svc.hub().chat.load();
    assert_eq!(snap.unread(), 1);
    assert_eq!(
        snap.chats[0].last.as_ref().unwrap().body,
        MessageBody::Text("gg".into())
    );
    assert!(w.drain().iter().any(|e| matches!(
        e,
        Event::Notice(Notice::ChatMessages(m)) if m[0].id == 10
    )));

    // Reading it clears the count and tells the engine.
    w.engine.take_requests();
    w.cmd(ChatCommand::MarkRead(kai));
    assert_eq!(w.svc.hub().chat.load().unread(), 0);
    assert_eq!(w.engine.take_requests(), vec![Request::MarkRead(kai)]);

    // A message from a conversation not in the list yet reads the list again.
    w.engine_says(ChatEvent::Messages(vec![msg(
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
    w.engine_says(ChatEvent::Failed {
        request: RequestKind::Start,
        code: ErrorCode::ChtRequestFailed,
        detail: "invalid display name".into(),
    });
    assert_eq!(w.status(), ChatStatus::Failed(ErrorCode::ChtRequestFailed));
    // The Chat page says why; no dialog on top of it.
    assert!(!w.drain().iter().any(|e| matches!(e, Event::Failure(_))));
    // A late exit of that engine restarts nothing.
    w.engine_says(ChatEvent::Exited {
        status: Some(1),
        started: false,
    });
    assert_eq!(w.engine.starts().len(), 1);

    w.cmd(ChatCommand::SetEnabled {
        on: true,
        name: Some("Rin".into()),
    });
    assert_eq!(w.engine.starts().len(), 2);
    assert_eq!(w.status(), ChatStatus::Starting);
}

#[test]
fn an_engine_that_keeps_exiting_is_restarted_then_given_up() {
    let mut w = World::new(true);
    // Exits 1 to 4 are each followed by a restart after the back-off (1, 2, 4, 8 s; the fake
    // engine records each and waits a hundredth of it, so the restart may already be done).
    for n in 1..zaun_core::service::flows::chat::MAX_FAILURES {
        w.engine_says(ChatEvent::Exited {
            status: Some(1),
            started: false,
        });
        assert_eq!(w.status(), ChatStatus::Starting, "after exit {n}");
        assert_eq!(
            w.engine.restart_waits().last().copied(),
            Some(zaun_core::service::flows::chat::backoff(n)),
            "back-off after exit {n}"
        );
        for _ in 0..40 {
            w.settle();
            if w.engine.running() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(w.engine.starts().len(), n as usize + 1, "restart {n}");
    }
    // The fifth: no more restarts.
    w.engine_says(ChatEvent::Exited {
        status: Some(1),
        started: false,
    });
    assert_eq!(w.status(), ChatStatus::Failed(ErrorCode::ChtEngineFailing));
    assert!(w.drain().iter().any(|e| matches!(e, Event::Failure(_))));
    // Longer than the next (scaled) back-off would be: still no restart.
    std::thread::sleep(Duration::from_millis(400));
    w.settle();
    assert_eq!(w.engine.starts().len(), 5);
    let secs: Vec<u64> = w
        .engine
        .restart_waits()
        .iter()
        .map(Duration::as_secs)
        .collect();
    assert_eq!(secs, [1, 2, 4, 8]);
}

#[test]
fn a_page_is_answered_and_pages_left_waiting_fail_when_the_engine_stops() {
    let mut w = World::new(true);
    w.ready();
    let kai = ChatRef::Direct(3);
    let first = query(1);
    w.cmd(ChatCommand::LoadChat {
        chat: kai,
        before: None,
        reply_to: first,
    });
    let Some(Request::History { tag, count, .. }) = w.engine.take_requests().pop() else {
        panic!("no history request")
    };
    assert_eq!(count, 50);
    w.engine_says(ChatEvent::History {
        tag,
        chat: kai,
        messages: vec![msg(kai, 1, "old", MessageAuthor::Friend)],
    });
    assert!(w.drain().iter().any(|e| matches!(
        e,
        Event::QueryResult { id, result: QueryResult::Chat(p) } if *id == first && p.messages.len() == 1
    )));

    let waiting = query(2);
    w.cmd(ChatCommand::LoadChat {
        chat: kai,
        before: Some(1),
        reply_to: waiting,
    });
    w.drain();
    w.engine_says(ChatEvent::Exited {
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
        zaun_core::config::PreferencePatch::new()
            .set("chat.relays", vec!["smp://fp@relay.example".to_owned()]),
    ));
    w.settle();
    assert_eq!(w.engine.starts().len(), 2);
    assert_eq!(w.engine.starts()[1].relays, vec!["smp://fp@relay.example"]);
    // The fake shares one sink, so "stale" here is the generation check: report Ready from
    // the new engine works, and the status follows only it.
    stale.emit(ChatEvent::Ready(ChatProfile {
        name: "Rin".into(),
        ..ChatProfile::default()
    }));
    w.settle();
    assert_eq!(w.status(), ChatStatus::Ready);
}

#[test]
fn without_an_engine_in_this_build_the_page_says_so() {
    let env = TestEnv::new().unwrap();
    let engine = FakeEngine::new();
    engine.set_available(false);
    let mut prefs = Preferences::default();
    prefs.chat.enabled = true;
    let (mut svc, _events) = AppService::open_headless(deps(&env, &engine, prefs)).unwrap();
    while svc.pump(Duration::from_millis(100)) {}
    assert_eq!(svc.hub().chat.load().status, ChatStatus::Unavailable);
    assert!(engine.starts().is_empty());
}

// ---- chat model v2: replies, edits, reactions, mute, block, codes, profile, files ------

/// Ready with every feature the real engine carries, and one friend.
fn ready_with_kai(w: &mut World) {
    w.engine_says(ChatEvent::Features {
        features: ChatFeatures {
            files: true,
            reactions: true,
            replies: true,
            edit: true,
            mute: true,
            block: true,
            verify: true,
            mentions: true,
            ..ChatFeatures::default()
        },
        reactions: Some(vec!["👍".into()]),
    });
    w.ready();
    w.engine_says(one_friend());
    w.engine.take_requests();
}

#[test]
fn features_and_the_engines_reactions_reach_the_snapshot() {
    let mut w = World::new(true);
    ready_with_kai(&mut w);
    let snap = w.svc.hub().chat.load();
    assert!(snap.features.reactions && snap.features.files);
    assert!(!snap.features.calls && !snap.features.gifs);
    assert_eq!(snap.reaction_emoji, Some(vec!["👍".to_owned()]));
}

#[test]
fn replies_mentions_edits_deletions_and_reactions_become_requests() {
    let mut w = World::new(true);
    ready_with_kai(&mut w);
    let kai = ChatRef::Direct(3);
    let crew = ChatRef::Group(9);
    w.cmd(ChatCommand::Send {
        chat: crew,
        text: "@Ana go".into(),
        reply_to: Some(4),
        mentions: vec![("Ana".into(), 2)],
    });
    w.cmd(ChatCommand::Edit {
        chat: kai,
        item: 5,
        text: "fixed".into(),
    });
    w.cmd(ChatCommand::Edit {
        chat: kai,
        item: 5,
        text: "  ".into(),
    });
    w.cmd(ChatCommand::Delete { chat: kai, item: 5 });
    w.cmd(ChatCommand::React {
        chat: kai,
        item: 6,
        emoji: "👍".into(),
        on: true,
    });
    assert_eq!(
        w.engine.take_requests(),
        vec![
            Request::Send {
                chat: crew,
                text: "@Ana go".into(),
                reply_to: Some(4),
                mentions: vec![("Ana".into(), 2)],
            },
            Request::Edit {
                chat: kai,
                item: 5,
                text: "fixed".into(),
            },
            Request::Delete { chat: kai, item: 5 },
            Request::React {
                chat: kai,
                item: 6,
                emoji: "👍".into(),
                on: true,
            },
        ]
    );
}

#[test]
fn muting_and_blocking_show_at_once_and_reach_the_engine() {
    let mut w = World::new(true);
    ready_with_kai(&mut w);
    let kai = ChatRef::Direct(3);
    w.engine_says(ChatEvent::Messages(vec![msg(
        kai,
        10,
        "gg",
        MessageAuthor::Friend,
    )]));
    assert_eq!(w.svc.hub().chat.load().unread(), 1);
    w.cmd(ChatCommand::SetMuted {
        chat: kai,
        on: true,
    });
    let snap = w.svc.hub().chat.load();
    assert!(snap.chats[0].muted && snap.friend(3).unwrap().muted);
    assert_eq!(snap.unread(), 0, "a muted chat does not count");
    w.cmd(ChatCommand::SetBlocked {
        friend: 3,
        on: true,
    });
    let snap = w.svc.hub().chat.load();
    assert!(snap.friend(3).unwrap().blocked);
    assert!(
        snap.chats.is_empty(),
        "a blocked friend's chat leaves the list"
    );
    assert_eq!(
        w.engine.take_requests(),
        vec![
            Request::SetMuted {
                chat: kai,
                on: true
            },
            Request::SetBlocked {
                friend: 3,
                on: true
            },
        ]
    );
}

#[test]
fn members_are_read_when_a_group_opens_and_survive_a_new_list() {
    let mut w = World::new(true);
    ready_with_kai(&mut w);
    let mut list = one_friend();
    if let ChatEvent::Chats { groups, .. } = &mut list {
        groups.push(ChatGroup {
            id: 9,
            name: "Crew".into(),
            ..ChatGroup::default()
        });
    }
    w.engine_says(list.clone());
    w.cmd(ChatCommand::LoadChat {
        chat: ChatRef::Group(9),
        before: None,
        reply_to: query(7),
    });
    let r = w.engine.take_requests();
    assert!(r.contains(&Request::Members(9)), "{r:?}");
    let members = vec![
        GroupMember {
            id: 1,
            name: "Rin".into(),
            me: true,
            owner: true,
            ..GroupMember::default()
        },
        GroupMember {
            id: 2,
            name: "Ana".into(),
            ..GroupMember::default()
        },
    ];
    w.engine_says(ChatEvent::Members {
        group: 9,
        members: members.clone(),
    });
    w.engine_says(list);
    let snap = w.svc.hub().chat.load();
    assert_eq!(snap.group(9).unwrap().member_list, members);

    w.cmd(ChatCommand::SetMemberBlocked {
        group: 9,
        member: 2,
        on: true,
    });
    assert!(w.svc.hub().chat.load().group(9).unwrap().member_list[1].blocked);
    assert_eq!(
        w.engine.take_requests(),
        vec![Request::SetMemberBlocked {
            group: 9,
            member: 2,
            on: true
        }]
    );
}

#[test]
fn a_security_code_is_shown_and_a_mismatch_is_said() {
    let mut w = World::new(true);
    ready_with_kai(&mut w);
    w.cmd(ChatCommand::ShowCode(3));
    assert_eq!(w.engine.take_requests(), vec![Request::ShowCode(3)]);
    w.engine_says(ChatEvent::Code {
        friend: 3,
        code: "12345 67890".into(),
    });
    assert_eq!(
        w.svc.hub().chat.load().code,
        Some((3, "12345 67890".to_owned()))
    );
    w.cmd(ChatCommand::Verify {
        friend: 3,
        code: "12345 67890".into(),
    });
    assert_eq!(
        w.engine.take_requests(),
        vec![Request::Verify {
            friend: 3,
            code: "12345 67890".into()
        }]
    );
    w.engine_says(ChatEvent::Verified {
        friend: 3,
        matched: true,
    });
    assert!(w.svc.hub().chat.load().friend(3).unwrap().verified);
    w.drain();
    w.engine_says(ChatEvent::Verified {
        friend: 3,
        matched: false,
    });
    assert!(!w.svc.hub().chat.load().friend(3).unwrap().verified);
    assert!(w.drain().iter().any(|e| matches!(e, Event::Failure(_))));
}

#[test]
fn the_profile_goes_to_the_engine_and_the_avatar_pair_stays_here() {
    let mut w = World::new(true);
    ready_with_kai(&mut w);
    w.cmd(ChatCommand::SetProfile(ChatProfile {
        name: "Rin K".into(),
        about: "Co-op only.".into(),
        paper: Some(4),
        ..ChatProfile::default()
    }));
    assert_eq!(
        w.engine.take_requests(),
        vec![Request::SetProfile(ChatProfile {
            name: "Rin K".into(),
            about: "Co-op only.".into(),
            paper: Some(4),
            ..ChatProfile::default()
        })]
    );
    assert_eq!(w.svc.hub().prefs.load().chat.name, "Rin K");
    let paper = w.env.paths().chat_dir.join("paper");
    assert_eq!(std::fs::read_to_string(&paper).unwrap(), "4");
    // The engine's answer keeps the pair.
    w.engine_says(ChatEvent::Profile(ChatProfile {
        name: "Rin K".into(),
        about: "Co-op only.".into(),
        ..ChatProfile::default()
    }));
    assert_eq!(
        w.svc.hub().chat.load().profile.as_ref().unwrap().paper,
        Some(4)
    );

    // A new name alone keeps the line about the user; the pair alone asks the engine nothing.
    w.cmd(ChatCommand::SetName("Rin".into()));
    let Some(Request::SetProfile(p)) = w.engine.take_requests().pop() else {
        panic!("no profile request")
    };
    assert_eq!((p.name.as_str(), p.about.as_str()), ("Rin", "Co-op only."));
    w.engine_says(ChatEvent::Profile(p.clone()));
    w.cmd(ChatCommand::SetProfile(ChatProfile {
        paper: Some(7),
        ..p
    }));
    assert!(w.engine.take_requests().is_empty());
    assert_eq!(std::fs::read_to_string(&paper).unwrap(), "7");
}

#[test]
fn a_picture_is_copied_into_the_chat_folder_before_it_is_sent() {
    let mut w = World::new(true);
    ready_with_kai(&mut w);
    let src = w.env.home().join("party.GIF");
    std::fs::write(&src, b"GIF89a").unwrap();
    w.cmd(ChatCommand::SendFile {
        chat: ChatRef::Direct(3),
        path: src.clone(),
        text: String::new(),
    });
    let Request::SendFile {
        chat, path, kind, ..
    } = w.next_request()
    else {
        panic!("no file request")
    };
    assert_eq!((chat, kind), (ChatRef::Direct(3), FileKind::Gif));
    assert!(path.starts_with(&w.env.paths().chat_dir), "{path:?}");
    // Its name is what the friend sees.
    assert_eq!(path.file_name().unwrap(), "party.GIF");
    assert_eq!(std::fs::read(&path).unwrap(), b"GIF89a");

    // A 3 MB photo goes too, with a small preview made from it.
    let photo = w.env.home().join("IMG_1.png");
    let img = image::RgbImage::from_fn(1200, 900, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, 90])
    });
    img.save(&photo).unwrap();
    w.cmd(ChatCommand::SendFile {
        chat: ChatRef::Direct(3),
        path: photo,
        text: String::new(),
    });
    let Request::SendFile { kind, preview, .. } = w.next_request() else {
        panic!("no file request")
    };
    assert_eq!(kind, FileKind::Image);
    let preview = preview.expect("a preview");
    assert!(preview.starts_with(&[0xFF, 0xD8]));
    // As large as fits the engine's message: within the budget, at least 160 px across.
    use zaun_core::service::flows::chat::{PREVIEW_BUDGET, data_uri_len, preview_jpeg};
    assert!(
        data_uri_len(preview.len()) <= PREVIEW_BUDGET,
        "{}",
        preview.len()
    );
    let shown = image::load_from_memory(&preview).unwrap();
    assert!(
        shown.width().max(shown.height()) >= 160,
        "{}x{}",
        shown.width(),
        shown.height()
    );
    assert_eq!(shown.width() * 3, shown.height() * 4, "the aspect is kept");

    // A noisy 12 MP photo still fits, smaller; a caption leaves it less room.
    let mut seed = 0x9e37_79b9_u32;
    let noisy = image::RgbImage::from_fn(4000, 3000, |_, _| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let [r, g, b, _] = seed.to_le_bytes();
        image::Rgb([r, g, b])
    });
    let mut png = Vec::new();
    noisy
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let full = preview_jpeg(&png, PREVIEW_BUDGET).expect("fits");
    assert!(data_uri_len(full.len()) <= PREVIEW_BUDGET);
    let captioned = preview_jpeg(&png, PREVIEW_BUDGET - 6000).expect("fits");
    assert!(data_uri_len(captioned.len()) <= PREVIEW_BUDGET - 6000);

    // The caption goes with the file.
    w.cmd(ChatCommand::SendFile {
        chat: ChatRef::Direct(3),
        path: src.clone(),
        text: "gg".into(),
    });
    let Request::SendFile { text, .. } = w.next_request() else {
        panic!("no file request")
    };
    assert_eq!(text, "gg");

    // A picture pasted from the clipboard is written into the Chat folder first.
    w.cmd(ChatCommand::SendClipboard {
        chat: ChatRef::Direct(3),
        bytes: b"GIF89a-pasted".to_vec(),
        kind: ClipKind::Gif,
        text: String::new(),
    });
    let Request::SendFile { path, kind, .. } = w.next_request() else {
        panic!("no file request")
    };
    assert_eq!(kind, FileKind::Gif);
    assert_eq!(std::fs::read(&path).unwrap(), b"GIF89a-pasted");

    // Over the 1 GiB the file servers take: refused (a sparse file, nothing written).
    let big = w.env.home().join("huge.mkv");
    std::fs::File::create(&big)
        .unwrap()
        .set_len(zaun_core::chat::MAX_SEND + 1)
        .unwrap();
    w.drain();
    w.cmd(ChatCommand::SendFile {
        chat: ChatRef::Direct(3),
        path: big,
        text: String::new(),
    });
    assert!(w.engine.take_requests().is_empty());
    assert!(w.drain().iter().any(|e| matches!(e, Event::Failure(_))));
}

#[test]
fn files_are_fetched_cancelled_and_opened_through_the_engine_and_group_links_kept() {
    let mut w = World::new(true);
    ready_with_kai(&mut w);
    let kai = ChatRef::Direct(3);
    w.cmd(ChatCommand::ReceiveFile { chat: kai, item: 7 });
    w.cmd(ChatCommand::CancelFile { chat: kai, item: 7 });
    w.cmd(ChatCommand::OpenFile { chat: kai, item: 7 });
    w.cmd(ChatCommand::CreateGroupLink(9));
    w.cmd(ChatCommand::DeleteGroupLink(9));
    assert_eq!(
        w.engine.take_requests(),
        vec![
            Request::ReceiveFile { chat: kai, item: 7 },
            Request::CancelFile { chat: kai, item: 7 },
            Request::OpenFile { chat: kai, item: 7 },
            Request::CreateGroupLink(9),
            Request::DeleteGroupLink(9),
        ]
    );
    let mut list = one_friend();
    if let ChatEvent::Chats { groups, .. } = &mut list {
        groups.push(ChatGroup {
            id: 9,
            name: "Crew".into(),
            ..ChatGroup::default()
        });
    }
    w.engine_says(list.clone());
    w.engine_says(ChatEvent::GroupLink {
        group: 9,
        link: Some("https://relay.example/g#x".into()),
    });
    w.engine_says(list);
    assert_eq!(
        w.svc.hub().chat.load().group(9).unwrap().link.as_deref(),
        Some("https://relay.example/g#x")
    );
    // A file outside the Chat folder is never opened (no desktop in this world anyway:
    // nothing happens, nothing fails).
    w.engine_says(ChatEvent::OpenFile {
        path: "/etc/passwd".into(),
        kind: FileKind::File,
    });
}

#[test]
fn files_off_refuses_a_picture_and_offline_reaches_the_snapshot() {
    let mut w = World::new(true);
    w.ready(); // no Features event: everything off
    let src = w.env.home().join("a.png");
    std::fs::write(&src, b"png").unwrap();
    w.cmd(ChatCommand::SendFile {
        chat: ChatRef::Direct(3),
        path: src,
        text: String::new(),
    });
    assert!(w.engine.take_requests().is_empty());
    w.engine_says(ChatEvent::Offline(true));
    assert!(w.svc.hub().chat.load().offline);
    w.engine_says(ChatEvent::Offline(false));
    assert!(!w.svc.hub().chat.load().offline);
    // An engine that restarts while offline starts out online again.
    w.engine_says(ChatEvent::Offline(true));
    w.ready();
    assert!(!w.svc.hub().chat.load().offline);
}

/// `SendGif` (services): a Giphy GIF is downloaded through the relay (only that one
/// address, the default proxy), handed to `SendFile` (copied into the Chat folder and sent
/// as a GIF), and the download is removed. A GIF that is not on Giphy's hosts is refused.
#[test]
fn a_giphy_gif_goes_through_the_relay_and_is_sent_as_a_file() {
    use std::sync::Mutex;
    use zaun_core::artwork::http::{
        HopLimits, HttpClient, HttpError, HttpsUrl, ProxyAddr, RawResponse, Transport,
    };
    use zaun_core::gifs::{Gif, GifPick};

    const GIF: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x01D\x00;";

    #[derive(Default)]
    struct Net {
        asked: Arc<Mutex<Vec<String>>>,
        via: Option<String>,
    }
    impl Transport for Net {
        fn get(&self, url: &HttpsUrl, _: HopLimits) -> Result<RawResponse, HttpError> {
            let via = self.via.as_deref().unwrap_or("direct");
            self.asked.lock().unwrap().push(format!("{via} {url}"));
            Ok(RawResponse {
                status: 200,
                location: None,
                body: GIF.to_vec(),
            })
        }
        fn via(&self, proxy: &ProxyAddr) -> Result<Arc<dyn Transport>, HttpError> {
            Ok(Arc::new(Net {
                asked: Arc::clone(&self.asked),
                via: Some(proxy.to_string()),
            }))
        }
    }

    let env = TestEnv::new().unwrap();
    let engine = FakeEngine::new();
    let mut prefs = Preferences::default();
    prefs.chat.enabled = true;
    prefs.chat.name = "Rin".into();
    let net = Arc::new(Net::default());
    let asked = Arc::clone(&net.asked);
    let mut d = deps(&env, &engine, prefs);
    d.http = Some(Arc::new(HttpClient::new(net, Arc::new(SystemClock::new()))));
    let (svc, events) = AppService::open_headless(d).unwrap();
    let mut w = World {
        env,
        svc,
        events,
        engine,
    };
    w.settle();
    ready_with_kai(&mut w);

    let gif = |url: &str| Gif {
        id: "wave1".into(),
        title: "Wave".into(),
        preview_url: "https://media2.giphy.com/media/wave1/200w_d.webp".into(),
        size: [200, 200],
        send_url: url.into(),
    };
    w.cmd(ChatCommand::SendGif {
        chat: ChatRef::Direct(3),
        gif: GifPick::Found(gif("https://media2.giphy.com/media/wave1/200.gif")),
    });
    let mut sent = Vec::new();
    for _ in 0..100 {
        sent = w.engine.take_requests();
        if !sent.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
        w.settle();
    }
    let Some(Request::SendFile {
        chat, path, kind, ..
    }) = sent.pop()
    else {
        panic!("no file request: {sent:?}")
    };
    assert_eq!((chat, kind), (ChatRef::Direct(3), FileKind::Gif));
    assert!(path.starts_with(&w.env.paths().chat_dir), "{path:?}");
    assert_eq!(std::fs::read(&path).unwrap(), GIF);
    assert_eq!(
        *asked.lock().unwrap(),
        ["http://contentproxy.signal.org:443 https://media2.giphy.com/media/wave1/200.gif"]
    );
    let downloads = w.env.paths().chat_dir.join("gif-downloads");
    assert!(
        std::fs::read_dir(&downloads).map_or(true, |mut d| d.next().is_none()),
        "the download stays"
    );

    // Not one of Giphy's addresses: nothing is fetched or sent.
    w.drain();
    w.cmd(ChatCommand::SendGif {
        chat: ChatRef::Direct(3),
        gif: GifPick::Found(gif("https://evil.example/x.gif")),
    });
    std::thread::sleep(Duration::from_millis(200));
    w.settle();
    assert!(w.engine.take_requests().is_empty());
    assert_eq!(asked.lock().unwrap().len(), 1);
    assert!(
        w.drain()
            .iter()
            .any(|e| matches!(e, Event::Notice(Notice::ChatGifNotSent)))
    );
}

#[test]
fn a_chats_latest_shown_message_fills_an_empty_preview() {
    let mut w = World::new(true);
    ready_with_kai(&mut w);
    let kai = ChatRef::Direct(3);
    let mut m = msg(kai, 4, "", MessageAuthor::Friend);
    m.body = MessageBody::Notice(zaun_core::model::ChatNotice::Connected);
    w.engine_says(ChatEvent::Preview {
        chat: kai,
        last: Box::new(m.clone()),
    });
    assert_eq!(w.svc.hub().chat.load().chats[0].last, Some(m.clone()));
    // An older one never replaces a newer preview.
    let mut older = m;
    older.id = 2;
    w.engine_says(ChatEvent::Preview {
        chat: kai,
        last: Box::new(older),
    });
    assert_eq!(
        w.svc.hub().chat.load().chats[0].last.as_ref().unwrap().id,
        4
    );
}

// ---- saving a file from a chat --------------------------------------------------------------

/// Ask to save item 7 of Kai's chat at `to`, answer the engine's lookup with `found`, and
/// collect the service's events until the copy reports (or nothing more comes).
fn save(w: &mut World, to: PathBuf, found: Option<PathBuf>) -> Vec<Event> {
    w.drain();
    w.cmd(ChatCommand::SaveFile {
        chat: ChatRef::Direct(3),
        item: 7,
        to,
    });
    let Request::LocateFile { tag, .. } = w.next_request() else {
        panic!("no lookup")
    };
    w.engine_says(ChatEvent::Located { tag, path: found });
    let mut seen = Vec::new();
    for _ in 0..100 {
        w.settle();
        seen.extend(w.drain());
        if seen.iter().any(|e| {
            matches!(
                e,
                Event::Failure(_) | Event::Notice(Notice::ChatFileSaved { .. })
            )
        }) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    seen
}

fn failed_with(events: &[Event], code: ErrorCode) -> bool {
    events
        .iter()
        .any(|e| matches!(e, Event::Failure(u) if u.code == code))
}

#[test]
fn a_saved_file_is_copied_where_the_user_chose_and_says_so() {
    let mut w = World::new(true);
    ready_with_kai(&mut w);
    let files = w.env.paths().chat_dir.join("files");
    std::fs::create_dir_all(&files).unwrap();
    let src = files.join("clip.mp4");
    std::fs::write(&src, b"moving pictures").unwrap();
    let downloads = w.env.home().join("Downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let to = downloads.join("clip.mp4");
    // An older file there is replaced (the save dialog asked).
    std::fs::write(&to, b"old").unwrap();

    let events = save(&mut w, to.clone(), Some(src.clone()));
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::Notice(Notice::ChatFileSaved { folder }) if folder == "Downloads"
        )),
        "{events:?}"
    );
    assert_eq!(std::fs::read(&to).unwrap(), b"moving pictures");
    // No temporary file is left behind.
    let left: Vec<_> = std::fs::read_dir(&downloads)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, vec![std::ffi::OsString::from("clip.mp4")]);

    // Two saves to one place at once: each uses its own temporary file; the copy is whole.
    std::fs::write(&src, vec![7u8; 256 * 1024]).unwrap();
    for _ in 0..2 {
        w.cmd(ChatCommand::SaveFile {
            chat: ChatRef::Direct(3),
            item: 7,
            to: to.clone(),
        });
    }
    let tags: Vec<u64> = w
        .engine
        .take_requests()
        .into_iter()
        .map(|r| match r {
            Request::LocateFile { tag, .. } => tag,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(tags.len(), 2);
    assert_ne!(tags[0], tags[1]);
    for tag in tags {
        w.engine.emit(ChatEvent::Located {
            tag,
            path: Some(src.clone()),
        });
    }
    let mut saved = 0;
    for _ in 0..200 {
        w.settle();
        saved += w
            .drain()
            .iter()
            .filter(|e| matches!(e, Event::Notice(Notice::ChatFileSaved { .. })))
            .count();
        if saved == 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(saved, 2);
    assert_eq!(std::fs::read(&to).unwrap(), vec![7u8; 256 * 1024]);
}

#[test]
fn a_save_that_cannot_be_written_says_so_with_its_code() {
    let mut w = World::new(true);
    ready_with_kai(&mut w);
    let files = w.env.paths().chat_dir.join("files");
    std::fs::create_dir_all(&files).unwrap();
    let src = files.join("a.png");
    std::fs::write(&src, b"png").unwrap();
    // A folder that does not exist (as good as one the user cannot write to).
    let events = save(
        &mut w,
        PathBuf::from("/proc/zaun-no-such-folder/a.png"),
        Some(src),
    );
    assert!(failed_with(&events, ErrorCode::ChtSaveFailed), "{events:?}");
    // A message whose file is not here.
    let b = w.env.home().join("b.png");
    let events = save(&mut w, b, None);
    assert!(failed_with(&events, ErrorCode::ChtSaveFailed), "{events:?}");
}

#[test]
fn a_file_outside_the_chat_folder_is_never_copied() {
    let mut w = World::new(true);
    ready_with_kai(&mut w);
    let secret = w.env.home().join("secret.txt");
    std::fs::write(&secret, b"not for chat").unwrap();
    let to = w.env.home().join("copy.txt");
    let events = save(&mut w, to.clone(), Some(secret));
    assert!(failed_with(&events, ErrorCode::ChtSaveFailed), "{events:?}");
    assert!(!to.exists());
}

// ---- profile pictures (ADR-0038) ----------------------------------------------------------

/// Send `c` and wait for the failure it causes (pictures are made on the job pool).
fn failure_of(w: &mut World, c: ChatCommand) -> Option<ErrorCode> {
    w.drain();
    w.cmd(c);
    for _ in 0..200 {
        w.settle();
        if let Some(Event::Failure(u)) = w
            .drain()
            .into_iter()
            .find(|e| matches!(e, Event::Failure(_)))
        {
            return Some(u.code);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

fn a_photo(w: &World, name: &str) -> PathBuf {
    let path = w.env.home().join(name);
    image::RgbImage::from_fn(1600, 900, |x, y| {
        image::Rgb([(x % 251) as u8, (y % 241) as u8, ((x ^ y) % 256) as u8])
    })
    .save(&path)
    .unwrap();
    path
}

#[test]
fn a_picture_file_becomes_the_profile_picture_once_the_engine_takes_it() {
    use zaun_core::chat::picture;
    let mut w = World::new(true);
    ready_with_kai(&mut w);
    let path = a_photo(&w, "me.png");
    w.cmd(ChatCommand::SetPicture { path, crop: None });
    let Request::SetPicture(Some(made)) = w.next_request() else {
        panic!("no picture request")
    };
    assert!(picture::data_uri(&made).len() <= picture::MAX_URI);
    let shown = image::load_from_memory(made.bytes()).unwrap();
    assert_eq!(shown.width(), shown.height(), "a square");
    // Not shown until the engine has it.
    let snap = w.svc.hub().chat.load();
    assert_eq!(snap.profile.as_ref().and_then(|p| p.picture.clone()), None);
    w.engine_says(ChatEvent::Profile(ChatProfile {
        name: "Rin".into(),
        picture: Some(made.clone()),
        ..ChatProfile::default()
    }));
    let snap = w.svc.hub().chat.load();
    assert_eq!(snap.profile.as_ref().unwrap().picture, Some(made));

    w.cmd(ChatCommand::ClearPicture);
    assert_eq!(w.next_request(), Request::SetPicture(None));
}

#[test]
fn a_file_that_is_not_a_picture_or_is_too_large_is_refused() {
    let mut w = World::new(true);
    ready_with_kai(&mut w);
    let text = w.env.home().join("notes.png");
    std::fs::write(&text, b"just some words").unwrap();
    let code = failure_of(
        &mut w,
        ChatCommand::SetPicture {
            path: text,
            crop: None,
        },
    );
    assert_eq!(code, Some(ErrorCode::ChtNotAPicture));
    // 21 MiB (sparse): refused before it is read.
    let big = w.env.home().join("huge.jpg");
    std::fs::File::create(&big)
        .unwrap()
        .set_len(21 * 1024 * 1024)
        .unwrap();
    let code = failure_of(
        &mut w,
        ChatCommand::SetPicture {
            path: big,
            crop: None,
        },
    );
    assert_eq!(code, Some(ErrorCode::ChtPictureTooLarge));
    assert!(w.engine.take_requests().is_empty(), "nothing was sent");
}

#[test]
fn a_group_picture_goes_to_its_group_and_an_unknown_group_is_refused() {
    let mut w = World::new(true);
    ready_with_kai(&mut w);
    w.engine_says(ChatEvent::Chats {
        friends: Vec::new(),
        groups: vec![ChatGroup {
            id: 7,
            name: "Raid".into(),
            owner: true,
            ..ChatGroup::default()
        }],
        requests: Vec::new(),
        chats: Vec::new(),
    });
    let path = a_photo(&w, "raid.png");
    w.cmd(ChatCommand::SetGroupPicture {
        group: 7,
        path: path.clone(),
        crop: Some(zaun_core::model::PictureCrop {
            x: 100,
            y: 0,
            side: 600,
        }),
    });
    let Request::SetGroupPicture {
        group: 7,
        picture: Some(_),
    } = w.next_request()
    else {
        panic!("no group picture request")
    };
    w.cmd(ChatCommand::ClearGroupPicture(7));
    assert_eq!(
        w.next_request(),
        Request::SetGroupPicture {
            group: 7,
            picture: None
        }
    );
    let code = failure_of(
        &mut w,
        ChatCommand::SetGroupPicture {
            group: 99,
            path,
            crop: None,
        },
    );
    assert_eq!(code, Some(ErrorCode::ChtRequestFailed));
}

#[test]
fn picture_commands_while_chat_is_off_are_refused() {
    let mut w = World::new(false);
    let code = failure_of(&mut w, ChatCommand::ClearPicture);
    assert_eq!(code, Some(ErrorCode::ChtRequestFailed));
    assert!(w.engine.take_requests().is_empty());
}

#[test]
fn a_friends_new_picture_reaches_the_snapshot() {
    let mut w = World::new(true);
    ready_with_kai(&mut w);
    let pic = zaun_core::model::Picture::new(vec![0xff, 0xd8, 0xff, 1, 2, 3]);
    let ChatEvent::Chats {
        mut friends,
        groups,
        requests,
        chats,
    } = one_friend()
    else {
        unreachable!()
    };
    friends[0].picture = Some(pic.clone());
    w.engine_says(ChatEvent::Chats {
        friends,
        groups,
        requests,
        chats,
    });
    let snap = w.svc.hub().chat.load();
    assert_eq!(
        snap.friend(3).unwrap().picture.as_ref().map(|p| p.key()),
        Some(pic.key())
    );
}

/// Ask for `path` for the crop step and wait for the answer.
fn picture_source(w: &mut World, path: PathBuf, n: u128) -> QueryResult {
    w.drain();
    w.cmd(ChatCommand::LoadPicture {
        path,
        reply_to: query(n),
    });
    for _ in 0..200 {
        w.settle();
        for e in w.drain() {
            if let Event::QueryResult { id, result } = e
                && id == query(n)
            {
                return result;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("no answer")
}

#[test]
fn the_crop_step_gets_the_pictures_size_and_a_smaller_copy() {
    let mut w = World::new(true);
    let path = a_photo(&w, "wide.png");
    let QueryResult::PictureSource(src) = picture_source(&mut w, path.clone(), 41) else {
        panic!("no picture")
    };
    assert_eq!((src.path, src.width, src.height), (path, 1600, 900));
    let copy = image::load_from_memory(src.preview.bytes()).unwrap();
    assert_eq!((copy.width(), copy.height()), (1024, 576));

    let text = w.env.home().join("notes.jpg");
    std::fs::write(&text, b"not a picture").unwrap();
    let QueryResult::Failed(e) = picture_source(&mut w, text, 42) else {
        panic!("not refused")
    };
    assert_eq!(e.code, ErrorCode::ChtNotAPicture);
}

#[test]
fn a_rename_and_a_new_picture_saved_together_reach_the_engine_rename_first() {
    let mut w = World::new(true);
    ready_with_kai(&mut w);
    // One Save: the name changes at once, the picture follows from the job pool.
    w.cmd(ChatCommand::SetProfile(ChatProfile {
        name: "Rin Two".into(),
        about: "Co-op only.".into(),
        ..ChatProfile::default()
    }));
    let path = a_photo(&w, "me.png");
    w.cmd(ChatCommand::SetPicture { path, crop: None });
    let mut requests = Vec::new();
    for _ in 0..200 {
        w.settle();
        requests.extend(w.engine.take_requests());
        if requests.iter().any(|r| matches!(r, Request::SetPicture(_))) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        matches!(requests[0], Request::SetProfile(_)),
        "{requests:?}"
    );
    // The engine applies them in order, so the picture goes out in the renamed profile
    // (zaun-chat's `setting_the_picture_sends_it_in_the_profile_and_keeps_name_and_about`).
    let Request::SetProfile(p) = &requests[0] else {
        unreachable!()
    };
    assert_eq!(
        (p.name.as_str(), p.about.as_str()),
        ("Rin Two", "Co-op only.")
    );
    let pictures = requests
        .iter()
        .filter(|r| matches!(r, Request::SetPicture(Some(_))));
    assert_eq!(pictures.count(), 1, "{requests:?}");
}
