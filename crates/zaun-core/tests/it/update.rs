//! Updates (ADR-0040, ADR-0041): signatures, the manifest's rules, downloads that differ from
//! what was signed, no downgrades, how a copy was installed, the daily throttle, the bundle
//! store (staging, switching, the installed copy's choice, trials and rollback) on `MemFs`,
//! and the service's flows (consent, the announcement and the background download, nothing
//! while a game runs). No test reaches the network: a fake transport serves a release made by
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
    HopLimits, HttpClient, HttpError, HttpsUrl, RawResponse, Transport, UreqTransport,
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
use zaun_core::update::install::GENERIC_LOADER;
use zaun_core::update::source::{MANIFEST_URL, SIGNATURE_URL};
use zaun_core::update::trial::{self, Choice};
use zaun_core::update::{
    Install, Release, Store, UpdateDeps, UpdateError, UpdateState, Version, apply, detect, stage,
    verify,
};
use zaun_testkit::fakes::{CannedOutput, FakeClock, FakeLauncher, MemFs, ScriptedRunner};
use zaun_testkit::fixtures::{games, hosts};
use zaun_testkit::{FakeHttpServer, Reply, TestEnv};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/update");
const DATA: &str = "/home/sam/.local/share/zaun";
const GZ_ZAUN: &str = "zaun-9.9.9-x86_64-linux.zaun.gz";
const FILES: [&str; 5] = [
    "zaun-9.9.9-x86_64-linux.zaun.gz",
    "zaun-9.9.9-x86_64-linux.zaun-init.gz",
    "zaun-9.9.9-x86_64-linux.zaun-overlay.gz",
    "zaun-9.9.9-x86_64-linux.zaun-chat.gz",
    "zaun-simplex-7.0.3-x86_64.tar.gz",
];
/// The old Chat engine's folder, still in the manifest for copies up to 0.5.0.
const ENGINE: &str = "zaun-simplex-7.0.3-x86_64.tar.gz";

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
    asked: Mutex<Vec<String>>,
}

impl Net {
    fn release() -> Arc<Self> {
        let n = Arc::new(Self::default());
        n.serve(MANIFEST_URL, fixture("bundle.json"));
        n.serve(SIGNATURE_URL, fixture("bundle.json.sig"));
        for f in FILES {
            n.serve(&download_url(f), fixture(f));
        }
        n
    }

    fn serve(&self, url: &str, body: Vec<u8>) {
        self.files.lock().unwrap().insert(url.to_owned(), body);
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn asked_for(&self, file: &str) -> usize {
        let url = download_url(file);
        self.asked
            .lock()
            .unwrap()
            .iter()
            .filter(|u| **u == url)
            .count()
    }
}

impl Transport for Net {
    fn get(&self, url: &HttpsUrl, limits: HopLimits) -> Result<RawResponse, HttpError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.asked.lock().unwrap().push(url.to_string());
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

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn store() -> Store {
    Store {
        versions: Path::new(DATA).join("versions"),
        components: Path::new(DATA).join("components"),
    }
}

/// A home with a data folder and nothing in the store yet.
fn empty_store() -> (MemFs, Store) {
    let fs = MemFs::new();
    fs.dir(DATA);
    (fs, store())
}

fn smoke_ok(store: &Store) -> ScriptedRunner {
    let bin = store.versions.join("9.9.9/zaun");
    ScriptedRunner::new().expect(
        &[bin.as_os_str(), "--version".as_ref()],
        CannedOutput::ok("zaun 9.9.9 (0123456789ab)\n"),
    )
}

fn release(net: &Arc<Net>) -> Release {
    fetch_release(&UpdateDeps::check_client(&base(net)), &[test_key()]).unwrap()
}

fn staged_store() -> (MemFs, Store, Arc<Net>) {
    let net = Net::release();
    let (fs, store) = empty_store();
    stage::download(
        &UpdateDeps::files_client(&base(&net)),
        &fs,
        &store,
        &release(&net),
        "x86_64",
    )
    .unwrap();
    (fs, store, net)
}

/// An older version, current already, with the old Chat engine's folder.
fn with_current(fs: &MemFs, store: &Store, version: &str) {
    let dir = store.versions.join(version);
    fs.file(dir.join("zaun"), format!("zaun {version}\n"));
    fs.file(
        store.components.join("simplex-old/libsimplex.so"),
        b"old engine",
    );
    fs.link(dir.join("simplex"), "../../components/simplex-old");
    fs.link(store.current(), version);
    fs.set_mode(&dir.join("zaun"), 0o755).unwrap();
}

fn linked(fs: &MemFs, link: &Path) -> Option<String> {
    fs.read_link(link)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
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
        r#"{{"format":2,"product":"{product}","version":"9.9.9","date":"2026-11-18","notes":"Line one\nline\u0007two","notes_url":"{notes_url}","artifacts":[{{"channel":"bundle","arch":"x86_64","path":"zaun","url":"{url}","size":1,"sha256":"{sha}","unpacked_size":1,"unpacked_sha256":"{sha}"}}]}}"#
    )
}

// ---- signatures and the manifest ---------------------------------------------------------

#[test]
fn a_bundle_manifest_signed_by_the_release_script_verifies() {
    let r = verify(
        &fixture("bundle.json"),
        &fixture("bundle.json.sig"),
        &[test_key()],
    )
    .unwrap();
    assert_eq!(r.version.to_string(), "9.9.9");
    let files = r.files("x86_64").unwrap();
    assert_eq!(
        files.iter().map(|a| a.path.as_str()).collect::<Vec<_>>(),
        ["zaun", "zaun-init", "zaun-overlay", "zaun-chat"],
        "the old Chat engine's folder is in the manifest only for older copies"
    );
    assert!(
        r.files("aarch64").is_err(),
        "no bundle for another architecture"
    );
    assert_eq!(
        r.notes_url.as_deref(),
        Some("https://github.com/abdulrahman1s/zaun/releases/tag/v9.9.9")
    );
}

#[test]
fn the_format_1_manifest_of_older_copies_is_not_read() {
    let r = verify(
        &fixture("manifest.json"),
        &fixture("manifest.json.sig"),
        &[test_key()],
    );
    assert!(matches!(r, Err(UpdateError::BadManifest(_))), "{r:?}");
}

#[test]
fn a_changed_manifest_a_bad_signature_or_another_key_is_refused() {
    let mut changed = fixture("bundle.json");
    let at = changed.iter().position(|b| *b == b'9').unwrap();
    changed[at] = b'8';
    let sig = fixture("bundle.json.sig");
    let bad = |r: Result<_, UpdateError>| matches!(r, Err(UpdateError::BadSignature));
    assert!(bad(verify(&changed, &sig, &[test_key()])));
    assert!(bad(verify(
        &fixture("bundle.json"),
        b"not base64!",
        &[test_key()]
    )));
    assert!(bad(verify(&fixture("bundle.json"), &sig, &[[7; 32]])));
    // The compiled-in release key did not sign the test release.
    assert!(bad(verify(
        &fixture("bundle.json"),
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
        manifest_json("zaun", &ok_url, "").replace("\"path\":\"zaun\"", "\"path\":\"../bin/sh\"")
    ));
}

// ---- download and stage ------------------------------------------------------------------

#[test]
fn a_download_that_differs_from_its_sha256_is_refused_and_nothing_is_staged() {
    let net = Net::release();
    let mut gz = fixture(GZ_ZAUN);
    let last = gz.len() - 1;
    gz[last] ^= 1;
    net.serve(&download_url(GZ_ZAUN), gz);
    let (fs, store) = empty_store();
    let r = stage::download(
        &UpdateDeps::files_client(&base(&net)),
        &fs,
        &store,
        &release(&net),
        "x86_64",
    );
    assert!(matches!(r, Err(UpdateError::BadFile { .. })), "{r:?}");
    assert!(!fs.exists(&store.staged()));
    assert!(!fs.exists(&store.versions.join("9.9.9")));
}

#[test]
fn a_file_larger_than_signed_is_never_read_whole() {
    let net = Net::release();
    net.serve(&download_url(GZ_ZAUN), vec![0; 4096]);
    let (fs, store) = empty_store();
    let r = stage::download(
        &UpdateDeps::files_client(&base(&net)),
        &fs,
        &store,
        &release(&net),
        "x86_64",
    );
    assert!(matches!(r, Err(UpdateError::BadFile { .. })), "{r:?}");
}

#[test]
fn a_download_fills_a_version_folder_and_skips_the_old_engines_folder() {
    let (fs, store, net) = staged_store();
    let dir = store.versions.join("9.9.9");
    assert_eq!(linked(&fs, &store.staged()).as_deref(), Some("9.9.9"));
    assert_eq!(
        fs.contents(dir.join("zaun")).unwrap(),
        b"zaun 9.9.9 (fixture binary)\n"
    );
    for f in [
        "zaun-init",
        "zaun-overlay",
        "zaun-chat",
        "bundle.json",
        "bundle.json.sig",
    ] {
        assert!(fs.exists(&dir.join(f)), "{f}");
    }
    assert!(!fs.exists(&dir.join("simplex")));
    assert!(!fs.exists(&store.components));
    assert_eq!(net.asked_for(ENGINE), 0);
}

#[test]
fn the_real_transport_downloads_files_of_exactly_their_signed_size() {
    // ureq 3.5.1 failed a body of exactly its limit, and every file is capped at its size.
    let server = FakeHttpServer::start().unwrap();
    let path = |url: &str| url.strip_prefix("https:/").unwrap().to_owned();
    server
        .route(&path(MANIFEST_URL), Reply::ok(fixture("bundle.json")))
        .route(&path(SIGNATURE_URL), Reply::ok(fixture("bundle.json.sig")));
    for f in FILES {
        server.route(&path(&download_url(f)), Reply::ok(fixture(f)));
    }
    let base = HttpClient::new(
        Arc::new(UreqTransport::plaintext_loopback(server.addr())),
        Arc::new(SystemClock::new()),
    );
    let release = fetch_release(&UpdateDeps::check_client(&base), &[test_key()]).unwrap();
    let (fs, store) = empty_store();
    stage::download(
        &UpdateDeps::files_client(&base),
        &fs,
        &store,
        &release,
        "x86_64",
    )
    .unwrap();
    assert_eq!(linked(&fs, &store.staged()).as_deref(), Some("9.9.9"));
}

#[test]
fn the_running_version_is_never_downloaded_over() {
    let net = Net::release();
    let (fs, store) = empty_store();
    with_current(&fs, &store, "9.9.9");
    let r = stage::download(
        &UpdateDeps::files_client(&base(&net)),
        &fs,
        &store,
        &release(&net),
        "x86_64",
    );
    assert!(matches!(r, Err(UpdateError::Install(_))), "{r:?}");
    assert_eq!(
        fs.contents(store.versions.join("9.9.9/zaun")).unwrap(),
        b"zaun 9.9.9\n"
    );
}

// ---- switch, start, confirm, roll back -------------------------------------------------------

#[test]
fn apply_moves_the_links_and_starts_a_trial() {
    let (fs, store, _) = staged_store();
    with_current(&fs, &store, "9.9.8");
    let runner = smoke_ok(&store);
    let got = apply::apply(&fs, &runner, &store, &[test_key()], &running(), "x86_64").unwrap();
    assert_eq!(got, Some(v("9.9.9")));
    assert_eq!(linked(&fs, &store.current()).as_deref(), Some("9.9.9"));
    assert_eq!(linked(&fs, &store.previous()).as_deref(), Some("9.9.8"));
    assert!(!fs.exists(&store.staged()));
    assert!(fs.exists(&store.trial_file()));
    assert!(
        runner.unused().is_empty(),
        "the new version was smoke-tested"
    );
}

#[test]
fn an_older_or_equal_release_is_never_switched_to() {
    let (fs, store, _) = staged_store();
    let r = apply::apply(
        &fs,
        &smoke_ok(&store),
        &store,
        &[test_key()],
        &v("9.9.9"),
        "x86_64",
    );
    assert!(matches!(r, Err(UpdateError::NotNewer { .. })), "{r:?}");
    assert!(!fs.exists(&store.current()));
    assert!(!fs.exists(&store.staged()), "a refused version is dropped");

    let (fs, store, _) = staged_store();
    with_current(&fs, &store, "10.0.0");
    let r = apply::apply(
        &fs,
        &smoke_ok(&store),
        &store,
        &[test_key()],
        &running(),
        "x86_64",
    );
    assert!(matches!(r, Err(UpdateError::NotNewer { .. })), "{r:?}");
    assert_eq!(linked(&fs, &store.current()).as_deref(), Some("10.0.0"));
}

#[test]
fn a_staged_file_changed_after_the_download_is_refused() {
    let (fs, store, _) = staged_store();
    fs.file(store.versions.join("9.9.9/zaun-init"), b"something else");
    let r = apply::apply(
        &fs,
        &smoke_ok(&store),
        &store,
        &[test_key()],
        &running(),
        "x86_64",
    );
    assert!(matches!(r, Err(UpdateError::BadFile { .. })), "{r:?}");
    assert!(!fs.exists(&store.current()));
}

#[test]
fn a_new_version_that_does_not_run_here_switches_nothing() {
    let (fs, store, _) = staged_store();
    with_current(&fs, &store, "9.9.8");
    let bin = store.versions.join("9.9.9/zaun");
    // NixOS without nix-ld: no program interpreter.
    let runner = ScriptedRunner::new().expect(
        &[bin.as_os_str(), "--version".as_ref()],
        CannedOutput::fail(127, "cannot execute: required file not found"),
    );
    let r = apply::apply(&fs, &runner, &store, &[test_key()], &running(), "x86_64");
    assert!(matches!(r, Err(UpdateError::Install(_))), "{r:?}");
    assert_eq!(linked(&fs, &store.current()).as_deref(), Some("9.9.8"));
    assert!(!fs.exists(&store.trial_file()));
}

fn switched() -> (MemFs, Store) {
    let (fs, store, _) = staged_store();
    with_current(&fs, &store, "9.9.8");
    apply::apply(
        &fs,
        &smoke_ok(&store),
        &store,
        &[test_key()],
        &running(),
        "x86_64",
    )
    .unwrap();
    (fs, store)
}

#[test]
fn the_installed_copy_starts_current_only_when_it_is_newer() {
    let (fs, store) = switched();
    let bin = store.versions.join("9.9.9/zaun");
    let start = |installed: &str| trial::choose(&fs, &store, &v(installed), false).unwrap();
    assert_eq!(start("0.3.0").start, Some(bin));
    assert_eq!(start("9.9.9").start, None, "the installed copy is as new");
    assert_eq!(
        start("10.0.0").start,
        None,
        "a package manager installed a newer one"
    );
    let (fs, store) = empty_store();
    assert_eq!(
        trial::choose(&fs, &store, &v("0.3.0"), true).unwrap(),
        Choice::default()
    );
}

#[test]
fn the_third_counted_start_without_a_confirmation_rolls_back() {
    let (fs, store) = switched();
    let installed = v("0.3.0");
    let new = store.versions.join("9.9.9/zaun");
    for _ in 0..5 {
        // CLI verbs neither count nor confirm.
        trial::choose(&fs, &store, &installed, false).unwrap();
    }
    for _ in 0..2 {
        let c = trial::choose(&fs, &store, &installed, true).unwrap();
        assert_eq!(c.start.as_ref(), Some(&new));
        assert_eq!(c.rolled_back, None);
    }
    let c = trial::choose(&fs, &store, &installed, true).unwrap();
    assert_eq!(c.rolled_back, Some(v("9.9.9")));
    assert_eq!(c.start, Some(store.versions.join("9.9.8/zaun")));
    assert_eq!(linked(&fs, &store.current()).as_deref(), Some("9.9.8"));
    assert!(!fs.exists(&store.previous()));
    assert!(!fs.exists(&store.trial_file()));
    assert!(
        !fs.exists(&store.versions.join("9.9.9")),
        "the failed version is removed"
    );
}

#[test]
fn a_first_version_that_fails_hands_back_to_the_installed_copy() {
    let (fs, store, _) = staged_store();
    apply::apply(
        &fs,
        &smoke_ok(&store),
        &store,
        &[test_key()],
        &running(),
        "x86_64",
    )
    .unwrap();
    let installed = v("0.3.0");
    for _ in 0..2 {
        trial::choose(&fs, &store, &installed, true).unwrap();
    }
    let c = trial::choose(&fs, &store, &installed, true).unwrap();
    assert_eq!(c.rolled_back, Some(v("9.9.9")));
    assert_eq!(c.start, None);
    assert!(!fs.exists(&store.current()));
}

#[test]
fn a_confirmed_version_keeps_only_itself() {
    let (fs, store) = switched();
    trial::choose(&fs, &store, &v("0.3.0"), true).unwrap();
    assert!(
        !trial::confirm(&fs, &store, &v("9.9.8")).unwrap(),
        "not the version on trial"
    );
    assert!(trial::confirm(&fs, &store, &v("9.9.9")).unwrap());
    assert!(!fs.exists(&store.previous()));
    assert!(!fs.exists(&store.trial_file()));
    assert!(!fs.exists(&store.versions.join("9.9.8")));
    assert!(
        !fs.exists(&store.components.join("simplex-old")),
        "the old engine is unused"
    );
    // Later starts count nothing.
    let c = trial::choose(&fs, &store, &v("0.3.0"), true).unwrap();
    assert_eq!(c.rolled_back, None);
    assert!(!fs.exists(&store.trial_file()));
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
        .file(format!("{DATA}/versions/0.3.0/zaun"), b"");
    let channel = |p: &str| detect(&fs, Path::new(p), store()).channel;
    let nix = "/nix/store/abc-zaun-0.1.0/bin/.zaun-wrapped";
    assert_eq!(channel(nix), InstallChannel::Nix { bundles: false });
    assert!(
        !channel(nix).self_updates(),
        "no loader for the bundle: announce only"
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
        InstallChannel::Tarball
    );
    assert_eq!(
        channel(&format!("{DATA}/versions/0.3.0/zaun")),
        InstallChannel::Bundle
    );
    assert!(
        detect(&fs, Path::new("/usr/bin/zaun"), store())
            .store()
            .is_some()
    );
    assert!(
        detect(&fs, Path::new("/opt/zaun/target/debug/zaun"), store())
            .store()
            .is_none()
    );
    // nix-ld (programs.zaun.selfUpdate) puts the generic loader in place.
    fs.file(GENERIC_LOADER, b"");
    assert_eq!(channel(nix), InstallChannel::Nix { bundles: true });
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
    store: Store,
}

impl World {
    fn new(check: bool) -> Self {
        Self::in_env(TestEnv::new().unwrap(), check)
    }

    fn in_env(env: TestEnv, check: bool) -> Self {
        let net = Net::release();
        let store = Store::new(&env.paths());
        std::fs::create_dir_all(&env.paths().data_dir).unwrap();
        let fs: Arc<dyn Fs> = Arc::new(RealFs::new());
        let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
        let jobs = Arc::new(JobPool::new(JobPoolConfig::default(), Arc::clone(&clock)).unwrap());
        let mut prefs = Preferences::default();
        prefs.updates.check = check;
        let b = base(&net);
        let staged = store.versions.join("9.9.9/zaun");
        let updates = UpdateDeps {
            check: Arc::new(UpdateDeps::check_client(&b)),
            files: Arc::new(UpdateDeps::files_client(&b)),
            keys: vec![test_key()],
            install: Install {
                channel: InstallChannel::Tarball,
                store: store.clone(),
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
            store,
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
    // The check, then the background download: the manifest and its signature again, and the
    // bundle's four files (not the old Chat engine's folder).
    assert_eq!(w.net.calls(), 2 + 2 + 4);
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
fn a_newer_version_is_downloaded_in_the_background() {
    let w = World::new(true);
    w.drain();
    assert_eq!(
        w.svc.hub().updates.load().download,
        UpdateDownload::Staged {
            version: "9.9.9".into()
        }
    );
    assert!(w.store.staged().join("zaun").exists());
    assert!(
        !w.store.current().exists(),
        "switched to at the next start only"
    );
}

#[test]
fn a_version_that_was_rolled_back_is_not_downloaded_again_by_itself() {
    let env = TestEnv::new().unwrap();
    let path = zaun_core::update::state::state_file(&env.paths());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let st = UpdateState {
        rolled_back: Some("9.9.9".into()),
        ..UpdateState::default()
    };
    st.save(&RealFs::new(), &path).unwrap();
    let mut w = World::in_env(env, true);
    w.drain();
    assert_eq!(w.svc.hub().updates.load().download, UpdateDownload::Idle);
    assert_eq!(w.net.calls(), 2, "only the check");
    // Asked for, it is downloaded.
    let evs = w.send(UpdateCommand::Download);
    assert_eq!(World::failure(&evs), None);
    assert!(w.store.staged().exists());
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
    assert!(w.store.staged().join("zaun").exists());

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
    assert!(!w.store.restart_file().exists());
    assert!(!evs.iter().any(|e| matches!(e, Event::QuitRequested)));
    assert!(!w.store.current().exists());

    // The game ended: restart leaves the note for the process and closes the window.
    let mut db = Db::open_rw(&w.env.paths().db_file).unwrap();
    assert!(db.delete_session(id, &[]).unwrap());
    drop(db);
    w.svc.poll_sessions_now();
    w.settle();
    let evs = w.send(UpdateCommand::Restart);
    assert_eq!(World::failure(&evs), None);
    assert!(w.store.restart_file().exists());
    assert!(evs.iter().any(|e| matches!(e, Event::QuitRequested)));
}
