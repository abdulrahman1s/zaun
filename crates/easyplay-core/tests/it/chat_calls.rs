//! Voice calls in the service (ADR-0035) against the fake engine and the fake call media:
//! ringing out and in, answering, declining, timeouts, hanging up, blocking, one call at a
//! time, and signals of a call that is over.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use easyplay_core::artwork::steam::FsAppIdFile;
use easyplay_core::artwork::{ArtIndex, NoIcons};
use easyplay_core::chat::fake::FakeEngine;
use easyplay_core::chat::media::fake::FakeMedia;
use easyplay_core::chat::media::{
    CONNECT_TIMEOUT, CallRequest, CallSignal, MediaEvent, MicTestEvent, Negotiation, RING_GRACE,
    RING_TIMEOUT, TrackKind,
};
use easyplay_core::chat::{ChatEvent, Request};
use easyplay_core::clock::{Clock, SystemClock};
use easyplay_core::config::Preferences;
use easyplay_core::error::ErrorCode;
use easyplay_core::fsx::{Fs, MountTable, RealFs};
use easyplay_core::host::StaticHostFacts;
use easyplay_core::identity::{FsPeInspector, SharedLocations};
use easyplay_core::ids::RuntimeName;
use easyplay_core::jobs::{JobHandle, JobLabel, JobPool, JobPoolConfig, JobPriority};
use easyplay_core::launch::NoLocks;
use easyplay_core::library::draft::NoUmuTitles;
use easyplay_core::library::service::NoPolicyCheck;
use easyplay_core::model::{
    AudioDevice, AudioDevices, CallLink, ChatFeatures, ChatLive, ChatProfile, ChatRef, Friend,
    Game, RuntimeRef, SlrVariant,
};
use easyplay_core::procfs::RealProc;
use easyplay_core::runtime::{ProvisionNeed, Resolution, RuntimeCatalog, RuntimeService};
use easyplay_core::service::{AppService, ChatCommand, Command, Event, LaunchEnv, ServiceDeps};
use easyplay_testkit::TestEnv;
use easyplay_testkit::fakes::FakeLauncher;
use easyplay_testkit::fixtures::hosts;

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
    _env: TestEnv,
    svc: AppService,
    events: Receiver<Event>,
    engine: FakeEngine,
    media: FakeMedia,
}

fn deps(env: &TestEnv, engine: &FakeEngine, media: Option<&FakeMedia>) -> ServiceDeps {
    let fs: Arc<dyn Fs> = Arc::new(RealFs::new());
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let jobs = Arc::new(JobPool::new(JobPoolConfig::default(), Arc::clone(&clock)).unwrap());
    let mut prefs = Preferences::default();
    prefs.chat.enabled = true;
    prefs.chat.name = "Rin".into();
    prefs.chat.mic = "usb-mic".into();
    prefs.chat.mic_volume = 80;
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
        chat: Some(Arc::new(engine.clone())),
        overlay: None,
        media: media
            .map(|m| Arc::new(m.clone()) as Arc<dyn easyplay_core::chat::media::MediaFactory>),
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
    /// Chat on and ready, with two friends, and calls carried by the engine.
    fn new() -> Self {
        Self::build(true)
    }

    fn build(with_media: bool) -> Self {
        let env = TestEnv::new().unwrap();
        let engine = FakeEngine::new();
        let media = FakeMedia::new();
        let (svc, events) =
            AppService::open_headless(deps(&env, &engine, with_media.then_some(&media))).unwrap();
        let mut w = World {
            _env: env,
            svc,
            events,
            engine,
            media,
        };
        w.settle();
        w.engine_says(ChatEvent::Features {
            features: ChatFeatures {
                calls: true,
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

    fn cmd(&mut self, c: ChatCommand) {
        self.svc.handle(Command::Chat(c));
        self.settle();
    }

    fn engine_says(&mut self, e: ChatEvent) {
        self.engine.emit(e);
        self.settle();
    }

    fn signal(&mut self, s: CallSignal) {
        self.engine_says(ChatEvent::Call(s));
    }

    fn media_says(&mut self, e: MediaEvent) {
        self.media.emit(e);
        self.settle();
    }

    fn live(&self) -> ChatLive {
        self.svc.hub().chat.load().live.clone()
    }

    /// The call requests made since last asked.
    fn calls(&self) -> Vec<CallRequest> {
        self.engine
            .take_requests()
            .into_iter()
            .filter_map(|r| match r {
                Request::Call(c) => Some(c),
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

    /// Let timers that have a thousandth of their wait run out.
    fn wait_timers(&mut self) {
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(20));
            self.settle();
        }
    }

    /// Rin calls Mira, Mira answers, the sound gets through.
    fn in_call_with_mira(&mut self) {
        self.cmd(ChatCommand::Call(ChatRef::Direct(MIRA)));
        self.signal(CallSignal::Offer {
            friend: MIRA,
            sdp: "v=0 offer".into(),
            candidates: Vec::new(),
        });
        self.media_says(MediaEvent::Connected);
        self.calls();
    }
}

#[test]
fn calling_a_friend_rings_them_then_their_offer_is_answered_and_the_call_counts_from_connecting() {
    let mut w = World::new();
    w.cmd(ChatCommand::Call(ChatRef::Direct(MIRA)));
    assert_eq!(w.calls(), vec![CallRequest::Invite(MIRA)]);
    let live = w.live();
    let call = live.call.expect("the call card");
    assert_eq!(
        (call.since, call.with),
        (None, vec!["Mira Okafor".to_owned()])
    );
    assert_eq!(live.link, CallLink::Ringing);
    assert!(!w.media.running(), "no media before the friend answers");

    w.signal(CallSignal::Offer {
        friend: MIRA,
        sdp: "v=0 offer".into(),
        candidates: vec!["candidate:1 1 udp 1 10.0.0.2 4000 typ host".into()],
    });
    let start = w.media.starts().pop().expect("media started");
    assert_eq!(start.negotiation, Negotiation::Answer("v=0 offer".into()));
    assert_eq!(start.tracks, vec![TrackKind::Audio]);
    assert!(start.relay_only, "chat.relay_calls is on by default");
    assert_eq!(start.audio.mic, "usb-mic");
    assert_eq!(start.audio.mic_volume, 80, "the microphone volume");
    assert_eq!(start.ice.len(), 2, "the default servers");
    assert_eq!(w.media.candidates().len(), 1);
    assert_eq!(w.live().link, CallLink::Connecting);

    w.media_says(MediaEvent::Description("v=0 answer".into()));
    assert_eq!(
        w.calls(),
        vec![CallRequest::Answer {
            friend: MIRA,
            sdp: "v=0 answer".into()
        }]
    );
    w.media_says(MediaEvent::Connected);
    assert_eq!(w.calls(), vec![CallRequest::Connected(MIRA)]);
    let live = w.live();
    assert!(live.call.unwrap().since.is_some());
    assert_eq!(live.link, CallLink::Connected);
}

#[test]
fn an_incoming_call_rings_and_answering_sends_the_medias_offer_then_takes_the_answer() {
    let mut w = World::new();
    w.signal(CallSignal::Invited {
        friend: MIRA,
        usable: true,
    });
    let ringing = w.live().ringing.expect("the incoming card");
    assert_eq!(ringing.name, "Mira Okafor");
    assert_eq!(ringing.chat, ChatRef::Direct(MIRA));

    w.cmd(ChatCommand::Answer);
    let live = w.live();
    assert!(live.ringing.is_none() && live.call.is_some());
    assert_eq!(w.media.starts()[0].negotiation, Negotiation::Offer);
    w.media_says(MediaEvent::Description("v=0 offer".into()));
    assert_eq!(
        w.calls(),
        vec![CallRequest::Offer {
            friend: MIRA,
            sdp: "v=0 offer".into()
        }]
    );
    w.signal(CallSignal::Answer {
        friend: MIRA,
        sdp: "v=0 answer".into(),
        candidates: vec!["candidate:x".into()],
    });
    assert_eq!(w.media.answers(), vec!["v=0 answer".to_owned()]);
    assert_eq!(w.media.candidates(), vec!["candidate:x".to_owned()]);
}

#[test]
fn ringing_unanswered_for_twenty_seconds_lets_the_call_go_as_missed_both_ways() {
    let mut w = World::new();
    w.media.set_short_waits(true);
    w.signal(CallSignal::Invited {
        friend: MIRA,
        usable: true,
    });
    w.wait_timers();
    assert!(w.live().ringing.is_none());
    assert_eq!(w.calls(), vec![CallRequest::End(MIRA)]);
    // The callee waits a little longer: the caller's own timeout normally ends it first.
    assert!(w.media.waits().contains(&(RING_TIMEOUT + RING_GRACE)));

    w.cmd(ChatCommand::Call(ChatRef::Direct(DEV)));
    w.wait_timers();
    assert!(w.live().call.is_none());
    assert_eq!(
        w.calls(),
        vec![CallRequest::Invite(DEV), CallRequest::End(DEV)]
    );
}

#[test]
fn an_answered_call_whose_sound_never_gets_through_fails_to_connect() {
    let mut w = World::new();
    w.signal(CallSignal::Invited {
        friend: MIRA,
        usable: true,
    });
    // Only the connect timeout is short (the ring timeout must not end the call first).
    w.media.set_short_waits(true);
    w.drain();
    w.cmd(ChatCommand::Answer);
    w.wait_timers();
    assert!(w.media.waits().contains(&CONNECT_TIMEOUT));
    assert!(w.live().call.is_none());
    assert!(!w.media.running());
    assert_eq!(w.calls(), vec![CallRequest::End(MIRA)]);
    assert_eq!(w.failures(), vec![ErrorCode::ChtCallFailed]);
}

#[test]
fn declining_ends_the_call_for_the_caller_too() {
    let mut w = World::new();
    w.signal(CallSignal::Invited {
        friend: MIRA,
        usable: true,
    });
    w.cmd(ChatCommand::Decline);
    assert!(w.live().ringing.is_none());
    assert_eq!(w.calls(), vec![CallRequest::Decline(MIRA)]);
    assert!(w.media.starts().is_empty());
}

#[test]
fn a_friend_declining_stops_the_users_ringing_at_once() {
    let mut w = World::new();
    w.cmd(ChatCommand::Call(ChatRef::Direct(MIRA)));
    w.calls();
    w.signal(CallSignal::Declined(MIRA));
    assert!(w.live().call.is_none());
    assert!(w.calls().is_empty(), "nothing to send back");
    assert!(w.failures().is_empty(), "a decline is no error");
}

#[test]
fn hanging_up_ends_the_call_and_stops_its_media() {
    let mut w = World::new();
    w.in_call_with_mira();
    assert!(w.media.running());
    w.cmd(ChatCommand::EndCall);
    assert_eq!(w.calls(), vec![CallRequest::End(MIRA)]);
    assert!(!w.media.running());
    assert!(w.live().call.is_none());
}

#[test]
fn the_friend_hanging_up_ends_the_call_without_a_word_back() {
    let mut w = World::new();
    w.in_call_with_mira();
    w.signal(CallSignal::Ended(MIRA));
    assert!(w.live().call.is_none() && !w.media.running());
    assert!(w.calls().is_empty());
}

#[test]
fn blocking_a_friend_ends_a_call_with_them_and_a_ringing_one_silently() {
    let mut w = World::new();
    w.in_call_with_mira();
    w.cmd(ChatCommand::SetBlocked {
        friend: MIRA,
        on: true,
    });
    assert_eq!(w.calls(), vec![CallRequest::End(MIRA)]);
    assert!(w.live().call.is_none() && !w.media.running());

    w.signal(CallSignal::Invited {
        friend: DEV,
        usable: true,
    });
    w.cmd(ChatCommand::SetBlocked {
        friend: DEV,
        on: true,
    });
    assert_eq!(w.calls(), vec![CallRequest::Reject(DEV)]);
    assert!(w.live().ringing.is_none());
}

#[test]
fn a_second_call_while_in_one_is_let_go_as_missed() {
    let mut w = World::new();
    w.in_call_with_mira();
    w.signal(CallSignal::Invited {
        friend: DEV,
        usable: true,
    });
    assert_eq!(w.calls(), vec![CallRequest::End(DEV)]);
    let live = w.live();
    assert!(live.ringing.is_none());
    assert_eq!(live.call.unwrap().chat, ChatRef::Direct(MIRA));
    // And the user cannot place a second one.
    w.cmd(ChatCommand::Call(ChatRef::Direct(DEV)));
    assert!(w.calls().is_empty());
}

#[test]
fn signals_and_timers_of_a_call_that_is_over_are_ignored() {
    let mut w = World::new();
    w.media.set_short_waits(true);
    w.cmd(ChatCommand::Call(ChatRef::Direct(MIRA)));
    w.cmd(ChatCommand::EndCall);
    w.media.set_short_waits(false);
    w.cmd(ChatCommand::Call(ChatRef::Direct(MIRA)));
    w.calls();
    // The first call's ring timeout runs out now: the second call keeps ringing.
    w.wait_timers();
    assert!(w.live().call.is_some());
    assert!(w.calls().is_empty());
    w.cmd(ChatCommand::EndCall);
    w.calls();
    // A late answer and a late hang-up of that call change nothing.
    w.signal(CallSignal::Answer {
        friend: MIRA,
        sdp: "v=0".into(),
        candidates: Vec::new(),
    });
    w.signal(CallSignal::Offer {
        friend: MIRA,
        sdp: "v=0".into(),
        candidates: Vec::new(),
    });
    w.signal(CallSignal::Ended(MIRA));
    assert!(w.media.starts().is_empty() && w.media.answers().is_empty());
    assert!(w.live().call.is_none() && w.calls().is_empty());
}

#[test]
fn a_call_from_another_app_is_let_go_as_missed_without_ringing() {
    let mut w = World::new();
    w.signal(CallSignal::Invited {
        friend: MIRA,
        usable: false,
    });
    assert!(w.live().ringing.is_none());
    assert_eq!(w.calls(), vec![CallRequest::End(MIRA)]);
}

#[test]
fn no_microphone_ends_the_call_being_answered_and_says_why() {
    let mut w = World::new();
    w.media.set_fail_start(Some(ErrorCode::ChtNoMicrophone));
    w.signal(CallSignal::Invited {
        friend: MIRA,
        usable: true,
    });
    w.drain();
    w.cmd(ChatCommand::Answer);
    assert_eq!(w.failures(), vec![ErrorCode::ChtNoMicrophone]);
    assert_eq!(w.calls(), vec![CallRequest::End(MIRA)]);
    assert!(w.live().call.is_none() && w.live().ringing.is_none());
}

#[test]
fn a_media_failure_during_the_call_ends_it_and_says_why() {
    let mut w = World::new();
    w.in_call_with_mira();
    w.drain();
    w.media_says(MediaEvent::Failed {
        code: ErrorCode::ChtMediaFailed,
        detail: "the speakers went away".into(),
    });
    assert_eq!(w.failures(), vec![ErrorCode::ChtMediaFailed]);
    assert_eq!(w.calls(), vec![CallRequest::End(MIRA)]);
    assert!(w.live().call.is_none());
}

#[test]
fn mute_and_deafen_reach_the_media_and_the_card() {
    let mut w = World::new();
    w.in_call_with_mira();
    w.media_says(MediaEvent::Levels { me: 40, friend: 60 });
    assert_eq!(
        w.live().call.unwrap().speaking,
        vec!["Mira Okafor".to_owned()]
    );
    w.cmd(ChatCommand::CallMute(true));
    w.cmd(ChatCommand::CallDeafen(true));
    assert_eq!(w.media.controls(), (true, true));
    let call = w.live().call.unwrap();
    assert!(call.muted && call.deafened);
    assert!(
        call.speaking.is_empty(),
        "a deafened user hears nobody speak"
    );
}

#[test]
fn the_call_paths_quality_reaches_the_card_and_goes_with_the_call() {
    let mut w = World::new();
    w.in_call_with_mira();
    assert_eq!(w.live().quality, None, "nothing measured yet");
    w.media_says(MediaEvent::Quality {
        rtt_ms: 180,
        loss: 4,
    });
    let q = w.live().quality.expect("measured");
    assert_eq!((q.rtt_ms, q.loss), (180, 4));
    w.cmd(ChatCommand::EndCall);
    assert_eq!(w.live().quality, None);
}

#[test]
fn turning_chat_off_drops_the_call() {
    let mut w = World::new();
    w.in_call_with_mira();
    w.cmd(ChatCommand::SetEnabled {
        on: false,
        name: None,
    });
    assert!(!w.media.running());
    assert!(w.live().call.is_none());
}

#[test]
fn without_call_media_calls_are_off_and_an_incoming_call_is_let_go() {
    let mut w = World::build(false);
    assert!(!w.svc.hub().chat.load().features.calls);
    w.cmd(ChatCommand::Call(ChatRef::Direct(MIRA)));
    assert!(w.calls().is_empty());
    w.signal(CallSignal::Invited {
        friend: MIRA,
        usable: true,
    });
    assert!(w.live().ringing.is_none());
    assert_eq!(w.calls(), vec![CallRequest::End(MIRA)]);
}

#[test]
fn a_group_call_is_refused_with_a_reason() {
    let mut w = World::new();
    w.cmd(ChatCommand::Call(ChatRef::Group(9)));
    assert_eq!(w.failures(), vec![ErrorCode::ChtCallFailed]);
    assert!(w.calls().is_empty());
}

#[test]
fn the_device_list_is_read_on_the_job_pool_into_the_snapshot() {
    let mut w = World::new();
    let devices = AudioDevices {
        inputs: vec![AudioDevice {
            name: "alsa_input.usb".into(),
            description: "USB headset".into(),
        }],
        outputs: Vec::new(),
    };
    w.media.set_devices(devices.clone());
    assert_eq!(w.svc.hub().chat.load().audio_devices, None);
    w.cmd(ChatCommand::RefreshAudioDevices);
    w.wait_timers();
    assert_eq!(w.svc.hub().chat.load().audio_devices, Some(devices));
}

#[test]
fn the_mic_test_opens_the_chosen_mic_only_when_asked_and_reports_its_level() {
    let mut w = World::new();
    assert!(w.media.mic_tests().is_empty(), "never opened unasked");
    w.cmd(ChatCommand::MicTest(true));
    assert!(w.media.mic_test_running());
    let settings = &w.media.mic_tests()[0];
    assert_eq!(
        (settings.mic.as_str(), settings.mic_volume),
        ("usb-mic", 80)
    );
    assert_eq!(w.live().mic_test, Some(0));
    w.media.emit_mic(MicTestEvent::Level(40));
    w.settle();
    assert_eq!(w.live().mic_test, Some(40));
    w.cmd(ChatCommand::MicTest(false));
    assert!(!w.media.mic_test_running());
    assert_eq!(w.live().mic_test, None);
    // A late level of that test changes nothing.
    w.media.emit_mic(MicTestEvent::Level(70));
    w.settle();
    assert_eq!(w.live().mic_test, None);
}

#[test]
fn starting_a_call_stops_the_mic_test_and_none_starts_during_a_call() {
    let mut w = World::new();
    w.cmd(ChatCommand::MicTest(true));
    w.cmd(ChatCommand::Call(ChatRef::Direct(MIRA)));
    assert!(!w.media.mic_test_running());
    assert_eq!(w.live().mic_test, None);
    w.cmd(ChatCommand::MicTest(true));
    assert!(!w.media.mic_test_running(), "the call has the devices");
    w.cmd(ChatCommand::EndCall);
    // Ringing in stops it too.
    w.cmd(ChatCommand::MicTest(true));
    w.signal(CallSignal::Invited {
        friend: DEV,
        usable: true,
    });
    assert!(!w.media.mic_test_running());
}

#[test]
fn a_mic_test_failure_stops_it_and_says_why() {
    let mut w = World::new();
    w.cmd(ChatCommand::MicTest(true));
    w.drain();
    w.media.emit_mic(MicTestEvent::Failed {
        code: ErrorCode::ChtNoMicrophone,
        detail: "unplugged".into(),
    });
    w.settle();
    assert_eq!(w.failures(), vec![ErrorCode::ChtNoMicrophone]);
    assert!(!w.media.mic_test_running() && w.live().mic_test.is_none());
}
