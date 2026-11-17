//! Steam games (ADR-0031): the scan against a fake Steam root, the library behaviour (appear,
//! disappear, what the user set is kept, nothing written under a Steam root, nothing read
//! while the option is off), requests handed to Steam, the running state, and the planner
//! entry point for programs run in a Steam game's Windows storage.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use easyplay_core::artwork::steam::FsAppIdFile;
use easyplay_core::artwork::{ArtIndex, NoIcons};
use easyplay_core::clock::{Clock, SystemClock};
use easyplay_core::config::Preferences;
use easyplay_core::error::{Error, ErrorCode};
use easyplay_core::fsx::{DirEntry, Fs, FsResult, Metadata, MountTable, RealFs};
use easyplay_core::host::{CommandSpec, StaticHostFacts};
use easyplay_core::identity::{FsPeInspector, SharedLocations};
use easyplay_core::ids::{GameId, RuntimeName, TagName};
use easyplay_core::jobs::{JobHandle, JobLabel, JobPool, JobPoolConfig, JobPriority};
use easyplay_core::launch::NoLocks;
use easyplay_core::library::draft::NoUmuTitles;
use easyplay_core::library::service::NoPolicyCheck;
use easyplay_core::model::{
    Activity, Availability, Game, GameOrigin, LaunchStateTag, PlaytimeSource, ProtectionState,
    RuntimeRef, SlrVariant, SteamCompat, SteamInstallKind, SteamUpdate, Tool,
};
use easyplay_core::runtime::{ProvisionNeed, Resolution, RuntimeCatalog, RuntimeService};
use easyplay_core::service::{
    AppService, Command, Event, GameEdit, LaunchEnv, LaunchOrigin, Notice, QueryResult,
    ServiceDeps, SteamCommand,
};
use easyplay_core::steam::client::SteamRequest;
use easyplay_core::steam::{Handoff, NotAGame};
use easyplay_core::storage::{Db, SteamRepo};
use easyplay_testkit::MemFs;
use easyplay_testkit::TestEnv;
use easyplay_testkit::fakes::{FakeLauncher, FakeProc, proc_entry};
use easyplay_testkit::fixtures::hosts;
use easyplay_testkit::fixtures::steam_root::{self, GAMES, NOT_GAMES};

// ---- The scan ------------------------------------------------------------------------------

#[test]
fn the_scan_reads_a_fake_root_and_writes_nothing() {
    let fs = MemFs::new();
    let home = Path::new("/home/player");
    let fx = steam_root::steam_root(&fs, home);
    let writes_before: usize = write_calls(&fs);
    let scan = easyplay_core::steam::scan(&fs, home);
    assert_eq!(write_calls(&fs), writes_before, "the scan wrote something");

    // `~/.steam/steam` and `~/.steam/root` are links to the same root: one installation.
    assert_eq!(scan.roots.len(), 1);
    assert_eq!(scan.roots[0].path, fx.root);
    assert_eq!(scan.roots[0].kind, SteamInstallKind::Native);

    let ids: Vec<u32> = scan.games.iter().map(|g| g.facts.app_id).collect();
    assert_eq!(ids, GAMES.to_vec());
    let skipped: Vec<(u32, NotAGame)> = scan.skipped.iter().map(|s| (s.app_id, s.reason)).collect();
    assert_eq!(
        skipped,
        vec![
            (NOT_GAMES[0], NotAGame::Redistributable),
            (NOT_GAMES[1], NotAGame::CompatibilityTool),
            (NOT_GAMES[2], NotAGame::Runtime),
        ]
    );

    let rdr = scan.game(1_174_180).unwrap();
    assert_eq!(rdr.name, "Red Dead Redemption 2");
    assert_eq!(rdr.facts.library_folder, fx.library);
    assert_eq!(
        rdr.facts.install_dir,
        fx.library.join("steamapps/common/Red Dead Redemption 2")
    );
    assert_eq!(rdr.facts.update, SteamUpdate::Waiting);
    assert_eq!(rdr.facts.playtime_minutes, Some(4474));
    assert!(rdr.last_played.is_some());
    assert!(rdr.present);
    match &rdr.facts.compat {
        SteamCompat::Proton {
            tool,
            version,
            compat_data,
            prefix,
            proton_dir,
        } => {
            assert_eq!(tool.as_deref(), Some("GE-Proton10-25"));
            assert_eq!(version.as_deref(), Some("GE-Proton10-25"));
            assert_eq!(compat_data, &fx.compat_data);
            assert_eq!(
                prefix.as_deref(),
                Some(fx.compat_data.join("pfx").as_path())
            );
            assert_eq!(proton_dir.as_deref(), Some(fx.proton.as_path()));
        }
        other => panic!("{other:?}"),
    }
    // Covers from the three cache layouts.
    assert_eq!(rdr.art.len(), 1, "hashed layout");
    let tf2 = scan.game(440).unwrap();
    assert_eq!(tf2.art.len(), 2, "per-app layout: cover and banner");
    assert_eq!(tf2.facts.update, SteamUpdate::Paused);
    assert_eq!(tf2.facts.compat, SteamCompat::NotProton);
    assert_eq!(tf2.facts.playtime_minutes, Some(125));
    let gta = scan.game(12210).unwrap();
    assert_eq!(gta.art.len(), 1, "flat layout");
    assert_eq!(gta.facts.update, SteamUpdate::UpToDate);
    assert_eq!(gta.last_played, None);
}

fn write_calls(fs: &MemFs) -> usize {
    use easyplay_core::fsx::FsOp;
    [
        FsOp::CreateDir,
        FsOp::Write,
        FsOp::Rename,
        FsOp::RemoveFile,
        FsOp::RemoveDir,
        FsOp::Symlink,
        FsOp::SetMode,
    ]
    .iter()
    .map(|op| fs.calls(*op))
    .sum()
}

#[test]
fn flatpak_and_snap_roots_and_a_missing_drive() {
    let fs = MemFs::new();
    let home = Path::new("/home/p");
    let flat = home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam");
    fs.file(
        flat.join("steamapps/appmanifest_70.acf"),
        "\"AppState\" { \"appid\" \"70\" \"name\" \"Half-Life\" \"StateFlags\" \"4\" \"installdir\" \"Half-Life\" }",
    );
    let snap = home.join("snap/steam/common/.local/share/Steam");
    fs.file(
        snap.join("steamapps/appmanifest_70.acf"),
        "\"AppState\" { \"appid\" \"70\" \"name\" \"Half-Life (snap)\" \"StateFlags\" \"4\" \"installdir\" \"Half-Life\" }",
    );
    fs.file(
        snap.join("steamapps/appmanifest_80.acf"),
        "\"AppState\" { \"appid\" \"80\" \"name\" \"Counter-Strike\" \"StateFlags\" \"4\" \"installdir\" \"cs\" }",
    );
    // An app being installed for the first time is not listed yet.
    fs.file(
        snap.join("steamapps/appmanifest_90.acf"),
        "\"AppState\" { \"appid\" \"90\" \"name\" \"New\" \"StateFlags\" \"1026\" \"installdir\" \"new\" }",
    );
    // A manifest whose app ID disagrees with its file name is not trusted.
    fs.file(
        snap.join("steamapps/appmanifest_91.acf"),
        "\"AppState\" { \"appid\" \"92\" \"name\" \"Odd\" \"StateFlags\" \"4\" \"installdir\" \"odd\" }",
    );
    fs.dir(flat.join("steamapps/common/Half-Life"));
    let scan = easyplay_core::steam::scan(&fs, home);
    let kinds: Vec<SteamInstallKind> = scan.roots.iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        vec![SteamInstallKind::Flatpak, SteamInstallKind::Snap]
    );
    let hl = scan.game(70).unwrap();
    assert_eq!(hl.name, "Half-Life", "the first installation wins");
    assert_eq!(hl.facts.install, SteamInstallKind::Flatpak);
    assert!(hl.present);
    let cs = scan.game(80).unwrap();
    assert!(
        !cs.present,
        "its folder is missing: listed, shown as not connected"
    );
    assert!(scan.game(90).is_none());
    assert!(scan.game(91).is_none() && scan.game(92).is_none());
    // No Steam at all.
    let empty = easyplay_core::steam::scan(&MemFs::new(), home);
    assert!(empty.roots.is_empty() && empty.games.is_empty());
}

// ---- The service ---------------------------------------------------------------------------

/// A filesystem that records every path read or written, delegating to the real one.
struct Recording {
    inner: RealFs,
    reads: Mutex<Vec<PathBuf>>,
    writes: Mutex<Vec<PathBuf>>,
}

impl Recording {
    fn new() -> Self {
        Self {
            inner: RealFs::new(),
            reads: Mutex::new(Vec::new()),
            writes: Mutex::new(Vec::new()),
        }
    }
    fn r(&self, p: &Path) {
        self.reads.lock().unwrap().push(p.to_path_buf());
    }
    fn w(&self, p: &Path) {
        self.writes.lock().unwrap().push(p.to_path_buf());
    }
    fn touched(&self, under: &Path, writes: bool) -> Vec<PathBuf> {
        let v = if writes { &self.writes } else { &self.reads };
        v.lock()
            .unwrap()
            .iter()
            .filter(|p| p.starts_with(under))
            .cloned()
            .collect()
    }
}

impl Fs for Recording {
    fn metadata(&self, path: &Path) -> FsResult<Metadata> {
        self.r(path);
        self.inner.metadata(path)
    }
    fn metadata_follow(&self, path: &Path) -> FsResult<Metadata> {
        self.r(path);
        self.inner.metadata_follow(path)
    }
    fn read_link(&self, path: &Path) -> FsResult<PathBuf> {
        self.r(path);
        self.inner.read_link(path)
    }
    fn read(&self, path: &Path, limit: u64) -> FsResult<Vec<u8>> {
        self.r(path);
        self.inner.read(path, limit)
    }
    fn read_follow(&self, path: &Path, limit: u64) -> FsResult<Vec<u8>> {
        self.r(path);
        self.inner.read_follow(path, limit)
    }
    fn read_range(&self, path: &Path, offset: u64, len: usize) -> FsResult<Vec<u8>> {
        self.r(path);
        self.inner.read_range(path, offset, len)
    }
    fn open_read(&self, path: &Path) -> FsResult<Box<dyn Read + Send>> {
        self.r(path);
        self.inner.open_read(path)
    }
    fn read_dir(&self, path: &Path) -> FsResult<Vec<DirEntry>> {
        self.r(path);
        self.inner.read_dir(path)
    }
    fn canonicalize(&self, path: &Path) -> FsResult<PathBuf> {
        self.r(path);
        self.inner.canonicalize(path)
    }
    fn create_dir(&self, path: &Path, mode: u32) -> FsResult<()> {
        self.w(path);
        self.inner.create_dir(path, mode)
    }
    fn create_dir_all(&self, path: &Path, mode: u32) -> FsResult<()> {
        self.w(path);
        self.inner.create_dir_all(path, mode)
    }
    fn write_new(&self, path: &Path, data: &[u8], mode: u32) -> FsResult<()> {
        self.w(path);
        self.inner.write_new(path, data, mode)
    }
    fn atomic_write(&self, path: &Path, data: &[u8], mode: u32) -> FsResult<()> {
        self.w(path);
        self.inner.atomic_write(path, data, mode)
    }
    fn rename(&self, from: &Path, to: &Path) -> FsResult<()> {
        self.w(from);
        self.w(to);
        self.inner.rename(from, to)
    }
    fn remove_file(&self, path: &Path) -> FsResult<()> {
        self.w(path);
        self.inner.remove_file(path)
    }
    fn remove_dir(&self, path: &Path) -> FsResult<()> {
        self.w(path);
        self.inner.remove_dir(path)
    }
    fn symlink(&self, target: &Path, link: &Path) -> FsResult<()> {
        self.w(link);
        self.inner.symlink(target, link)
    }
    fn set_mode(&self, path: &Path, mode: u32) -> FsResult<()> {
        self.w(path);
        self.inner.set_mode(path, mode)
    }
    fn free_space(&self, path: &Path) -> FsResult<u64> {
        self.inner.free_space(path)
    }
    fn reflink_supported(&self, a: &Path, b: &Path) -> FsResult<bool> {
        self.inner.reflink_supported(a, b)
    }
}

/// Records the commands handed to Steam.
#[derive(Default)]
struct RecordingHandoff(Mutex<Vec<CommandSpec>>);

impl Handoff for RecordingHandoff {
    fn start_detached(&self, cmd: &CommandSpec) -> Result<(), Error> {
        self.0.lock().unwrap().push(cmd.clone());
        Ok(())
    }
}

struct NeedsSlr(Arc<JobPool>);

impl RuntimeService for NeedsSlr {
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
    fx: steam_root::SteamFixture,
    fs: Arc<Recording>,
    proc: Arc<FakeProc>,
    svc: AppService,
    events: Receiver<Event>,
    handoff: Arc<RecordingHandoff>,
    launcher: Arc<FakeLauncher>,
    steam_bin: PathBuf,
}

impl World {
    fn new(show_games: bool) -> Self {
        let env = TestEnv::new().unwrap();
        let fx = steam_root::steam_root_on_disk(&env.home());
        // A `steam` program on PATH (never run: the hand-off is recorded).
        let bin = env.root().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let steam_bin = bin.join("steam");
        std::fs::write(&steam_bin, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(
            &steam_bin,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        let fs = Arc::new(Recording::new());
        let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
        let jobs = Arc::new(JobPool::new(JobPoolConfig::default(), Arc::clone(&clock)).unwrap());
        let proc = Arc::new(FakeProc::new());
        let launcher = Arc::new(FakeLauncher::new());
        let mut prefs = Preferences::default();
        prefs.steam.show_games = show_games;
        let mut host_env = BTreeMap::new();
        host_env.insert("PATH".to_owned(), bin.display().to_string());
        host_env.insert("HOME".to_owned(), env.home().display().to_string());
        host_env.insert("EASYPLAY_DATA_DIR".to_owned(), "/x".to_owned());
        let deps = ServiceDeps {
            fs: Arc::clone(&fs) as Arc<dyn Fs>,
            clock: Arc::clone(&clock),
            paths: env.paths(),
            jobs: Arc::clone(&jobs),
            proc: Arc::clone(&proc) as _,
            launcher: Arc::clone(&launcher) as _,
            locks: Arc::new(NoLocks),
            runtimes: Arc::new(NeedsSlr(Arc::clone(&jobs))),
            host: Arc::new(StaticHostFacts(Arc::new(hosts::arch_mesa()))),
            pe: Arc::new(FsPeInspector::new(
                Arc::new(RealFs::new()),
                Arc::clone(&clock),
            )),
            icons: Arc::new(NoIcons),
            codec: None,
            launch_env: LaunchEnv {
                uid: 1000,
                gid: 1000,
                user_name: "sam".into(),
                group_name: "sam".into(),
                home: env.home(),
                host_env,
                init_binary: PathBuf::from("/nonexistent/easyplay-init"),
                mounts: MountTable::default(),
                user_dirs: Vec::new(),
                seccomp_available: false,
                global_env: Vec::new(),
            },
            prefs,
            desktop: None,
            http: None,
            app_id_file: Arc::new(FsAppIdFile::new(Arc::new(RealFs::new()))),
            art_index: Arc::new(ArtIndex::new()),
            shared: SharedLocations::default(),
            policy_check: Some(Arc::new(NoPolicyCheck)),
            umu_titles: Arc::new(NoUmuTitles),
            watch_sessions: false,
        };
        let (mut svc, events) = AppService::open_headless(deps).unwrap();
        let handoff = Arc::new(RecordingHandoff::default());
        svc.set_steam_handoff(Arc::clone(&handoff) as Arc<dyn Handoff>);
        let mut w = Self {
            env,
            fx,
            fs,
            proc,
            svc,
            events,
            handoff,
            launcher,
            steam_bin,
        };
        w.settle();
        w
    }

    fn settle(&mut self) {
        while self.svc.pump(Duration::from_millis(300)) {}
    }

    fn send(&mut self, c: Command) -> Vec<Event> {
        self.svc.handle(c);
        self.settle();
        self.events.try_iter().collect()
    }

    fn steam_ids(&self) -> Vec<u32> {
        let snap = self.svc.hub().library.load();
        let mut v: Vec<u32> = snap
            .steam_games()
            .filter_map(|g| g.steam().map(|f| f.app_id))
            .collect();
        v.sort_unstable();
        v
    }

    fn handed(&self) -> Vec<Vec<String>> {
        self.handoff
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|c| {
                c.argv()
                    .iter()
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect()
            })
            .collect()
    }
}

fn failure(evs: &[Event]) -> Option<ErrorCode> {
    evs.iter().find_map(|e| match e {
        Event::Failure(u) => Some(u.code),
        _ => None,
    })
}

const RDR: GameId = GameId::steam(1_174_180);

#[test]
fn nothing_of_steam_is_read_while_the_option_is_off() {
    let mut w = World::new(false);
    assert!(w.steam_ids().is_empty());
    w.send(Command::RescanMissing);
    assert!(w.steam_ids().is_empty());
    let steam = w.env.home().join(".local/share/Steam");
    assert!(w.fs.touched(&steam, false).is_empty(), "read while off");
    assert!(w.fs.touched(&w.fx.library, false).is_empty());
}

#[test]
fn steam_games_appear_disappear_and_keep_what_the_user_set() {
    let mut w = World::new(false);
    let evs = w.send(Command::Steam(SteamCommand::SetEnabled(true)));
    assert!(evs.iter().any(|e| matches!(e, Event::PreferencesChanged)));
    assert_eq!(w.steam_ids(), GAMES.to_vec());

    let snap = w.svc.hub().library.load();
    let rdr = snap.get(RDR).unwrap();
    assert!(matches!(rdr.origin, GameOrigin::Steam(_)));
    assert_eq!(rdr.protection(), ProtectionState::RunBySteam);
    assert_eq!(rdr.playtime_source(), PlaytimeSource::Steam);
    assert_eq!(rdr.total_playtime_ms, 4474 * 60_000);
    assert_eq!(rdr.availability, Availability::Present);
    assert_eq!(rdr.steam_app_id, Some(1_174_180));
    assert!(rdr.cover.is_some());
    // Steam minutes are counted apart from EasyPlay's sessions.
    assert_eq!(snap.totals.total_playtime_ms, 0);
    assert_eq!(snap.totals.steam_playtime_ms, (4474 + 125) * 60_000);
    // GTA IV has no play time: "Never played"; it is a game, listed in "All games".
    assert!(snap.get(GameId::steam(12210)).unwrap().never_played());
    assert_eq!(snap.totals.all, 3);
    // Search and the `is:steam` operator find them; protection filters never do.
    let q = |text: &str| {
        easyplay_core::library::run_query(
            &snap,
            &easyplay_core::library::LibraryQuery {
                text: text.into(),
                ..Default::default()
            },
        )
        .items
        .len()
    };
    assert_eq!(q("red dead"), 1);
    assert_eq!(q("is:steam"), 3);
    assert_eq!(q("preset:standard"), 0);
    assert_eq!(q("is:online"), 0);
    // The "Steam" scope holds them (ADR-0031).
    let steam = easyplay_core::library::run_query(
        &snap,
        &easyplay_core::library::LibraryQuery {
            collection: easyplay_core::library::Collection::Steam,
            ..Default::default()
        },
    );
    assert_eq!(steam.items.len(), 3);

    // What the user sets is EasyPlay's own and stored.
    w.send(Command::SetFavorite {
        id: RDR,
        favorite: true,
    });
    w.send(Command::SetHidden {
        id: GameId::steam(440),
        hidden: true,
    });
    w.send(Command::SetTags {
        id: RDR,
        tags: vec![TagName::new("western").unwrap()],
    });
    let evs = w.send(Command::UpdateGame {
        id: RDR,
        edits: vec![GameEdit::Notes("Chapter 4".into())],
    });
    assert_eq!(failure(&evs), None);
    let snap = w.svc.hub().library.load();
    let rdr = snap.get(RDR).unwrap();
    assert!(rdr.favorite);
    assert_eq!(rdr.tags.len(), 1);
    assert!(snap.get(GameId::steam(440)).unwrap().hidden);
    // Edits Steam owns are refused.
    let evs = w.send(Command::UpdateGame {
        id: RDR,
        edits: vec![GameEdit::Name("Other".into())],
    });
    assert_eq!(failure(&evs), Some(ErrorCode::StmNotForSteam));

    // Uninstalled in Steam: it disappears, its row stays.
    let manifest = w.fx.library.join("steamapps/appmanifest_1174180.acf");
    let saved = std::fs::read(&manifest).unwrap();
    std::fs::remove_file(&manifest).unwrap();
    w.send(Command::RescanMissing);
    assert_eq!(w.steam_ids(), vec![440, 12210]);
    let rows = Db::open_ro(&w.env.paths().db_file)
        .unwrap()
        .steam_overlays()
        .unwrap();
    assert!(rows.iter().any(|r| r.app_id == 1_174_180 && r.favorite));

    // Installed again: everything the user set returns.
    std::fs::write(&manifest, saved).unwrap();
    let evs = w.send(Command::Steam(SteamCommand::Rescan));
    assert!(
        evs.iter()
            .any(|e| matches!(e, Event::Notice(Notice::SteamScanned { games: 3 })))
    );
    let snap = w.svc.hub().library.load();
    let rdr = snap.get(RDR).unwrap();
    assert!(rdr.favorite);
    assert_eq!(rdr.tags.len(), 1);
    let q = easyplay_core::ids::QueryId::from_uuid(uuid::Uuid::from_u128(7));
    let evs = w.send(Command::Steam(SteamCommand::Details {
        id: RDR,
        reply_to: q,
    }));
    let detail = evs
        .iter()
        .find_map(|e| match e {
            Event::QueryResult {
                result: QueryResult::Steam(d),
                ..
            } => Some(d.clone()),
            _ => None,
        })
        .expect("details");
    assert_eq!(detail.notes, "Chapter 4");
    assert!(detail.client_found);
    assert!(detail.prefix_tools);
    assert!(!detail.running);

    // Removing is refused (Steam uninstalls); hiding is the way.
    let evs = w.send(Command::RemoveGame {
        id: RDR,
        options: easyplay_core::library::RemoveOptions::default(),
    });
    assert_eq!(failure(&evs), Some(ErrorCode::StmNotForSteam));

    // Turned off: they leave the library at once.
    w.send(Command::Steam(SteamCommand::SetEnabled(false)));
    assert!(w.steam_ids().is_empty());

    // Nothing was ever written under a Steam root or library folder.
    let root = w.fx.root.clone();
    assert!(
        w.fs.touched(&root, true).is_empty(),
        "{:?}",
        w.fs.touched(&root, true)
    );
    assert!(w.fs.touched(&w.fx.library, true).is_empty());
}

#[test]
fn play_update_and_open_are_handed_to_steam_and_running_is_seen() {
    let mut w = World::new(true);
    let evs = w.send(Command::Launch {
        id: GameId::steam(440),
        exe: None,
        origin: LaunchOrigin::Library,
    });
    assert_eq!(failure(&evs), None);
    assert!(evs.iter().any(|e| matches!(
        e,
        Event::Notice(Notice::SteamAsked {
            request: SteamRequest::Run(440),
            ..
        })
    )));
    // Nothing was planned or launched by EasyPlay itself.
    assert!(w.launcher.plans().is_empty());
    let bin = w.steam_bin.display().to_string();
    assert_eq!(
        w.handed(),
        vec![vec![bin.clone(), "steam://rungameid/440".into()]]
    );
    // EasyPlay's own variables are not passed on.
    let env = w.handoff.0.lock().unwrap()[0].env.clone();
    assert!(env.iter().any(|(k, _)| k == "HOME"));
    assert!(
        !env.iter()
            .any(|(k, _)| k.to_string_lossy().starts_with("EASYPLAY_"))
    );
    let tf2 = GameId::steam(440);
    assert_eq!(
        w.svc.hub().library.load().get(tf2).unwrap().activity,
        Activity::Launching(LaunchStateTag::Starting)
    );

    // Steam starts it: `reaper SteamLaunch AppId=440 -- …`.
    w.proc.insert(proc_entry(4242, 1, "reaper", 1));
    w.proc.set_cmdline(
        4242,
        &[
            "/home/p/.local/share/Steam/ubuntu12_32/reaper",
            "SteamLaunch",
            "AppId=440",
            "--",
            "x",
        ],
    );
    w.svc.steam_poll_now();
    assert!(matches!(
        w.svc.hub().library.load().get(tf2).unwrap().activity,
        Activity::Running { .. }
    ));
    // It ends.
    w.proc.remove(4242);
    w.svc.steam_poll_now();
    assert_eq!(
        w.svc.hub().library.load().get(tf2).unwrap().activity,
        Activity::Idle
    );

    w.send(Command::Steam(SteamCommand::Update(RDR)));
    w.send(Command::Steam(SteamCommand::OpenInSteam(RDR)));
    let handed = w.handed();
    assert_eq!(
        handed[1],
        vec![bin.clone(), "steam://open/downloads".to_owned()]
    );
    assert_eq!(
        handed[2],
        vec![bin, "steam://nav/games/details/1174180".to_owned()]
    );

    // Cancelling a Steam launch does nothing EasyPlay could not do.
    let evs = w.send(Command::CancelLaunch { id: tf2 });
    assert_eq!(failure(&evs), None);
}

#[test]
fn windows_storage_actions_check_the_game_first() {
    let mut w = World::new(true);
    // TF2 has no Windows storage (Linux version).
    let evs = w.send(Command::Steam(SteamCommand::RunInPrefix {
        id: GameId::steam(440),
        tool: Tool::Winecfg,
    }));
    assert_eq!(failure(&evs), Some(ErrorCode::StmNoPrefix));
    // Not while the game runs.
    w.proc.insert(proc_entry(77, 1, "reaper", 1));
    w.proc
        .set_cmdline(77, &["reaper", "SteamLaunch", "AppId=1174180", "--", "x"]);
    w.svc.steam_poll_now();
    let evs = w.send(Command::Steam(SteamCommand::RunInPrefix {
        id: RDR,
        tool: Tool::Winecfg,
    }));
    assert_eq!(failure(&evs), Some(ErrorCode::StmGameRunning));
    w.proc.remove(77);
    w.svc.steam_poll_now();
    // The compatibility layer resolves through the runtime service (here: missing).
    let evs = w.send(Command::Steam(SteamCommand::RunInPrefix {
        id: RDR,
        tool: Tool::RunExe {
            path: w.env.home().join("Downloads/ModInstaller/setup.exe"),
            args: vec![],
        },
    }));
    assert_eq!(failure(&evs), Some(ErrorCode::RtNotInstalled));
    assert!(w.launcher.plans().is_empty());
    // Winetricks is not offered for Steam games.
    let evs = w.send(Command::RunTool {
        id: RDR,
        tool: Tool::Winetricks {
            verbs: vec!["corefonts".into()],
        },
    });
    assert_eq!(failure(&evs), Some(ErrorCode::StmNotForSteam));
    // EasyPlay's own folder for the game is not an orphan.
    let orphans = w.svc.library().orphans().unwrap();
    assert!(orphans.iter().all(|o| o.id.is_none_or(|i| !i.is_steam())));
    // Nothing written under Steam's folders by any of this.
    assert!(w.fs.touched(&w.fx.root, true).is_empty());
    assert!(w.fs.touched(&w.fx.library, true).is_empty());
}

// ---- The planner ---------------------------------------------------------------------------

#[test]
fn a_tool_can_be_planned_in_a_steam_prefix() {
    use easyplay_core::clock::Timestamp;
    use easyplay_core::launch::{HostEnv, LaunchTarget, PlanRequest, plan_launch_in_prefix};
    use easyplay_core::model::{ExeRole, Executable, Fingerprint, GameFlags, LaunchConfig, PeInfo};
    use easyplay_testkit::fixtures::hosts::SandboxWorld;

    let w = SandboxWorld::worked_example();
    let compat = PathBuf::from("/mnt/games/SteamLibrary/steamapps/compatdata/1174180");
    w.fs.dir(compat.join("pfx/drive_c"));
    let now = Timestamp::from_millis(1_794_480_000_000);
    let id = GameId::steam(1_174_180);
    let game = Game {
        id,
        name: "Red Dead Redemption 2".into(),
        sort_name: "red dead redemption 2".into(),
        name_is_user_set: false,
        root: w.root.clone(),
        added_at: now,
        updated_at: now,
        favorite: false,
        hidden: false,
        notes: String::new(),
        policy: easyplay_core::sandbox::offline(),
        policy_updated_at: now,
        launch: LaunchConfig::default(),
        umu_id: None,
        store: None,
        runtime: RuntimeRef::Default,
        runtime_follow_updates: false,
        runtime_last_resolved: None,
        prefix_tainted_at: None,
        prefix_prepared_with: None,
        flags: GameFlags::default(),
        last_launched_at: None,
        steam_app_id: Some(1_174_180),
        steam_app_id_is_user_set: false,
        executables: vec![Executable {
            id: "0192f0c2-1c3a-7d4e-8f00-0000000000e1".parse().unwrap(),
            role: ExeRole::Primary,
            path: w.exe.clone(),
            canonical_path: w.exe.clone(),
            rel_path: None,
            mount_point: None,
            fingerprint: Fingerprint {
                size: 2,
                mtime_ns: 0,
                quick_hash: [0; 32],
                full_hash: None,
            },
            pe: PeInfo::default(),
            args_override: None,
            label: None,
            added_at: now,
        }],
    };
    let env = HostEnv {
        uid: 1000,
        gid: 1000,
        user_name: "sam",
        group_name: "sam",
        home: Path::new(hosts::HOME),
        paths: &w.paths,
        host_env: &w.host_env,
        install_id: hosts::INSTALL_ID,
        init_binary: &w.init,
        developer_mode: false,
        seccomp_available: true,
        verbose_log: false,
        global_env: &[],
        user_dirs: &w.user_dirs,
        other_game_roots: &w.other_roots,
        mounts: &w.mounts,
        fs: &w.fs,
    };
    let tool = Tool::Winecfg;
    let req = PlanRequest {
        session_id: hosts::SESSION_ID.parse().unwrap(),
        target: LaunchTarget::Tool(&tool),
        game: Some(&game),
        runtime: Some(&w.runtime),
        proton_codename: None,
        network_override: None,
        host: &w.host,
        env: &env,
    };
    let planned = plan_launch_in_prefix(&req, &compat).unwrap();
    let args: Vec<String> = planned
        .plan
        .sandbox
        .args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let compat_s = compat.display().to_string();
    assert!(
        args.iter().any(|a| a == &compat_s),
        "the Steam prefix is bound"
    );
    let own = w.paths.prefix_dir(id).display().to_string();
    assert!(!args.iter().any(|a| a == &own), "not EasyPlay's own prefix");
    assert!(planned.sandbox.namespaces.net, "offline");
}
