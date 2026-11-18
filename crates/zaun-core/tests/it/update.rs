//! Updates (ADR-0040): signatures, the manifest's rules, downloads that differ from what was
//! signed, no downgrades, how a copy was installed, the daily throttle, staging, apply and
//! rollback on `MemFs`, and the service's flows (consent, the announcement, nothing while a
//! game runs). No test reaches the network: a fake transport serves a release made by
//! `scripts/sign-release-manifest.sh` with a throwaway test key.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use ring::signature::{Ed25519KeyPair, KeyPair as _};
use zaun_core::artwork::http::{
    HopLimits, HttpClient, HttpError, HttpsUrl, RawResponse, Transport,
};
use zaun_core::artwork::steam::FsAppIdFile;
use zaun_core::artwork::{ArtIndex, NoIcons};
use zaun_core::clock::{Clock, SystemClock, Timestamp};
use zaun_core::config::Preferences;
use zaun_core::error::ErrorCode;
use zaun_core::fsx::{Fs, MountTable, RealFs};
use zaun_core::host::StaticHostFacts;
use zaun_core::identity::{FsPeInspector, SharedLocations};
use zaun_core::jobs::{JobHandle, JobLabel, JobPool, JobPoolConfig, JobPriority};
use zaun_core::launch::NoLocks;
use zaun_core::library::draft::NoUmuTitles;
use zaun_core::library::service::NoPolicyCheck;
use zaun_core::model::{Game, InstallChannel, RuntimeRef, UpdateDownload};
use zaun_core::procfs::RealProc;
use zaun_core::runtime::{Resolution, RuntimeCatalog, RuntimeService, UnavailableReason};
use zaun_core::service::{
    AppService, Command, Event, LaunchEnv, Notice, ServiceDeps, UpdateCommand,
};
use zaun_core::storage::{Db, GameRepo, SessionRepo};
use zaun_core::update::check::fetch_release;
use zaun_core::update::source::{MANIFEST_URL, SIGNATURE_URL};
use zaun_core::update::trial::{self, TrialStart};
use zaun_core::update::{
    Install, Layout, UpdateDeps, UpdateError, UpdateState, Version, apply, detect, stage, verify,
};
use zaun_testkit::TestEnv;
use zaun_testkit::fakes::{CannedOutput, FakeClock, FakeLauncher, MemFs, ScriptedRunner};
use zaun_testkit::fixtures::{games, hosts};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/update");
const PREFIX: &str = "/home/sam/.local";
const GZ_ZAUN: &str = "zaun-9.9.9-x86_64-linux.zaun.gz";
const GZ_INIT: &str = "zaun-9.9.9-x86_64-linux.zaun-init.gz";

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(FIXTURE).join(name)).unwrap()
}

/// The public half of the throwaway key the fixture was signed with.
fn test_key() -> [u8; 32] {
    let hex = String::from_utf8(fixture("test-key.pub.hex")).unwrap();
    let bytes: Vec<u8> = (0..64)
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    bytes.try_into().unwrap()
}

fn download_url(file: &str) -> String {
    format!("https://github.com/abdulrahman1s/zaun/releases/download/v9.9.9/{file}")
}

/// Serves a fixed set of URLs and counts every request.
#[derive(Default)]
struct Net {
    files: Mutex<HashMap<String, Vec<u8>>>,
    calls: AtomicUsize,
}

impl Net {
    fn release() -> Arc<Self> {
        let n = Arc::new(Self::default());
        n.serve(MANIFEST_URL, fixture("manifest.json"));
        n.serve(SIGNATURE_URL, fixture("manifest.json.sig"));
        n.serve(&download_url(GZ_ZAUN), fixture(GZ_ZAUN));
        n.serve(&download_url(GZ_INIT), fixture(GZ_INIT));
        n
    }

    fn serve(&self, url: &str, body: Vec<u8>) {
        self.files.lock().unwrap().insert(url.to_owned(), body);
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Transport for Net {
    fn get(&self, url: &HttpsUrl, limits: HopLimits) -> Result<RawResponse, HttpError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let files = self.files.lock().unwrap();
        let Some(body) = files.get(&url.to_string()) else {
            return Ok(RawResponse {
                status: 404,
                location: None,
                body: Vec::new(),
            });
        };
        if body.len() as u64 > limits.max_body {
            return Err(HttpError::TooLarge);
        }
        Ok(RawResponse {
            status: 200,
            location: None,
            body: body.clone(),
        })
    }
}

fn base(net: &Arc<Net>) -> HttpClient {
    HttpClient::new(
        Arc::clone(net) as Arc<dyn Transport>,
        Arc::new(FakeClock::baseline()),
    )
}

fn running() -> Version {
    Version::parse(zaun_core::consts::APP_VERSION).unwrap()
}

/// A tarball install under `/home/sam/.local`, as `install.sh` leaves it.
fn owned_install() -> (MemFs, Layout) {
    let fs = MemFs::new();
    fs.file(format!("{PREFIX}/libexec/zaun/zaun"), b"old zaun")
        .file(format!("{PREFIX}/libexec/zaun/zaun-init"), b"old init")
        .file(
            format!("{PREFIX}/libexec/zaun/install-manifest"),
            b"f bin/zaun\n",
        );
    (
        fs,
        Layout {
            prefix: PathBuf::from(PREFIX),
        },
    )
}

fn smoke_ok(layout: &Layout) -> ScriptedRunner {
    let staged = layout.staged().join("zaun");
    ScriptedRunner::new().expect(
        &[staged.as_os_str(), "--version".as_ref()],
        CannedOutput::ok("zaun 9.9.9 (0123456789ab)\n"),
    )
}

fn installed(fs: &MemFs, layout: &Layout) -> (Vec<u8>, Vec<u8>) {
    (
        fs.contents(layout.target("libexec/zaun/zaun")).unwrap(),
        fs.contents(layout.target("libexec/zaun/zaun-init"))
            .unwrap(),
    )
}

/// Sign `json` with a key made from `seed` (tests only): (manifest, signature, public key).
fn signed(json: &str, seed: u8) -> (Vec<u8>, Vec<u8>, [u8; 32]) {
    let kp = Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap();
    let sig = base64::engine::general_purpose::STANDARD.encode(kp.sign(json.as_bytes()));
    let public: [u8; 32] = kp.public_key().as_ref().try_into().unwrap();
    (json.as_bytes().to_vec(), sig.into_bytes(), public)
}

fn manifest_json(product: &str, url: &str, notes_url: &str) -> String {
    let sha = "a".repeat(64);
    format!(
        r#"{{"format":1,"product":"{product}","version":"9.9.9","date":"2026-11-18","notes":"Line one\nline\u0007two","notes_url":"{notes_url}","artifacts":[{{"channel":"tarball","arch":"x86_64","path":"libexec/zaun/zaun","url":"{url}","size":1,"sha256":"{sha}","unpacked_size":1,"unpacked_sha256":"{sha}"}}]}}"#
    )
}

// ---- signatures and the manifest ---------------------------------------------------------

#[test]
fn a_manifest_signed_by_the_release_script_verifies() {
    let r = verify(
        &fixture("manifest.json"),
        &fixture("manifest.json.sig"),
        &[test_key()],
    )
    .unwrap();
    assert_eq!(r.version.to_string(), "9.9.9");
    assert_eq!(r.files("tarball", "x86_64").unwrap().len(), 2);
    assert_eq!(
        r.notes_url.as_deref(),
        Some("https://github.com/abdulrahman1s/zaun/releases/tag/v9.9.9")
    );
}

#[test]
fn a_changed_manifest_a_bad_signature_or_another_key_is_refused() {
    let mut changed = fixture("manifest.json");
    let at = changed.iter().position(|b| *b == b'9').unwrap();
    changed[at] = b'8';
    let sig = fixture("manifest.json.sig");
    let bad = |r: Result<_, UpdateError>| matches!(r, Err(UpdateError::BadSignature));
    assert!(bad(verify(&changed, &sig, &[test_key()])));
    assert!(bad(verify(
        &fixture("manifest.json"),
        b"not base64!",
        &[test_key()]
    )));
    assert!(bad(verify(&fixture("manifest.json"), &sig, &[[7; 32]])));
    // The compiled-in release key did not sign the test release.
    assert!(bad(verify(
        &fixture("manifest.json"),
        &sig,
        zaun_core::update::TRUSTED_KEYS
    )));
}

#[test]
fn without_a_key_nothing_is_requested() {
    let net = Net::release();
    let r = fetch_release(&UpdateDeps::check_client(&base(&net)), &[]);
    assert!(matches!(r, Err(UpdateError::NotConfigured)));
    assert_eq!(net.calls(), 0);
}

#[test]
fn signed_manifests_still_follow_the_rules() {
    let ok_url = download_url("x.gz");
    let (m, s, k) = signed(
        &manifest_json("zaun", &ok_url, "https://evil.example/notes"),
        1,
    );
    let r = verify(&m, &s, &[k]).unwrap();
    assert_eq!(
        r.notes, "Line one line two",
        "control characters are removed"
    );
    assert_eq!(
        r.notes_url, None,
        "notes outside the repository are dropped"
    );
    let refused = |json: String| {
        let (m, s, k) = signed(&json, 2);
        matches!(verify(&m, &s, &[k]), Err(UpdateError::BadManifest(_)))
    };
    assert!(refused(manifest_json("other", &ok_url, "")));
    assert!(refused(manifest_json(
        "zaun",
        "https://evil.example/zaun.gz",
        ""
    )));
    assert!(refused(manifest_json(
        "zaun",
        "https://github.com/abdulrahman1s/zaun/releases/download/../../x",
        ""
    )));
    assert!(refused(
        manifest_json("zaun", &ok_url, "").replace("libexec/zaun/zaun\"", "bin/sh\"")
    ));
}

// ---- download, stage, apply, roll back ---------------------------------------------------

#[test]
fn a_download_that_differs_from_its_sha256_is_refused_and_nothing_is_staged() {
    let net = Net::release();
    let mut gz = fixture(GZ_ZAUN);
    let last = gz.len() - 1;
    gz[last] ^= 1;
    net.serve(&download_url(GZ_ZAUN), gz);
    let release = fetch_release(&UpdateDeps::check_client(&base(&net)), &[test_key()]).unwrap();
    let (fs, layout) = owned_install();
    let r = stage::download(
        &UpdateDeps::files_client(&base(&net)),
        &fs,
        &layout,
        &release,
        "x86_64",
    );
    assert!(matches!(r, Err(UpdateError::BadFile { .. })), "{r:?}");
    assert!(!fs.exists(&layout.staged()));
    assert_eq!(
        installed(&fs, &layout),
        (b"old zaun".to_vec(), b"old init".to_vec())
    );
}

#[test]
fn a_file_larger_than_signed_is_never_read_whole() {
    let net = Net::release();
    net.serve(&download_url(GZ_ZAUN), vec![0; 4096]);
    let release = fetch_release(&UpdateDeps::check_client(&base(&net)), &[test_key()]).unwrap();
    let (fs, layout) = owned_install();
    let r = stage::download(
        &UpdateDeps::files_client(&base(&net)),
        &fs,
        &layout,
        &release,
        "x86_64",
    );
    assert!(matches!(r, Err(UpdateError::BadFile { .. })), "{r:?}");
}

fn staged_install() -> (MemFs, Layout) {
    let net = Net::release();
    let release = fetch_release(&UpdateDeps::check_client(&base(&net)), &[test_key()]).unwrap();
    let (fs, layout) = owned_install();
    stage::download(
        &UpdateDeps::files_client(&base(&net)),
        &fs,
        &layout,
        &release,
        "x86_64",
    )
    .unwrap();
    (fs, layout)
}

#[test]
fn apply_replaces_both_files_keeps_the_previous_and_starts_a_trial() {
    let (fs, layout) = staged_install();
    assert!(fs.exists(&layout.staged().join("manifest.json")));
    let runner = smoke_ok(&layout);
    let v = apply::apply(&fs, &runner, &layout, &[test_key()], &running(), "x86_64").unwrap();
    assert_eq!(v.map(|v| v.to_string()).as_deref(), Some("9.9.9"));
    assert_eq!(
        installed(&fs, &layout),
        (
            b"zaun 9.9.9 (fixture binary)\n".to_vec(),
            b"zaun-init 9.9.9 (fixture shim)\n".to_vec()
        )
    );
    assert_eq!(
        fs.contents(layout.previous().join("zaun")).unwrap(),
        b"old zaun"
    );
    assert_eq!(
        fs.contents(layout.previous().join("zaun-init")).unwrap(),
        b"old init"
    );
    assert!(fs.exists(&layout.trial_file()));
    assert!(!fs.exists(&layout.staged()));
    assert!(
        runner.unused().is_empty(),
        "the staged binary was smoke-tested"
    );
}

#[test]
fn an_older_or_equal_release_is_never_installed() {
    let (fs, layout) = staged_install();
    let same = Version::parse("9.9.9").unwrap();
    let r = apply::apply(
        &fs,
        &smoke_ok(&layout),
        &layout,
        &[test_key()],
        &same,
        "x86_64",
    );
    assert!(matches!(r, Err(UpdateError::NotNewer { .. })), "{r:?}");
    assert_eq!(
        installed(&fs, &layout),
        (b"old zaun".to_vec(), b"old init".to_vec())
    );
    assert!(!fs.exists(&layout.staged()), "a refused update is dropped");
}

#[test]
fn a_staged_file_changed_after_the_download_is_refused() {
    let (fs, layout) = staged_install();
    fs.file(layout.staged().join("zaun"), b"something else");
    let r = apply::apply(
        &fs,
        &smoke_ok(&layout),
        &layout,
        &[test_key()],
        &running(),
        "x86_64",
    );
    assert!(matches!(r, Err(UpdateError::BadFile { .. })), "{r:?}");
    assert_eq!(
        installed(&fs, &layout),
        (b"old zaun".to_vec(), b"old init".to_vec())
    );
}

#[test]
fn a_staged_binary_that_does_not_run_installs_nothing() {
    let (fs, layout) = staged_install();
    let staged = layout.staged().join("zaun");
    let runner = ScriptedRunner::new().expect(
        &[staged.as_os_str(), "--version".as_ref()],
        CannedOutput::fail(127, "cannot execute"),
    );
    let r = apply::apply(&fs, &runner, &layout, &[test_key()], &running(), "x86_64");
    assert!(matches!(r, Err(UpdateError::Install(_))), "{r:?}");
    assert_eq!(
        installed(&fs, &layout),
        (b"old zaun".to_vec(), b"old init".to_vec())
    );
}

#[test]
fn the_third_start_without_a_confirmation_rolls_back() {
    let (fs, layout) = staged_install();
    apply::apply(
        &fs,
        &smoke_ok(&layout),
        &layout,
        &[test_key()],
        &running(),
        "x86_64",
    )
    .unwrap();
    let new = Version::parse("9.9.9").unwrap();
    assert_eq!(
        trial::on_start(&fs, &layout, &new).unwrap(),
        TrialStart::Counting
    );
    assert_eq!(
        trial::on_start(&fs, &layout, &new).unwrap(),
        TrialStart::Counting
    );
    assert_eq!(
        trial::on_start(&fs, &layout, &new).unwrap(),
        TrialStart::RolledBack {
            failed: "9.9.9".into()
        }
    );
    assert_eq!(
        installed(&fs, &layout),
        (b"old zaun".to_vec(), b"old init".to_vec())
    );
    assert!(!fs.exists(&layout.trial_file()));
    // The restored version starts without a trial.
    assert_eq!(
        trial::on_start(&fs, &layout, &running()).unwrap(),
        TrialStart::None
    );
}

#[test]
fn a_confirmed_version_drops_the_previous_one() {
    let (fs, layout) = staged_install();
    apply::apply(
        &fs,
        &smoke_ok(&layout),
        &layout,
        &[test_key()],
        &running(),
        "x86_64",
    )
    .unwrap();
    let new = Version::parse("9.9.9").unwrap();
    assert_eq!(
        trial::on_start(&fs, &layout, &new).unwrap(),
        TrialStart::Counting
    );
    assert!(trial::confirm(&fs, &layout, &new).unwrap());
    assert!(!fs.exists(&layout.previous()));
    assert!(!fs.exists(&layout.trial_file()));
    assert_eq!(
        trial::on_start(&fs, &layout, &new).unwrap(),
        TrialStart::None
    );
}

// ---- how the copy was installed, the throttle ----------------------------------------------

#[test]
fn the_channel_follows_where_the_binary_lives() {
    let fs = MemFs::new();
    fs.file("/usr/bin/zaun", b"")
        .file("/opt/zaun/target/debug/zaun", b"")
        .file("/usr/local/libexec/zaun/zaun", b"")
        .file("/usr/local/libexec/zaun/install-manifest", b"")
        .file("/home/sam/unpacked/libexec/zaun/zaun", b"")
        .file(format!("{PREFIX}/libexec/zaun/zaun"), b"")
        .file(format!("{PREFIX}/libexec/zaun/install-manifest"), b"");
    fs.chown("/usr/local/libexec/zaun", 0, 0)
        .chown("/usr/local/libexec/zaun/zaun", 0, 0);
    let channel = |p: &str| detect(&fs, Path::new(p), 1000).channel;
    assert_eq!(
        channel("/nix/store/abc-zaun-0.1.0/bin/.zaun-wrapped"),
        InstallChannel::Nix
    );
    assert_eq!(channel("/usr/bin/zaun"), InstallChannel::Package);
    assert_eq!(
        channel("/opt/zaun/target/debug/zaun"),
        InstallChannel::Other
    );
    assert_eq!(
        channel("/home/sam/unpacked/libexec/zaun/zaun"),
        InstallChannel::Other,
        "no install-manifest: not an install"
    );
    assert_eq!(
        channel("/usr/local/libexec/zaun/zaun"),
        InstallChannel::Tarball { owned: false }
    );
    let own = detect(&fs, &Path::new(PREFIX).join("libexec/zaun/zaun"), 1000);
    assert_eq!(own.channel, InstallChannel::Tarball { owned: true });
    assert_eq!(
        own.layout(),
        Some(Layout {
            prefix: PathBuf::from(PREFIX)
        })
    );
    assert_eq!(detect(&fs, Path::new("/usr/bin/zaun"), 1000).layout(), None);
    fs.file("/.flatpak-info", b"");
    assert_eq!(channel("/app/bin/zaun"), InstallChannel::Flatpak);
}

#[test]
fn automatic_checks_are_due_once_a_day() {
    let t0 = Timestamp::from_millis(1_794_441_600_000);
    let hour = Duration::from_secs(3600);
    let mut st = UpdateState::default();
    assert!(st.due(t0));
    st.last_checked = Some(t0);
    assert!(!st.due(t0.saturating_add(23 * hour)));
    assert!(st.due(t0.saturating_add(24 * hour)));
    assert!(
        st.due(t0.saturating_sub(hour)),
        "a clock that went back checks again"
    );
}

// ---- the service ----------------------------------------------------------------------------

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
        Resolution::Unavailable(UnavailableReason::ExternalInvalid(PathBuf::from("/none")))
    }
}

struct World {
    env: TestEnv,
    svc: AppService,
    events: Receiver<Event>,
    net: Arc<Net>,
    layout: Layout,
}

impl World {
    fn new(check: bool) -> Self {
        let env = TestEnv::new().unwrap();
        let net = Net::release();
        let prefix = env.home().join(".local");
        for (f, body) in [
            ("zaun", "old zaun"),
            ("zaun-init", "old init"),
            ("install-manifest", ""),
        ] {
            let p = prefix.join("libexec/zaun").join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        let layout = Layout {
            prefix: prefix.clone(),
        };
        let fs: Arc<dyn Fs> = Arc::new(RealFs::new());
        let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
        let jobs = Arc::new(JobPool::new(JobPoolConfig::default(), Arc::clone(&clock)).unwrap());
        let mut prefs = Preferences::default();
        prefs.updates.check = check;
        let b = base(&net);
        let staged = layout.staged().join("zaun");
        let updates = UpdateDeps {
            check: Arc::new(UpdateDeps::check_client(&b)),
            files: Arc::new(UpdateDeps::files_client(&b)),
            keys: vec![test_key()],
            install: Install {
                channel: InstallChannel::Tarball { owned: true },
                prefix: Some(prefix),
            },
            runner: Arc::new(ScriptedRunner::new().expect(
                &[staged.as_os_str(), "--version".as_ref()],
                CannedOutput::ok("zaun 9.9.9\n"),
            )),
            arch: "x86_64".into(),
        };
        let deps = ServiceDeps {
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
            chat: None,
            overlay: None,
            media: None,
            updates: Some(updates),
        };
        let (svc, events) = AppService::open_headless(deps).unwrap();
        let mut w = Self {
            env,
            svc,
            events,
            net,
            layout,
        };
        w.settle();
        w
    }

    fn settle(&mut self) {
        while self.svc.pump(Duration::from_millis(300)) {}
    }

    fn drain(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    fn send(&mut self, c: UpdateCommand) -> Vec<Event> {
        self.svc.handle(Command::Updates(c));
        self.settle();
        self.drain()
    }

    fn announced(evs: &[Event]) -> Vec<String> {
        evs.iter()
            .filter_map(|e| match e {
                Event::Notice(Notice::UpdateAvailable { version }) => Some(version.clone()),
                _ => None,
            })
            .collect()
    }

    fn failure(evs: &[Event]) -> Option<ErrorCode> {
        evs.iter().find_map(|e| match e {
            Event::Failure(u) => Some(u.code),
            _ => None,
        })
    }
}

#[test]
fn with_checks_off_nothing_is_requested() {
    let mut w = World::new(false);
    assert_eq!(w.net.calls(), 0, "no automatic check");
    let evs = w.send(UpdateCommand::Check);
    assert_eq!(World::failure(&evs), Some(ErrorCode::UpdChecksOff));
    let evs = w.send(UpdateCommand::Download);
    assert_eq!(World::failure(&evs), Some(ErrorCode::UpdChecksOff));
    assert_eq!(w.net.calls(), 0, "not even on request");
    let u = w.svc.hub().updates.load();
    assert!(u.configured);
    assert_eq!(u.available, None);
}

#[test]
fn a_newer_version_is_found_at_start_and_announced_once() {
    let mut w = World::new(true);
    let evs = w.drain();
    assert_eq!(World::announced(&evs), vec!["9.9.9".to_owned()]);
    assert_eq!(w.net.calls(), 2, "the manifest and its signature");
    let u = w.svc.hub().updates.load();
    assert_eq!(
        u.available.as_ref().map(|a| a.version.as_str()),
        Some("9.9.9")
    );
    assert!(u.last_checked.is_some());
    let evs = w.send(UpdateCommand::Check);
    assert!(
        World::announced(&evs).is_empty(),
        "the same version is not announced again"
    );
    // The throttle and the announcement survive a restart through the state file.
    let st = UpdateState::load(
        &RealFs::new(),
        &zaun_core::update::state::state_file(&w.env.paths()),
    );
    assert_eq!(st.announced.as_deref(), Some("9.9.9"));
}

#[test]
fn download_stages_and_restart_waits_while_a_game_runs() {
    let mut w = World::new(true);
    w.drain();
    let evs = w.send(UpdateCommand::Download);
    assert_eq!(World::failure(&evs), None);
    assert_eq!(
        w.svc.hub().updates.load().download,
        UpdateDownload::Staged {
            version: "9.9.9".into()
        }
    );
    assert!(w.layout.staged().join("zaun").exists());

    // A running game: a session row whose supervisor is this test process.
    let id = {
        let draft_game = games::game(1, 1);
        let mut db = Db::open_rw(&w.env.paths().db_file).unwrap();
        db.insert_game(&draft_game).unwrap();
        let mut s = games::session(&draft_game, 1, 1, SystemClock::new().now());
        s.ended_at = None;
        s.end_reason = None;
        s.duration_ms = None;
        s.state = zaun_core::model::SessionState::Running;
        let me =
            zaun_core::session::sys::own_identity(&RealProc::new(Arc::new(RealFs::new()))).unwrap();
        s.supervisor = Some(zaun_core::model::SupervisorIdent {
            pid: me.pid,
            start_ticks: me.start_time_ticks,
            boot_id: String::new(),
        });
        db.insert_session(&s).unwrap();
        s.id
    };
    w.svc.poll_sessions_now();
    w.settle();
    w.drain();
    let evs = w.send(UpdateCommand::Restart);
    assert_eq!(World::failure(&evs), Some(ErrorCode::UpdGameRunning));
    assert!(!w.layout.restart_file().exists());
    assert!(!evs.iter().any(|e| matches!(e, Event::QuitRequested)));
    assert_eq!(
        std::fs::read(w.layout.target("libexec/zaun/zaun")).unwrap(),
        b"old zaun"
    );

    // The game ended: restart leaves the note for the process and closes the window.
    let mut db = Db::open_rw(&w.env.paths().db_file).unwrap();
    assert!(db.delete_session(id, &[]).unwrap());
    drop(db);
    w.svc.poll_sessions_now();
    w.settle();
    let evs = w.send(UpdateCommand::Restart);
    assert_eq!(World::failure(&evs), None);
    assert!(w.layout.restart_file().exists());
    assert!(evs.iter().any(|e| matches!(e, Event::QuitRequested)));
}
