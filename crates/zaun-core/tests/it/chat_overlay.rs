//! The in-game overlay in the service (ADR-0037) against the fake engine and the fake
//! overlay: it starts with a game when Chat and `chat.overlay` are on and stops when the game
//! ends, restarts after a crash with a back-off, and what is typed in it reaches the engine.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use zaun_core::artwork::steam::FsAppIdFile;
use zaun_core::artwork::{ArtIndex, NoIcons};
use zaun_core::chat::fake::FakeEngine;
use zaun_core::chat::overlay::fake::FakeOverlay;
use zaun_core::chat::overlay::protocol::EXIT_UNSUPPORTED;
use zaun_core::chat::overlay::{ChatKey, FromOverlay, OverlayEvent, ToOverlay};
use zaun_core::chat::{ChatEvent, Request};
use zaun_core::clock::{Clock, SystemClock, Timestamp};
use zaun_core::config::Preferences;
use zaun_core::fsx::{Fs, MountTable, RealFs};
use zaun_core::host::StaticHostFacts;
use zaun_core::identity::{FsPeInspector, SharedLocations};
use zaun_core::ids::RuntimeName;
use zaun_core::jobs::{JobHandle, JobLabel, JobPool, JobPoolConfig, JobPriority};
use zaun_core::launch::NoLocks;
use zaun_core::library::draft::NoUmuTitles;
use zaun_core::library::service::NoPolicyCheck;
use zaun_core::model::{
    ChatMessage, ChatPreview, ChatProfile, ChatRef, Delivery, EndReason, Friend, Game,
    MessageAuthor, MessageBody, RuntimeRef, Session, SessionState, SlrVariant, SupervisorIdent,
};
use zaun_core::procfs::RealProc;
use zaun_core::runtime::{ProvisionNeed, Resolution, RuntimeCatalog, RuntimeService};
use zaun_core::service::LaunchEnv;
use zaun_core::service::{AppService, Command, OverlayAction, ServiceDeps};
use zaun_core::storage::{Db, GameRepo, SessionClose, SessionRepo};
use zaun_testkit::TestEnv;
use zaun_testkit::fakes::FakeLauncher;
use zaun_testkit::fixtures::{games, hosts};

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

fn deps(
    env: &TestEnv,
    engine: &FakeEngine,
    overlay: &FakeOverlay,
    prefs: Preferences,
) -> ServiceDeps {
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
        overlay: Some(Arc::new(overlay.clone())),
        media: None,
        updates: None,
    }
}

struct World {
    env: TestEnv,
    svc: AppService,
    events: std::sync::mpsc::Receiver<zaun_core::service::Event>,
    engine: FakeEngine,
    overlay: FakeOverlay,
    session: Option<Session>,
    /// Sessions started so far (each game run is a new row).
    runs: u32,
}

impl World {
    /// Chat on and ready with Ana and a group, `chat.overlay` as given.
    fn new(overlay_on: bool) -> Self {
        Self::on_desktop(overlay_on, None)
    }

    /// The same on a desktop that says whether it has a layer shell.
    fn on_desktop(overlay_on: bool, layer_shell: Option<bool>) -> Self {
        let env = TestEnv::new().unwrap();
        let engine = FakeEngine::new();
        let overlay = FakeOverlay::new();
        overlay.set_layer_shell(layer_shell);
        let mut prefs = Preferences::default();
        prefs.chat.enabled = true;
        prefs.chat.name = "Rin".into();
        prefs.chat.overlay = overlay_on;
        let (svc, events) =
            AppService::open_headless(deps(&env, &engine, &overlay, prefs)).unwrap();
        let mut w = World {
            env,
            svc,
            events,
            engine,
            overlay,
            session: None,
            runs: 0,
        };
        w.settle();
        w.engine.emit(ChatEvent::Ready(ChatProfile {
            name: "Rin".into(),
            ..ChatProfile::default()
        }));
        w.engine.emit(ChatEvent::Chats {
            friends: vec![Friend {
                id: 1,
                name: "Ana".into(),
                ready: true,
                ..Friend::default()
            }],
            groups: Vec::new(),
            requests: Vec::new(),
            chats: vec![
                ChatPreview {
                    chat: ChatRef::Direct(1),
                    name: "Ana".into(),
                    last: None,
                    unread: 0,
                    muted: false,
                },
                ChatPreview {
                    chat: ChatRef::Group(5),
                    name: "Raid".into(),
                    last: None,
                    unread: 0,
                    muted: false,
                },
            ],
        });
        w.settle();
        w.engine.take_requests();
        w
    }

    fn settle(&mut self) {
        while self.svc.pump(Duration::from_millis(150)) {}
    }

    /// A game session row whose supervisor is this process (alive), then a watcher poll.
    fn start_game(&mut self) {
        let mut db = Db::open_rw(&self.env.paths().db_file).unwrap();
        let game = games::game(1, 1);
        if self.runs == 0 {
            db.insert_game(&game).unwrap();
        }
        self.runs += 1;
        let mut s = games::session(&game, 1, self.runs, SystemClock::new().now());
        s.state = SessionState::Running;
        let me =
            zaun_core::session::sys::own_identity(&RealProc::new(Arc::new(RealFs::new()))).unwrap();
        s.supervisor = Some(SupervisorIdent {
            pid: me.pid,
            start_ticks: me.start_time_ticks,
            boot_id: String::new(),
        });
        db.insert_session(&s).unwrap();
        self.session = Some(s);
        self.svc.poll_sessions_now();
        self.settle();
    }

    fn end_game(&mut self) {
        let s = self.session.take().unwrap();
        let mut db = Db::open_rw(&self.env.paths().db_file).unwrap();
        db.close_session(&SessionClose {
            id: s.id,
            ended_at: SystemClock::new().now(),
            duration_ms: 1000,
            end_reason: EndReason::Exited { code: 0 },
            exit_status: Some(0),
            counted: false,
            pieces: Vec::new(),
        })
        .unwrap();
        self.svc.poll_sessions_now();
        self.settle();
    }

    /// Speak as the overlay and let the service take it.
    fn overlay_says(&mut self, m: FromOverlay) {
        self.overlay.say(m);
        self.settle();
    }

    fn overlay_exits(&mut self, status: Option<i32>, ready: bool) {
        self.overlay.emit(OverlayEvent::Exited { status, ready });
        self.settle();
    }

    /// The last snapshot sent to the overlay.
    fn last_state(&self) -> Option<zaun_core::chat::overlay::OverlaySnapshot> {
        self.overlay
            .take_sent()
            .into_iter()
            .rev()
            .find_map(|m| match m {
                ToOverlay::State { state } => Some(*state),
                ToOverlay::Open | ToOverlay::Close => None,
            })
    }
}

fn text(chat: ChatRef, id: i64, words: &str, author: MessageAuthor) -> ChatMessage {
    ChatMessage {
        id,
        chat,
        delivery: Delivery::Received,
        unread: author != MessageAuthor::Me,
        author,
        body: MessageBody::Text(words.into()),
        at: Timestamp::from_millis(1_800_000_000_000 + id),
        reply: None,
        reactions: Vec::new(),
        edited: false,
        mentions: Vec::new(),
    }
}

#[test]
fn the_overlay_runs_only_while_a_game_runs_with_chat_and_the_setting_on() {
    let mut w = World::new(true);
    assert!(!w.overlay.running(), "no game yet");
    w.start_game();
    assert!(w.overlay.running());
    assert_eq!(w.overlay.starts(), 1);
    let state = w.last_state().expect("a snapshot was sent");
    assert_eq!(state.chats[0].chat, ChatKey::Direct(1));
    w.end_game();
    assert!(!w.overlay.running(), "stopped with the game");

    let mut off = World::new(false);
    off.start_game();
    assert_eq!(off.overlay.starts(), 0, "chat.overlay is off by default");
}

#[test]
fn turning_the_setting_off_stops_the_overlay() {
    let mut w = World::new(true);
    w.start_game();
    assert!(w.overlay.running());
    let patch = zaun_core::config::PreferencePatch::new().set("chat.overlay", false);
    w.svc.handle(Command::SetPreference(patch));
    w.settle();
    assert!(!w.overlay.running());
}

#[test]
fn a_crashed_overlay_restarts_with_a_back_off_and_is_given_up_after_five() {
    let mut w = World::new(true);
    w.start_game();
    for n in 1..5 {
        w.overlay_exits(Some(101), false);
        // The fake waits a hundredth of the back-off.
        std::thread::sleep(Duration::from_millis(10 << n));
        w.settle();
        assert_eq!(w.overlay.starts(), n + 1, "restart {n}");
    }
    w.overlay_exits(Some(101), false);
    std::thread::sleep(Duration::from_millis(400));
    w.settle();
    assert_eq!(w.overlay.starts(), 5, "given up after five exits in a row");
    let waits: Vec<u64> = w
        .overlay
        .restart_waits()
        .iter()
        .map(Duration::as_secs)
        .collect();
    assert_eq!(waits, vec![1, 2, 4, 8]);
    // The next game gives it a new chance.
    w.end_game();
    w.start_game();
    assert_eq!(w.overlay.starts(), 6);
}

#[test]
fn an_overlay_that_was_ready_starts_its_failure_count_again() {
    let mut w = World::new(true);
    w.start_game();
    for _ in 0..7 {
        w.overlay_says(FromOverlay::Ready);
        w.overlay_exits(Some(101), true);
        std::thread::sleep(Duration::from_millis(30));
        w.settle();
    }
    assert_eq!(w.overlay.starts(), 8);
    assert!(w.overlay.restart_waits().iter().all(|d| d.as_secs() == 1));
}

#[test]
fn a_desktop_without_a_layer_shell_is_not_retried() {
    let mut w = World::new(true);
    w.start_game();
    w.overlay_exits(Some(EXIT_UNSUPPORTED), false);
    std::thread::sleep(Duration::from_millis(100));
    w.settle();
    assert_eq!(w.overlay.starts(), 1);
    assert!(w.overlay.restart_waits().is_empty());
}

#[test]
fn an_overlay_speaking_another_version_is_stopped_and_not_restarted() {
    let mut w = World::new(true);
    w.start_game();
    w.overlay.emit(OverlayEvent::Incompatible(2));
    w.settle();
    assert!(!w.overlay.running());
    std::thread::sleep(Duration::from_millis(100));
    w.settle();
    assert_eq!(w.overlay.starts(), 1);
}

#[test]
fn a_late_exit_of_a_replaced_overlay_restarts_nothing() {
    let mut w = World::new(true);
    w.start_game();
    let old = w.overlay.sink().expect("running");
    w.end_game();
    w.start_game();
    assert_eq!(w.overlay.starts(), 2);
    old(OverlayEvent::Exited {
        status: Some(101),
        ready: false,
    });
    std::thread::sleep(Duration::from_millis(50));
    w.settle();
    assert_eq!(w.overlay.starts(), 2);
    assert!(w.overlay.running());
    assert!(w.overlay.restart_waits().is_empty());
}

#[test]
fn a_message_typed_in_the_overlay_is_sent_to_its_conversation() {
    let mut w = World::new(true);
    w.start_game();
    w.engine.take_requests();
    w.overlay_says(FromOverlay::SendMessage {
        chat: ChatKey::Direct(1),
        text: "gg".into(),
    });
    let sent: Vec<Request> = w.engine.take_requests();
    assert_eq!(
        sent,
        vec![Request::Send {
            chat: ChatRef::Direct(1),
            text: "gg".into(),
            reply_to: None,
            mentions: Vec::new(),
        }]
    );
    // Only to conversations it was shown.
    w.overlay_says(FromOverlay::SendMessage {
        chat: ChatKey::Direct(99),
        text: "x".into(),
    });
    assert!(w.engine.take_requests().is_empty());
}

#[test]
fn the_hotkey_opens_and_closes_the_panel_and_reads_the_open_conversation() {
    let mut w = World::new(true);
    w.start_game();
    w.overlay.take_sent();
    w.svc.handle(Command::Overlay(OverlayAction::Toggle));
    w.settle();
    assert!(w.overlay.take_sent().contains(&ToOverlay::Open));
    w.svc.handle(Command::Overlay(OverlayAction::Open));
    w.settle();
    assert!(
        !w.overlay.take_sent().contains(&ToOverlay::Open),
        "already open"
    );
    w.overlay_says(FromOverlay::Closed);
    w.svc.handle(Command::Overlay(OverlayAction::Toggle));
    w.settle();
    assert!(
        w.overlay.take_sent().contains(&ToOverlay::Open),
        "closed from inside"
    );
    w.svc.handle(Command::Overlay(OverlayAction::Close));
    w.settle();
    assert!(w.overlay.take_sent().contains(&ToOverlay::Close));
}

#[test]
fn the_open_conversation_gets_its_latest_page_and_new_messages_and_others_toast() {
    let mut w = World::new(true);
    w.start_game();
    let history = w
        .engine
        .take_requests()
        .into_iter()
        .find_map(|r| match r {
            Request::History { chat, tag, .. } => Some((chat, tag)),
            _ => None,
        })
        .expect("the overlay asked for its conversation");
    assert_eq!(history.0, ChatRef::Direct(1));
    let ana = ChatRef::Direct(1);
    w.engine.emit(ChatEvent::History {
        tag: history.1,
        chat: ana,
        messages: vec![text(ana, 1, "you in?", MessageAuthor::Friend)],
    });
    w.engine.emit(ChatEvent::Messages(vec![
        text(ana, 2, "ready", MessageAuthor::Friend),
        text(
            ChatRef::Group(5),
            3,
            "pull",
            MessageAuthor::Member("Bo".into()),
        ),
    ]));
    w.settle();
    std::thread::sleep(Duration::from_millis(150));
    w.settle();
    let state = w.last_state().expect("state");
    let open = state.open.expect("open conversation");
    let texts: Vec<&str> = open.messages.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts, vec!["you in?", "ready"]);
    // Closed, a message from the open conversation is a toast too; so is the other one.
    assert_eq!(state.toasts.len(), 2);
    assert_eq!(state.toasts[1].text, "pull");
}

#[test]
fn the_overlay_stops_when_chat_stops_running() {
    let mut w = World::new(true);
    w.start_game();
    assert!(w.overlay.running());
    w.engine.emit(ChatEvent::Failed {
        request: zaun_core::chat::RequestKind::Start,
        code: zaun_core::error::ErrorCode::ChtDatabase,
        detail: "the database does not open".into(),
    });
    w.settle();
    assert!(!w.overlay.running());
    assert_eq!(w.overlay.starts(), 1);
}

#[test]
fn a_page_for_a_conversation_switched_away_from_reaches_neither_panel_nor_page() {
    let mut w = World::new(true);
    w.start_game();
    let tags = |w: &World| -> Vec<u64> {
        w.engine
            .take_requests()
            .into_iter()
            .filter_map(|r| match r {
                Request::History { tag, .. } => Some(tag),
                _ => None,
            })
            .collect()
    };
    let first = tags(&w);
    w.overlay_says(FromOverlay::OpenConversation {
        chat: ChatKey::Group(5),
    });
    assert_eq!(tags(&w).len(), 1);
    let _ = w.events.try_iter().count();
    let ana = ChatRef::Direct(1);
    w.engine.emit(ChatEvent::History {
        tag: first[0],
        chat: ana,
        messages: vec![text(ana, 1, "late", MessageAuthor::Friend)],
    });
    w.settle();
    std::thread::sleep(Duration::from_millis(150));
    w.settle();
    assert!(
        !w.events
            .try_iter()
            .any(|e| matches!(e, zaun_core::service::Event::QueryResult { .. }))
    );
    let open = w.last_state().and_then(|s| s.open);
    assert!(open.is_none_or(|o| o.chat == ChatKey::Group(5) && o.messages.is_empty()));
}

#[test]
fn the_show_while_playing_switches_are_the_settings_keys() {
    let mut w = World::new(true);
    w.start_game();
    w.overlay_says(FromOverlay::SetLook {
        toasts: false,
        voice_widget: false,
        corner: zaun_core::chat::overlay::Corner::BottomRight,
    });
    let prefs = w.svc.hub().prefs.load();
    assert!(!prefs.chat.overlay_toasts && !prefs.chat.overlay_voice_widget);
    assert_eq!(
        prefs.chat.overlay_corner,
        zaun_core::config::schema::OverlayCorner::BottomRight
    );
}

#[test]
fn the_hub_says_why_the_overlay_cannot_show_until_the_setting_is_turned_off_and_on() {
    use zaun_core::chat::overlay::OverlayStatus;
    let mut w = World::new(true);
    assert_eq!(*w.svc.hub().overlay.load(), OverlayStatus::Idle);
    w.start_game();
    assert_eq!(*w.svc.hub().overlay.load(), OverlayStatus::Running);
    w.overlay_exits(Some(EXIT_UNSUPPORTED), false);
    assert_eq!(*w.svc.hub().overlay.load(), OverlayStatus::Unsupported);
    // The desktop does not change with the next game.
    w.end_game();
    w.start_game();
    assert_eq!(w.overlay.starts(), 1);
    assert_eq!(*w.svc.hub().overlay.load(), OverlayStatus::Unsupported);
    for on in [false, true] {
        let patch = zaun_core::config::PreferencePatch::new().set("chat.overlay", on);
        w.svc.handle(Command::SetPreference(patch));
        w.settle();
    }
    assert_eq!(w.overlay.starts(), 2);
    assert_eq!(*w.svc.hub().overlay.load(), OverlayStatus::Running);
}

#[test]
fn an_overlay_that_keeps_failing_is_reported_as_failing() {
    use zaun_core::chat::overlay::OverlayStatus;
    let mut w = World::new(true);
    w.start_game();
    w.overlay.emit(OverlayEvent::Incompatible(9));
    w.settle();
    assert_eq!(*w.svc.hub().overlay.load(), OverlayStatus::Failing);
}

#[test]
fn a_restarted_overlay_is_sent_the_pictures_again_and_not_the_old_toasts() {
    let mut w = World::new(true);
    w.engine.emit(ChatEvent::Chats {
        friends: vec![Friend {
            id: 1,
            name: "Ana".into(),
            ready: true,
            picture: Some(zaun_core::model::Picture::new(vec![0x89, b'P', b'N', b'G'])),
            ..Friend::default()
        }],
        groups: Vec::new(),
        requests: Vec::new(),
        chats: vec![ChatPreview {
            chat: ChatRef::Direct(1),
            name: "Ana".into(),
            last: None,
            unread: 0,
            muted: false,
        }],
    });
    w.settle();
    w.start_game();
    std::thread::sleep(Duration::from_millis(150));
    w.settle();
    w.engine.emit(ChatEvent::Messages(vec![text(
        ChatRef::Direct(1),
        5,
        "gg",
        MessageAuthor::Friend,
    )]));
    w.settle();
    std::thread::sleep(Duration::from_millis(150));
    w.settle();
    let pictures_in = |sent: Vec<ToOverlay>| -> (usize, usize) {
        sent.into_iter().fold((0, 0), |(p, t), m| match m {
            ToOverlay::State { state } => (p + state.pictures.len(), state.toasts.len()),
            ToOverlay::Open | ToOverlay::Close => (p, t),
        })
    };
    assert_eq!(
        pictures_in(w.overlay.take_sent()),
        (1, 1),
        "sent once, with the toast"
    );
    w.overlay_exits(Some(101), true);
    std::thread::sleep(Duration::from_millis(50));
    w.settle();
    assert_eq!(w.overlay.starts(), 2);
    w.overlay_says(FromOverlay::Ready);
    std::thread::sleep(Duration::from_millis(150));
    w.settle();
    let (pictures, toasts) = pictures_in(w.overlay.take_sent());
    assert_eq!(pictures, 1, "the new process holds no pictures");
    assert_eq!(toasts, 0, "the old toasts are not shown again");
}

#[test]
fn a_desktop_without_a_layer_shell_is_known_at_start_even_with_the_overlay_off() {
    use zaun_core::chat::overlay::OverlayStatus;
    let mut w = World::on_desktop(false, Some(false));
    assert_eq!(w.overlay.probes(), 1, "asked once, at the service's start");
    assert_eq!(*w.svc.hub().overlay.load(), OverlayStatus::Unsupported);
    let patch = zaun_core::config::PreferencePatch::new().set("chat.overlay", true);
    w.svc.handle(Command::SetPreference(patch));
    w.settle();
    w.start_game();
    assert_eq!(w.overlay.starts(), 0, "never started where it cannot show");
    assert_eq!(*w.svc.hub().overlay.load(), OverlayStatus::Unsupported);
    assert_eq!(w.overlay.probes(), 1);
}

#[test]
fn a_desktop_with_a_layer_shell_starts_the_overlay_as_usual() {
    use zaun_core::chat::overlay::OverlayStatus;
    let mut w = World::on_desktop(true, Some(true));
    assert_eq!(*w.svc.hub().overlay.load(), OverlayStatus::Idle);
    w.start_game();
    assert_eq!(w.overlay.starts(), 1);
}

#[test]
fn snapshots_are_not_sent_again_when_nothing_changed() {
    let mut w = World::new(true);
    w.start_game();
    std::thread::sleep(Duration::from_millis(150));
    w.settle();
    w.overlay.take_sent();
    // Chat publishes for reasons the overlay does not show (the relays went offline).
    w.engine.emit(ChatEvent::Offline(true));
    w.settle();
    std::thread::sleep(Duration::from_millis(150));
    w.settle();
    assert!(w.overlay.take_sent().is_empty());
}

/// The real `zaun-overlay` started by the real factory under bwrap, on a real
/// compositor (ADR-0037; a local recipe kept by the maintainer). It needs
/// `ZAUN_OVERLAY_BIN` (the built overlay), `ZAUN_OVERLAY_WAYLAND` (a compositor's
/// socket, e.g. a private headless sway's) and `ZAUN_OVERLAY_PHASES` (a folder). It
/// shows the design's scenes one after the other and writes each one's name there once it
/// is drawn (`closed`, `contrast`, `call`, `answer`, `ar`, `open`), waiting for `<name>.done` before the
/// next; then it waits up to a minute for a message typed into the panel.
#[test]
#[ignore = "needs a compositor and the built overlay"]
fn the_real_overlay_draws_under_bwrap_and_hands_back_what_is_typed() {
    use super::chat_overlay_scene::{Scene, scene};
    use zaun_core::chat::overlay::{HelperOverlayFactory, OverlayFactory, OverlaySink};
    let var = |k: &str| std::env::var_os(k).unwrap_or_else(|| panic!("{k} is not set"));
    let phases = PathBuf::from(var("ZAUN_OVERLAY_PHASES"));
    let fs = RealFs::new();
    let factory = HelperOverlayFactory {
        helper: var("ZAUN_OVERLAY_BIN").into(),
        bwrap: zaun_core::host::bwrap_probe::locate_bwrap(&fs, std::env::var_os("PATH").as_deref()),
        wayland: Some(var("ZAUN_OVERLAY_WAYLAND").into()),
        gpu_nodes: std::fs::read_dir("/dev")
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with("nvidia"))
            .map(|e| e.path())
            .filter(|p| !p.is_dir())
            .collect(),
        library_path: std::env::var_os("LD_LIBRARY_PATH"),
        log: phases.join("overlay.log"),
    };
    assert!(factory.bwrap.is_some(), "bwrap is needed for this test");
    let (tx, rx) = std::sync::mpsc::channel();
    let sink: OverlaySink = Arc::new(move |e| {
        let _ = tx.send(e);
    });
    let overlay = factory.start(sink).unwrap();
    let wait_for = |want: &dyn Fn(&OverlayEvent) -> bool, secs: u64| {
        let deadline = std::time::Instant::now() + Duration::from_secs(secs);
        while let Ok(e) =
            rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
        {
            if want(&e) {
                return Some(e);
            }
            assert!(
                !matches!(e, OverlayEvent::Exited { .. }),
                "the overlay exited: {e:?}"
            );
        }
        None
    };
    wait_for(&|e| *e == OverlayEvent::Said(FromOverlay::Ready), 30).expect("ready");
    let show = |name: &str, s: Scene, open: bool| {
        overlay.send(&ToOverlay::State {
            state: Box::new(scene(s)),
        });
        overlay.send(if open {
            &ToOverlay::Open
        } else {
            &ToOverlay::Close
        });
        // The toasts' and the panel's motion is over well within this.
        std::thread::sleep(Duration::from_millis(1200));
        std::fs::write(phases.join(name), "").unwrap();
        for _ in 0..600 {
            if phases.join(format!("{name}.done")).exists() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("no screenshot of {name}");
    };
    show("closed", Scene::Closed, false);
    show("contrast", Scene::Contrast, false);
    show("call", Scene::Ringing, false);
    show("answer", Scene::Ringing, true);
    show("ar", Scene::Arabic, true);
    show("open", Scene::Open, true);
    std::fs::write(phases.join("type"), "").unwrap();
    let typed = wait_for(
        &|e| matches!(e, OverlayEvent::Said(FromOverlay::SendMessage { .. })),
        60,
    )
    .expect("a message typed into the panel");
    std::fs::write(phases.join("typed"), format!("{typed:?}")).unwrap();
    drop(overlay);
    let exit = wait_for(&|e| matches!(e, OverlayEvent::Exited { .. }), 10);
    assert_eq!(
        exit,
        Some(OverlayEvent::Exited {
            status: Some(0),
            ready: true
        })
    );
}
