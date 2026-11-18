//! Game streams in the service (ADR-0036) against the fake engine and fake streams: sharing
//! needs a running game and ends with it, one share and one watched stream at a time, a
//! friend's stream is taken at once and shows the picture when watched, it ends when the
//! friend stops, and blocking stops it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use zaun_core::artwork::steam::FsAppIdFile;
use zaun_core::artwork::{ArtIndex, NoIcons};
use zaun_core::chat::fake::FakeEngine;
use zaun_core::chat::media::fake::FakeMedia;
use zaun_core::chat::media::fake_stream::FakeAv;
use zaun_core::chat::media::pattern::shows_test_pattern;
use zaun_core::chat::media::{MediaFactory, StreamEvent, StreamRequest, StreamSignal};
use zaun_core::chat::{ChatEvent, Request};
use zaun_core::clock::{Clock, SystemClock};
use zaun_core::config::Preferences;
use zaun_core::error::ErrorCode;
use zaun_core::fsx::{Fs, MountTable, RealFs};
use zaun_core::host::StaticHostFacts;
use zaun_core::identity::{FsPeInspector, SharedLocations};
use zaun_core::ids::{GameId, RuntimeName};
use zaun_core::jobs::{JobHandle, JobLabel, JobPool, JobPoolConfig, JobPriority};
use zaun_core::launch::NoLocks;
use zaun_core::library::draft::NoUmuTitles;
use zaun_core::library::service::NoPolicyCheck;
use zaun_core::model::{
    ChatFeatures, ChatLive, ChatProfile, ChatRef, EndReason, Friend, Game, RuntimeRef,
    SessionState, SlrVariant,
};
use zaun_core::procfs::RealProc;
use zaun_core::runtime::{ProvisionNeed, Resolution, RuntimeCatalog, RuntimeService};
use zaun_core::service::{
    AfterCreate, AppService, ChatCommand, Command, Event, LaunchEnv, Notice, OpenOrigin,
    ServiceDeps, ShareHeight,
};
use zaun_core::storage::{Db, GameRepo, SessionClose, SessionRepo};
use zaun_testkit::TestEnv;
use zaun_testkit::fakes::FakeLauncher;
use zaun_testkit::fixtures::{games, hosts, pe};

const MIRA: i64 = 3;
const DEV: i64 = 4;

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

struct World {
    env: TestEnv,
    svc: AppService,
    events: Receiver<Event>,
    engine: FakeEngine,
    av: FakeAv,
    session: Option<zaun_core::model::Session>,
}

fn deps(env: &TestEnv, engine: &FakeEngine, media: Arc<dyn MediaFactory>) -> ServiceDeps {
    let fs: Arc<dyn Fs> = Arc::new(RealFs::new());
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let jobs = Arc::new(JobPool::new(JobPoolConfig::default(), Arc::clone(&clock)).unwrap());
    let mut prefs = Preferences::default();
    prefs.chat.enabled = true;
    prefs.chat.name = "Rin".into();
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
        media: Some(media),
    }
}

fn friend(id: i64, name: &str) -> Friend {
    Friend {
        id,
        name: name.into(),
        ready: true,
        ..Friend::default()
    }
}

impl World {
    /// Chat ready with two friends; streams carried by the media.
    fn new() -> Self {
        Self::with_media(None)
    }

    fn with_media(media: Option<Arc<dyn MediaFactory>>) -> Self {
        let env = TestEnv::new().unwrap();
        let engine = FakeEngine::new();
        let av = FakeAv::default();
        let media = media.unwrap_or_else(|| Arc::new(av.clone()));
        let (svc, events) = AppService::open_headless(deps(&env, &engine, media)).unwrap();
        let mut w = World {
            env,
            svc,
            events,
            engine,
            av,
            session: None,
        };
        w.settle();
        w.engine_says(ChatEvent::Features {
            features: ChatFeatures {
                calls: true,
                share: true,
                ..ChatFeatures::default()
            },
            reactions: None,
        });
        w.engine_says(ChatEvent::Ready(ChatProfile {
            name: "Rin".into(),
            ..ChatProfile::default()
        }));
        w.engine_says(ChatEvent::Chats {
            friends: vec![friend(MIRA, "Mira Okafor"), friend(DEV, "Dev Patel")],
            groups: Vec::new(),
            requests: Vec::new(),
            chats: Vec::new(),
        });
        w.engine.take_requests();
        w.drain();
        w
    }

    fn settle(&mut self) {
        while self.svc.pump(Duration::from_millis(50)) {}
    }

    fn drain(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    fn send(&mut self, c: Command) -> Vec<Event> {
        self.svc.handle(c);
        self.settle();
        self.drain()
    }

    fn cmd(&mut self, c: ChatCommand) {
        self.svc.handle(Command::Chat(c));
        self.settle();
    }

    fn engine_says(&mut self, e: ChatEvent) {
        self.engine.emit(e);
        self.settle();
    }

    fn signal(&mut self, s: StreamSignal) {
        self.engine_says(ChatEvent::Stream(s));
    }

    fn share_says(&mut self, e: StreamEvent) {
        self.av.streams.emit_share(e);
        self.settle();
    }

    fn watch_says(&mut self, e: StreamEvent) {
        self.av.streams.emit_watch(e);
        self.settle();
    }

    fn live(&self) -> ChatLive {
        self.svc.hub().chat.load().live.clone()
    }

    /// The stream requests made since last asked.
    fn streams(&self) -> Vec<StreamRequest> {
        self.engine
            .take_requests()
            .into_iter()
            .filter_map(|r| match r {
                Request::Stream(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    fn failures(&self) -> Vec<ErrorCode> {
        self.drain()
            .into_iter()
            .filter_map(|e| match e {
                Event::Failure(u) => Some(u.code),
                _ => None,
            })
            .collect()
    }

    /// Add a game and make it run: an open session row whose supervisor is this process.
    fn run_game(&mut self, dir: &str) -> GameId {
        let d = self.env.home().join("Games").join(dir);
        std::fs::create_dir_all(&d).unwrap();
        let exe = d.join(format!("{dir}.exe"));
        std::fs::write(&exe, pe::sample_game()).unwrap();
        let evs = self.send(Command::OpenPaths {
            paths: vec![exe],
            origin: OpenOrigin::Picker,
        });
        let draft = evs
            .into_iter()
            .find_map(|e| match e {
                Event::SetupRequested { draft, .. } => Some(draft),
                _ => None,
            })
            .expect("setup requested");
        let id = self
            .send(Command::CreateGame {
                draft,
                then: AfterCreate::Nothing,
            })
            .iter()
            .find_map(|e| match e {
                Event::Notice(Notice::GameAdded { game, .. }) => Some(*game),
                _ => None,
            })
            .expect("game added");
        let mut db = Db::open_rw(&self.env.paths().db_file).unwrap();
        let game = db.game(id).unwrap().unwrap();
        let mut s = games::session(&game, 1, 1, SystemClock::new().now());
        s.ended_at = None;
        s.end_reason = None;
        s.duration_ms = None;
        s.state = SessionState::Running;
        let me = zaun_core::session::sys::own_identity(&RealProc::new(Arc::new(RealFs::new())))
            .expect("own identity");
        s.supervisor = Some(zaun_core::model::SupervisorIdent {
            pid: me.pid,
            start_ticks: me.start_time_ticks,
            boot_id: String::new(),
        });
        db.insert_session(&s).unwrap();
        drop(db);
        self.session = Some(s);
        self.svc.poll_sessions_now();
        self.settle();
        self.drain();
        id
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
        drop(db);
        self.svc.poll_sessions_now();
        self.settle();
    }

    /// Sharing the running game with Mira, past the window picker, offered to her app.
    fn sharing_with_mira(&mut self) {
        self.run_game("Lantern");
        self.cmd(ChatCommand::StartShare(ChatRef::Direct(MIRA)));
        self.share_says(StreamEvent::Ready);
        assert_eq!(self.streams(), vec![StreamRequest::Invite(MIRA)]);
    }

    /// Mira's stream taken and connected, her game named.
    fn mira_streams(&mut self) {
        self.signal(StreamSignal::Invited(MIRA));
        self.watch_says(StreamEvent::Description("v=0 offer".into()));
        self.signal(StreamSignal::Answer {
            friend: MIRA,
            sdp: "v=0 answer".into(),
            candidates: Vec::new(),
        });
        self.watch_says(StreamEvent::Connected);
        self.watch_says(StreamEvent::Title("Ninefold Arena".into()));
    }
}

#[test]
fn sharing_without_a_running_game_is_refused() {
    let mut w = World::new();
    w.cmd(ChatCommand::StartShare(ChatRef::Direct(MIRA)));
    assert_eq!(w.failures(), vec![ErrorCode::ChtNoGameRunning]);
    assert!(w.av.streams.shares().is_empty());
    assert!(w.live().share.is_none());
}

#[test]
fn a_share_picks_the_window_then_offers_the_stream_and_answers_the_friends_offer() {
    let mut w = World::new();
    w.run_game("Lantern");
    w.cmd(ChatCommand::StartShare(ChatRef::Direct(MIRA)));
    let shares = w.av.streams.shares();
    assert_eq!(shares.len(), 1);
    // The defaults: 1080 at most, 60 frames a second, the GPU tried first.
    assert_eq!(
        (shares[0].max_height, shares[0].fps, shares[0].hardware),
        (Some(1080), 60, true)
    );
    // While the portal asks, nothing is sent and the page shows no share yet.
    assert!(w.streams().is_empty());
    assert!(w.live().share.is_none());
    w.share_says(StreamEvent::Ready);
    assert_eq!(w.streams(), vec![StreamRequest::Invite(MIRA)]);
    let share = w.live().share.unwrap();
    assert_eq!(
        (share.chat, share.title.as_str()),
        (ChatRef::Direct(MIRA), shares[0].title.as_str())
    );
    assert_eq!(share.watching, 0);
    w.signal(StreamSignal::Offer {
        friend: MIRA,
        sdp: "v=0 offer".into(),
        candidates: Vec::new(),
    });
    assert_eq!(w.av.streams.offers(), vec!["v=0 offer".to_owned()]);
    w.share_says(StreamEvent::Description("v=0 answer".into()));
    w.share_says(StreamEvent::Connected);
    assert_eq!(
        w.streams(),
        vec![
            StreamRequest::Answer {
                friend: MIRA,
                sdp: "v=0 answer".into()
            },
            StreamRequest::Connected(MIRA)
        ]
    );
    w.share_says(StreamEvent::Watching(true));
    assert_eq!(w.live().share.unwrap().watching, 1, "the viewer count");
    w.share_says(StreamEvent::Paused(true));
    assert!(w.live().stream.share_paused);
}

#[test]
fn the_share_ends_with_its_game() {
    let mut w = World::new();
    w.sharing_with_mira();
    w.end_game();
    assert!(w.live().share.is_none());
    assert_eq!(w.streams(), vec![StreamRequest::End(MIRA)]);
    assert_eq!(w.av.streams.running(), (0, 0));
}

#[test]
fn one_share_at_a_time() {
    let mut w = World::new();
    w.sharing_with_mira();
    w.cmd(ChatCommand::StartShare(ChatRef::Direct(DEV)));
    assert_eq!(w.av.streams.shares().len(), 1);
    assert_eq!(w.live().share.unwrap().chat, ChatRef::Direct(MIRA));
    w.cmd(ChatCommand::StopShare);
    assert_eq!(w.streams(), vec![StreamRequest::End(MIRA)]);
    assert!(w.live().share.is_none());
}

#[test]
fn a_refused_window_picker_sends_nothing_and_says_why() {
    let mut w = World::new();
    w.run_game("Lantern");
    w.cmd(ChatCommand::StartShare(ChatRef::Direct(MIRA)));
    w.share_says(StreamEvent::Failed {
        code: ErrorCode::ChtShareRefused,
        detail: "cancelled".into(),
    });
    assert_eq!(w.failures(), vec![ErrorCode::ChtShareRefused]);
    assert!(w.streams().is_empty(), "no invitation, no hang-up");
    assert!(w.live().share.is_none());
}

#[test]
fn streams_are_one_to_one_and_never_while_in_a_call_with_the_friend() {
    let mut w = World::new();
    w.run_game("Lantern");
    w.cmd(ChatCommand::StartShare(ChatRef::Group(9)));
    assert_eq!(w.failures(), vec![ErrorCode::ChtStreamFailed]);
    w.cmd(ChatCommand::Call(ChatRef::Direct(MIRA)));
    w.cmd(ChatCommand::StartShare(ChatRef::Direct(MIRA)));
    assert_eq!(w.failures(), vec![ErrorCode::ChtStreamFailed]);
    assert!(w.av.streams.shares().is_empty());
}

#[test]
fn a_friends_stream_is_taken_at_once_and_watching_shows_its_picture() {
    let mut w = World::new();
    w.mira_streams();
    assert_eq!(w.av.streams.watches(), 1);
    assert_eq!(
        w.streams(),
        vec![
            StreamRequest::Offer {
                friend: MIRA,
                sdp: "v=0 offer".into()
            },
            StreamRequest::Connected(MIRA)
        ]
    );
    assert_eq!(w.av.streams.answers(), vec!["v=0 answer".to_owned()]);
    // Not watched yet: no picture asked for, but the friend shows live in her game.
    assert_eq!(w.av.streams.controls(), (false, false));
    let snap = w.svc.hub().chat.load();
    let playing = snap.friend(MIRA).unwrap().playing.clone().unwrap();
    assert!(playing.live && playing.title == "Ninefold Arena");
    w.cmd(ChatCommand::Watch(ChatRef::Direct(MIRA)));
    assert_eq!(w.av.streams.controls(), (true, false));
    let watch = w.live().watch.unwrap();
    assert_eq!(
        (watch.title.as_str(), watch.watching),
        ("Ninefold Arena", 1)
    );
    let (_, frame) = w.svc.hub().frames.newer_than(0).expect("a picture");
    assert!(shows_test_pattern(&frame));
    w.cmd(ChatCommand::WatchMute(true));
    assert_eq!(w.av.streams.controls(), (true, true));
    assert!(w.live().watch.unwrap().muted);
    w.cmd(ChatCommand::StopWatch);
    assert!(!w.av.streams.controls().0);
    assert!(w.live().watch.is_none());
    assert!(!w.svc.hub().frames.has_frame());
    assert_eq!(
        w.av.streams.running(),
        (0, 1),
        "still taken, ready to watch again"
    );
}

#[test]
fn watching_ends_when_the_friend_stops_and_the_stage_says_so() {
    let mut w = World::new();
    w.mira_streams();
    w.cmd(ChatCommand::Watch(ChatRef::Direct(MIRA)));
    w.streams();
    w.signal(StreamSignal::Ended(MIRA));
    let live = w.live();
    assert!(live.watch.is_none());
    assert_eq!(live.stream.ended, Some(ChatRef::Direct(MIRA)));
    assert_eq!(w.av.streams.running(), (0, 0));
    assert!(
        w.streams().is_empty(),
        "the friend ended it; nothing to say back"
    );
    let snap = w.svc.hub().chat.load();
    assert!(snap.friend(MIRA).unwrap().playing.is_none());
}

#[test]
fn blocking_a_friend_stops_watching_them() {
    let mut w = World::new();
    w.mira_streams();
    w.cmd(ChatCommand::Watch(ChatRef::Direct(MIRA)));
    w.streams();
    w.cmd(ChatCommand::SetBlocked {
        friend: MIRA,
        on: true,
    });
    assert!(w.live().watch.is_none());
    assert_eq!(
        w.live().stream.ended,
        None,
        "the user stopped it, not the friend"
    );
    assert_eq!(w.streams(), vec![StreamRequest::End(MIRA)]);
    assert_eq!(w.av.streams.running(), (0, 0));
}

#[test]
fn one_stream_is_watched_at_a_time() {
    let mut w = World::new();
    w.mira_streams();
    w.signal(StreamSignal::Invited(DEV));
    w.cmd(ChatCommand::Watch(ChatRef::Direct(MIRA)));
    w.cmd(ChatCommand::Watch(ChatRef::Direct(DEV)));
    assert_eq!(w.live().watch.unwrap().chat, ChatRef::Direct(DEV));
    assert_eq!(w.av.streams.running(), (0, 2), "both still taken");
}

#[test]
fn without_media_that_streams_sharing_is_off() {
    let w = World::with_media(Some(Arc::new(FakeMedia::new())));
    assert!(!w.svc.hub().chat.load().features.share);
}

#[test]
fn a_build_that_cannot_capture_windows_offers_no_streams() {
    let av = FakeAv::default();
    av.streams.set_no_capture(true);
    let w = World::with_media(Some(Arc::new(av)));
    assert!(!w.svc.hub().chat.load().features.share);
    assert!(World::new().svc.hub().chat.load().features.share);
}

#[test]
fn a_stream_that_never_connects_fails_after_the_connect_timeout() {
    let mut w = World::new();
    w.av.calls.set_short_waits(true);
    w.sharing_with_mira();
    w.signal(StreamSignal::Offer {
        friend: MIRA,
        sdp: "v=0 offer".into(),
        candidates: Vec::new(),
    });
    for _ in 0..10 {
        std::thread::sleep(Duration::from_millis(20));
        w.settle();
    }
    assert_eq!(w.failures(), vec![ErrorCode::ChtStreamFailed]);
    assert!(w.live().share.is_none());
    assert_eq!(w.streams(), vec![StreamRequest::End(MIRA)]);
}

#[test]
fn a_friend_who_shares_with_the_user_cannot_be_shared_with_at_the_same_time() {
    let mut w = World::new();
    w.run_game("Lantern");
    w.signal(StreamSignal::Invited(MIRA));
    w.cmd(ChatCommand::StartShare(ChatRef::Direct(MIRA)));
    assert_eq!(w.failures(), vec![ErrorCode::ChtStreamFailed]);
    assert!(w.av.streams.shares().is_empty());
}

#[test]
fn a_friends_app_that_lets_the_stream_go_before_it_connects_fails_the_share() {
    let mut w = World::new();
    w.sharing_with_mira();
    w.signal(StreamSignal::Ended(MIRA));
    assert_eq!(w.failures(), vec![ErrorCode::ChtStreamFailed]);
    assert!(w.live().share.is_none());
    assert!(
        w.streams().is_empty(),
        "their app ended it; nothing to say back"
    );
}

#[test]
fn changing_the_picture_reaches_the_running_share_and_is_kept_for_the_next() {
    let mut w = World::new();
    w.sharing_with_mira();
    w.cmd(ChatCommand::SetShareQuality {
        height: ShareHeight::P1440,
        fps: 60,
    });
    assert_eq!(w.av.streams.qualities(), vec![(Some(1440), 60)]);
    let prefs = w.svc.hub().prefs.load();
    assert_eq!(prefs.chat.share_quality, ShareHeight::P1440);
    assert_eq!(prefs.chat.share_fps.as_str(), "60");
    let live = w.live();
    assert_eq!(
        (live.stream.share_height, live.stream.share_fps),
        (Some(1440), 60)
    );
    // What the media reports it sends reaches the snapshot.
    let info = zaun_core::model::StreamInfo {
        encoder: zaun_core::model::VideoEncoding::Hardware,
        width: 2560,
        height: 1440,
        fps: 60,
    };
    w.share_says(StreamEvent::Sending(info));
    assert_eq!(w.live().stream.sending, Some(info));
    // The next share starts the same way.
    w.cmd(ChatCommand::StopShare);
    w.cmd(ChatCommand::StartShare(ChatRef::Direct(MIRA)));
    let next = w.av.streams.shares().pop().unwrap();
    assert_eq!((next.max_height, next.fps), (Some(1440), 60));
}

#[test]
fn only_30_or_60_frames_a_second_can_be_chosen() {
    let mut w = World::new();
    w.cmd(ChatCommand::SetShareQuality {
        height: ShareHeight::P720,
        fps: 45,
    });
    assert_eq!(w.failures(), vec![ErrorCode::CfgInvalidValue]);
    assert_eq!(
        w.svc.hub().prefs.load().chat.share_quality,
        ShareHeight::P1080
    );
}

#[test]
fn a_software_only_choice_reaches_the_media() {
    let mut w = World::new();
    w.send(Command::SetPreference(
        zaun_core::config::PreferencePatch::new().set("chat.share_hw_encode", false),
    ));
    w.run_game("Lantern");
    w.cmd(ChatCommand::StartShare(ChatRef::Direct(MIRA)));
    assert!(!w.av.streams.shares()[0].hardware);
}
