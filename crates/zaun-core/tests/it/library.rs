//! Library: snapshot and index, queries, drafts, create/update with failure injection,
//! tags and flags, availability, re-link, removal, prefix operations, installer discovery,
//! export/import (LIB-001 … LIB-012), plus the PE icon source and the `steam_appid.txt`
//! reader of the artwork module (ART-003).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use proptest::prelude::*;
use zaun_core::artwork::steam::AppIdFile;
use zaun_core::artwork::{FsAppIdFile, IconSource, NoIcons, PeIconSource};
use zaun_core::clock::{Clock, SystemClock, Timestamp};
use zaun_core::error::ErrorCode;
use zaun_core::fsx::{DirEntry, Fs, FsErrorKind, FsOp, FsResult, Metadata, MountTable};
use zaun_core::identity::pe::{FsPeInspector, PeLimits};
use zaun_core::identity::{LibraryIndex, SharedLocations};
use zaun_core::ids::{GameId, TagName};
use zaun_core::library::draft::{DraftEnv, NoUmuTitles, build_draft};
use zaun_core::library::export::{self, ImportNote};
use zaun_core::library::index::{self as lindex, DefaultRuntimeInfo, natural_cmp};
use zaun_core::library::missing::{self, ScanTarget, ScanThrottle, ScanTrigger};
use zaun_core::library::prefix_ops::{self, PrefixProblem, ResetOptions};
use zaun_core::library::query::{
    Collection, Filters, GroupKey, Grouping, LibraryQuery, PresetFilter, SortKey, run_query,
};
use zaun_core::library::relink::{RelinkOutcome, Verdict};
use zaun_core::library::remove::{NoHooks, RemoveOptions};
use zaun_core::library::service::{NoPolicyCheck, PolicyCheck};
use zaun_core::library::{GameDraft, Library, LibraryDeps, LibrarySnapshot, installer};
use zaun_core::model::{
    Activity, Availability, BasePreset, EntryKind, ExeRole, FolderAccess, FolderGrant, Game,
    GameDirAccess, GameStats, GrantOrigin, NetworkAccess, SandboxPolicy, SaveDataLocation,
    SocketAccess, TagColor, UserDir, WorkingDir,
};
use zaun_core::paths::{AppPaths, PathEnv};
use zaun_core::sandbox::{IssueCode, PolicyIssue, presets};
use zaun_core::storage::{Db, ExecutableRepo, GamePatch, GameRepo, StatsRepo, TagRepo};
use zaun_testkit::fixtures::games::{self, BASE_MS};
use zaun_testkit::fixtures::lnk::{LnkSpec, build as build_lnk};
use zaun_testkit::fixtures::pe::{PeBuilder, sample_game};
use zaun_testkit::{FakeClock, MemFs};

// ---- Environment ----------------------------------------------------------------------------

const HOME: &str = "/home/sam";
const HK: &str = "/home/sam/Games/Hollow Knight/hollow_knight.exe";

fn paths() -> AppPaths {
    AppPaths::resolve(
        &PathEnv::new(1000)
            .with("HOME", HOME)
            .with("XDG_RUNTIME_DIR", "/run/user/1000"),
    )
    .unwrap()
}

fn shared() -> SharedLocations {
    SharedLocations {
        home: Some(HOME.into()),
        user_dirs: vec![("Downloads".into(), "/home/sam/Downloads".into())],
        temp_dirs: vec!["/tmp".into()],
        mount_points: vec!["/".into()],
        media_parents: vec!["/run/media/sam".into(), "/mnt".into()],
        zaun_dirs: vec!["/home/sam/.local/share/zaun".into()],
        game_roots: vec![],
        deny: None,
    }
}

/// A filesystem wrapper: records every path touched and can hang one subtree.
struct Wrap {
    inner: Arc<MemFs>,
    log: Mutex<Vec<PathBuf>>,
    slow: Option<(PathBuf, Duration)>,
}

impl Wrap {
    fn new(inner: Arc<MemFs>) -> Self {
        Self {
            inner,
            log: Mutex::new(Vec::new()),
            slow: None,
        }
    }
    fn touch(&self, p: &Path) {
        self.log.lock().push(p.to_path_buf());
        if let Some((s, d)) = &self.slow
            && p.starts_with(s)
        {
            std::thread::sleep(*d);
        }
    }
}

impl Fs for Wrap {
    fn metadata(&self, p: &Path) -> FsResult<Metadata> {
        self.touch(p);
        self.inner.metadata(p)
    }
    fn metadata_follow(&self, p: &Path) -> FsResult<Metadata> {
        self.touch(p);
        self.inner.metadata_follow(p)
    }
    fn read_link(&self, p: &Path) -> FsResult<PathBuf> {
        self.touch(p);
        self.inner.read_link(p)
    }
    fn read(&self, p: &Path, l: u64) -> FsResult<Vec<u8>> {
        self.touch(p);
        self.inner.read(p, l)
    }
    fn read_follow(&self, p: &Path, l: u64) -> FsResult<Vec<u8>> {
        self.touch(p);
        self.inner.read_follow(p, l)
    }
    fn read_range(&self, p: &Path, o: u64, l: usize) -> FsResult<Vec<u8>> {
        self.touch(p);
        self.inner.read_range(p, o, l)
    }
    fn open_read(&self, p: &Path) -> FsResult<Box<dyn Read + Send>> {
        self.touch(p);
        self.inner.open_read(p)
    }
    fn read_dir(&self, p: &Path) -> FsResult<Vec<DirEntry>> {
        self.touch(p);
        self.inner.read_dir(p)
    }
    fn canonicalize(&self, p: &Path) -> FsResult<PathBuf> {
        self.touch(p);
        self.inner.canonicalize(p)
    }
    fn create_dir(&self, p: &Path, m: u32) -> FsResult<()> {
        self.inner.create_dir(p, m)
    }
    fn create_dir_all(&self, p: &Path, m: u32) -> FsResult<()> {
        self.inner.create_dir_all(p, m)
    }
    fn write_new(&self, p: &Path, d: &[u8], m: u32) -> FsResult<()> {
        self.inner.write_new(p, d, m)
    }
    fn atomic_write(&self, p: &Path, d: &[u8], m: u32) -> FsResult<()> {
        self.inner.atomic_write(p, d, m)
    }
    fn rename(&self, a: &Path, b: &Path) -> FsResult<()> {
        self.inner.rename(a, b)
    }
    fn remove_file(&self, p: &Path) -> FsResult<()> {
        self.inner.remove_file(p)
    }
    fn remove_dir(&self, p: &Path) -> FsResult<()> {
        self.inner.remove_dir(p)
    }
    fn symlink(&self, t: &Path, l: &Path) -> FsResult<()> {
        self.inner.symlink(t, l)
    }
    fn set_mode(&self, p: &Path, m: u32) -> FsResult<()> {
        self.inner.set_mode(p, m)
    }
    fn free_space(&self, p: &Path) -> FsResult<u64> {
        self.inner.free_space(p)
    }
    fn reflink_supported(&self, a: &Path, b: &Path) -> FsResult<bool> {
        self.inner.reflink_supported(a, b)
    }
}

struct Env {
    fs: Arc<MemFs>,
    clock: Arc<FakeClock>,
    paths: AppPaths,
    lib: Library,
}

fn deps(fs: Arc<MemFs>, clock: Arc<FakeClock>, check: Arc<dyn PolicyCheck>) -> LibraryDeps {
    let dynfs: Arc<dyn Fs> = fs.clone();
    let dynclock: Arc<dyn Clock> = clock.clone();
    LibraryDeps {
        fs: dynfs.clone(),
        clock: dynclock.clone(),
        paths: paths(),
        runtime: Arc::new(DefaultRuntimeInfo),
        check,
        pe: Arc::new(FsPeInspector::new(dynfs, dynclock)),
        codec: None,
        mounts: MountTable::parse(fs.mountinfo().as_bytes()),
        shared: shared(),
        overlay_supported: true,
    }
}

fn env_with(fs: MemFs, check: Arc<dyn PolicyCheck>) -> Env {
    let fs = Arc::new(fs);
    let clock = Arc::new(FakeClock::at_millis(BASE_MS));
    let lib = Library::open(
        Db::open_in_memory().unwrap(),
        deps(fs.clone(), clock.clone(), check),
    )
    .unwrap();
    Env {
        fs,
        clock,
        paths: paths(),
        lib,
    }
}

fn env(fs: MemFs) -> Env {
    env_with(fs, Arc::new(NoPolicyCheck))
}

fn hk_fs() -> MemFs {
    let fs = MemFs::new();
    fs.file(HK, sample_game())
        .file("/home/sam/Games/Hollow Knight/steam_api64.dll", b"dll")
        .file("/home/sam/Games/Hollow Knight/steam_appid.txt", b"367520\n")
        .dir("/home/sam/Downloads");
    fs
}

fn draft(e: &Env, path: &str) -> GameDraft {
    let fs: Arc<dyn Fs> = e.fs.clone();
    let clock: Arc<dyn Clock> = e.clock.clone();
    let pe = FsPeInspector::new(fs.clone(), clock.clone());
    let icons = PeIconSource::new(Arc::new(pe.clone()));
    let mounts = MountTable::parse(e.fs.mountinfo().as_bytes());
    let shared = shared();
    let denv = DraftEnv {
        fs: &*fs,
        clock: &*clock,
        pe: &pe,
        limits: PeLimits::default(),
        icons: &icons,
        shared: &shared,
        mounts: &mounts,
        index: e.lib.exe_index(),
        base_policy: presets::offline(),
        umu: &NoUmuTitles,
        check: &NoPolicyCheck,
    };
    build_draft(&denv, Path::new(path)).unwrap()
}

fn tag(s: &str) -> TagName {
    TagName::new(s).unwrap()
}

// ---- LIB-001 snapshot and index ----------------------------------------------------------

fn seed(db: &mut Db, n: u32, now: Timestamp) -> Vec<Game> {
    let gs: Vec<Game> = (0..n).map(|i| games::game(i, 1 + i % 3)).collect();
    let t1 = db
        .create_tag(&tag("RPG"), TagColor::default(), now)
        .unwrap();
    let t2 = db
        .create_tag(&tag("Finished"), TagColor::new(3).unwrap(), now)
        .unwrap();
    for (i, g) in gs.iter().enumerate() {
        db.insert_game(g).unwrap();
        let tags: Vec<_> = [t1.id, t2.id].into_iter().take(i % 3).collect();
        db.set_game_tags(g.id, &tags).unwrap();
        if i % 2 == 0 {
            db.put_game_stats(&GameStats {
                total_ms: 1000 * i as u64,
                sessions: i as u32,
                last_played_at: Some(Timestamp::from_millis(BASE_MS - i as i64 * 86_400_000)),
                ..GameStats::empty(g.id)
            })
            .unwrap();
        }
    }
    gs
}

fn full(n: u32) -> LibrarySnapshot {
    let fs = Arc::new(MemFs::new());
    let clock = Arc::new(FakeClock::at_millis(BASE_MS));
    let mut db = Db::open_in_memory().unwrap();
    seed(&mut db, n, clock.now());
    let lib = Library::open(db, deps(fs, clock, Arc::new(NoPolicyCheck))).unwrap();
    (*lib.snapshot()).clone()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]
    #[test]
    fn incremental_equals_full_rebuild(order in Just((0..14u32).collect::<Vec<_>>()).prop_shuffle()) {
        let n = order.len() as u32;
        let expected = full(n);
        // Same rows, reached one game at a time in a random order, with a detour.
        let fs = Arc::new(MemFs::new());
        let clock = Arc::new(FakeClock::at_millis(BASE_MS));
        let mut lib = Library::open(Db::open_in_memory().unwrap(), deps(fs, clock.clone(), Arc::new(NoPolicyCheck))).unwrap();
        let mut scratch = Db::open_in_memory().unwrap();
        let gs = seed(&mut scratch, n, clock.now());
        let now = clock.now();
        let db = lib.db_mut();
        let t1 = db.create_tag(&tag("RPG"), TagColor::default(), now).unwrap();
        let t2 = db.create_tag(&tag("Finished"), TagColor::new(3).unwrap(), now).unwrap();
        lib.refresh_tags().unwrap();
        for i in order {
            let g = &gs[i as usize];
            let db = lib.db_mut();
            db.insert_game(g).unwrap();
            lib.refresh_game(g.id).unwrap();
            let db = lib.db_mut();
            let tags: Vec<_> = [t1.id, t2.id].into_iter().take(i as usize % 3).collect();
            db.set_game_tags(g.id, &tags).unwrap();
            if let Some(st) = scratch.game_stats(g.id).unwrap() {
                db.put_game_stats(&st).unwrap();
            }
            lib.refresh_game(g.id).unwrap();
        }
        let got = lib.snapshot();
        prop_assert!(got.same_content(&expected), "incremental differs from full build");
        prop_assert!(got.revision > expected.revision);
        // Removing and re-adding one game is also equivalent.
        let g = gs[0].clone();
        lib.db_mut().purge_game(g.id, false).unwrap();
        lib.refresh_game(g.id).unwrap();
        prop_assert_eq!(lib.snapshot().games.len(), n as usize - 1);
    }
}

#[test]
fn snapshot_shares_strings_and_counts() {
    let fs = Arc::new(MemFs::new());
    let clock = Arc::new(FakeClock::at_millis(BASE_MS));
    let mut db = Db::open_in_memory().unwrap();
    let gs = seed(&mut db, 12, clock.now());
    let mut lib = Library::open(db, deps(fs, clock, Arc::new(NoPolicyCheck))).unwrap();
    let before = lib.snapshot();
    lib.update(
        gs[1].id,
        GamePatch {
            favorite: Some(true),
            ..GamePatch::default()
        },
    )
    .unwrap();
    let after = lib.snapshot();
    let other = gs[2].id;
    assert!(Arc::ptr_eq(
        &before.get(other).unwrap().name,
        &after.get(other).unwrap().name
    ));
    assert!(Arc::ptr_eq(
        &before.get(gs[1].id).unwrap().name,
        &after.get(gs[1].id).unwrap().name
    ));
    // fixtures: hidden when n % 11 == 0 (games 0 and 11), favourite when n % 5 == 0 (0, 5, 10)
    assert_eq!(after.totals.hidden, 2);
    assert_eq!(after.totals.all, 10);
    assert_eq!(after.totals.favorites, 3, "5, 10 and now 1 (0 is hidden)");
    let rpg = after.tags.iter().find(|t| &*t.name == "RPG").unwrap();
    assert!(rpg.count > 0);
    // Overlays publish without touching the database.
    let rev = after.revision;
    lib.set_activity(
        other,
        Activity::Running {
            since: Timestamp::from_millis(BASE_MS),
        },
    );
    lib.set_availability(&[(other, Availability::Missing)]);
    let s = lib.snapshot();
    assert_eq!(s.revision, rev + 2);
    assert_eq!(s.totals.running, 1);
    assert_eq!(s.totals.missing, 1);
    assert_eq!(s.get(other).unwrap().availability, Availability::Missing);
    // The matcher index knows every executable.
    let e = &gs[2].executables[0];
    assert_eq!(
        lib.exe_index().by_path(&e.canonical_path).unwrap().game,
        gs[2].id
    );
    assert_eq!(lib.exe_index().roots().len(), 12);
}

#[test]
fn full_build_budget_smoke() {
    // PERFORMANCE §4: 5 000 games < 30 ms in release; here only a smoke run on 2 000.
    let fs = Arc::new(MemFs::new());
    let clock = Arc::new(FakeClock::at_millis(BASE_MS));
    let mut db = Db::open_in_memory().unwrap();
    seed(&mut db, 2000, clock.now());
    let lib = Library::open(db, deps(fs, clock, Arc::new(NoPolicyCheck))).unwrap();
    let s = lib.snapshot();
    assert_eq!(s.games.len(), 2000);
    let sys = SystemClock::new();
    let t = sys.mono();
    let r = run_query(
        &s,
        &LibraryQuery {
            text: "hol kn".into(),
            ..LibraryQuery::default()
        },
    );
    assert!(!r.items.is_empty());
    assert!(sys.mono().saturating_duration_since(t) < Duration::from_millis(500));
}

// ---- LIB-002 queries ---------------------------------------------------------------------

fn named(names: &[&str]) -> (Library, Vec<GameId>) {
    let fs = Arc::new(MemFs::new());
    let clock = Arc::new(FakeClock::at_millis(BASE_MS));
    let mut db = Db::open_in_memory().unwrap();
    let now = clock.now();
    let coop = db
        .create_tag(&tag("Open World"), TagColor::default(), now)
        .unwrap();
    let mut ids = Vec::new();
    for (i, n) in names.iter().enumerate() {
        let mut g = games::game(i as u32 + 1, 1);
        g.name = (*n).to_owned();
        g.sort_name = zaun_core::identity::naming::sort_name(n);
        g.hidden = false;
        g.favorite = i == 1;
        if i == 2 {
            g.policy = presets::online();
        }
        db.insert_game(&g).unwrap();
        if i == 0 {
            db.set_game_tags(g.id, &[coop.id]).unwrap();
        }
        if i < 3 {
            db.put_game_stats(&GameStats {
                total_ms: (3 - i as u64) * 1000,
                sessions: 1,
                last_played_at: Some(Timestamp::from_millis(BASE_MS - i as i64 * 1000)),
                ..GameStats::empty(g.id)
            })
            .unwrap();
        }
        ids.push(g.id);
    }
    let lib = Library::open(db, deps(fs, clock, Arc::new(NoPolicyCheck))).unwrap();
    (lib, ids)
}

fn names_of(s: &LibrarySnapshot, q: &LibraryQuery) -> Vec<String> {
    run_query(s, q)
        .items
        .iter()
        .map(|i| s.games[*i as usize].name.to_string())
        .collect()
}

fn q(text: &str) -> LibraryQuery {
    LibraryQuery {
        text: text.into(),
        ..LibraryQuery::default()
    }
}

#[test]
fn search_fields_operators_and_ranking() {
    let (lib, _) = named(&[
        "Grand Theft Auto V",
        "Final Fantasy VII",
        "Pokémon & Friends",
        "Game 10",
        "Game 2",
        "The Witcher 3",
    ]);
    let s = lib.snapshot();
    assert_eq!(names_of(&s, &q("final fantasy 7")), ["Final Fantasy VII"]);
    assert_eq!(names_of(&s, &q("gta")), ["Grand Theft Auto V"]);
    assert_eq!(names_of(&s, &q("pokemon and")), ["Pokémon & Friends"]);
    assert_eq!(names_of(&s, &q("witcher iii")), ["The Witcher 3"]);
    // Tag word prefix (rank 40) and operator.
    assert_eq!(names_of(&s, &q("open")), ["Grand Theft Auto V"]);
    assert_eq!(names_of(&s, &q(r#"tag:"open wo""#)), ["Grand Theft Auto V"]);
    assert_eq!(names_of(&s, &q("tag:open -gta")), Vec::<String>::new());
    assert_eq!(names_of(&s, &q("is:favorite")), ["Final Fantasy VII"]);
    assert_eq!(names_of(&s, &q("is:online")), ["Pokémon & Friends"]);
    assert_eq!(names_of(&s, &q("preset:online")), ["Pokémon & Friends"]);
    assert_eq!(names_of(&s, &q("-is:never-played")).len(), 3);
    assert_eq!(names_of(&s, &q("layer:default")).len(), 6);
    // Executable file name (rank 20): every fixture primary is game.exe.
    assert_eq!(
        names_of(&s, &q("game")).first().map(String::as_str),
        Some("Game 2")
    );
    // Word prefix (100) outranks substring (60).
    let (lib2, _) = named(&["Mega Man", "Omega Quest"]);
    assert_eq!(
        names_of(&lib2.snapshot(), &q("mega")),
        ["Mega Man", "Omega Quest"]
    );
}

#[test]
fn sorting_collections_and_grouping() {
    let (mut lib, ids) = named(&["Game 10", "Game 2", "alpha", "Zeta"]);
    let s = lib.snapshot();
    assert_eq!(
        names_of(&s, &LibraryQuery::default()),
        ["alpha", "Game 2", "Game 10", "Zeta"]
    );
    let by = |key: SortKey| LibraryQuery {
        sort: key,
        descending: key.default_descending(),
        ..LibraryQuery::default()
    };
    // Last played newest first; never played last.
    assert_eq!(
        names_of(&s, &by(SortKey::LastPlayed)),
        ["Game 10", "Game 2", "alpha", "Zeta"]
    );
    assert_eq!(names_of(&s, &by(SortKey::Playtime))[0], "Game 10");
    assert_eq!(
        names_of(
            &s,
            &LibraryQuery {
                collection: Collection::NeverPlayed,
                ..LibraryQuery::default()
            }
        ),
        ["Zeta"]
    );
    assert_eq!(
        names_of(
            &s,
            &LibraryQuery {
                collection: Collection::Favorites,
                ..LibraryQuery::default()
            }
        ),
        ["Game 2"]
    );
    assert_eq!(
        names_of(
            &s,
            &LibraryQuery {
                collection: Collection::Recent,
                ..LibraryQuery::default()
            }
        )
        .len(),
        3
    );
    let f = LibraryQuery {
        filters: Filters {
            presets: vec![PresetFilter::Online],
            ..Filters::default()
        },
        ..LibraryQuery::default()
    };
    assert_eq!(names_of(&s, &f), ["alpha"]);
    // Grouping by first letter and by tag (a game under each of its tags, "No tags" last).
    let g = run_query(
        &s,
        &LibraryQuery {
            group: Grouping::FirstLetter,
            ..LibraryQuery::default()
        },
    );
    let keys: Vec<_> = g.groups.iter().map(|g| g.key.clone()).collect();
    assert_eq!(
        keys,
        [
            GroupKey::Letter('A'),
            GroupKey::Letter('G'),
            GroupKey::Letter('Z')
        ]
    );
    assert_eq!(g.groups[1].len, 2);
    lib.set_tags(ids[1], &[tag("Open World"), tag("Short")])
        .unwrap();
    let s = lib.snapshot();
    let g = run_query(
        &s,
        &LibraryQuery {
            group: Grouping::Tag,
            ..LibraryQuery::default()
        },
    );
    let labels: Vec<_> = g.groups.iter().map(|g| g.label.to_string()).collect();
    assert_eq!(labels, ["Open World", "Short", "No tags"]);
    assert_eq!(g.items.len(), 5);
    assert_eq!(g.revision, s.revision);
    assert_eq!(natural_cmp("a2", "a10"), std::cmp::Ordering::Less);
}

// ---- LIB-003 draft ---------------------------------------------------------------------------

#[test]
fn drafts_for_fixture_files() {
    let fs = hk_fs();
    fs.file(
        "/home/sam/Downloads/setup_cool.exe",
        PeBuilder::new()
            .string("ProductName", "Cool Game Setup")
            .build(),
    )
    .file(
        "/home/sam/Downloads/readme.txt",
        b"this is not a windows program at all, it is text that is long enough",
    )
    .file(
        "/home/sam/Games/Thing/bin/thing.exe",
        PeBuilder::new().string("ProductName", "Thing").build(),
    );
    let e = env(fs);
    let d = draft(&e, HK);
    assert_eq!(d.name, "Hollow Knight");
    assert_eq!(
        d.root.as_deref(),
        Some(Path::new("/home/sam/Games/Hollow Knight"))
    );
    assert_eq!(d.policy.game_dir, GameDirAccess::ReadWrite);
    assert_eq!(
        d.policy.network,
        NetworkAccess::None,
        "hints never loosen the policy"
    );
    assert!(d.hints.online_capable);
    assert!(d.hints.exe.steam_api);
    assert_eq!(d.kind, EntryKind::Game);
    let ico = d.icon.as_ref().expect("embedded icon");
    assert_eq!(&ico[..4], &[0, 0, 1, 0]);
    assert_eq!(d.launch.working_dir, WorkingDir::ExeDir);
    assert!(d.issues.is_empty());

    let inst = draft(&e, "/home/sam/Downloads/setup_cool.exe");
    assert_eq!(inst.kind, EntryKind::Installer);
    assert_eq!(inst.root, None, "Downloads is a shared location");
    assert_eq!(inst.policy.game_dir, GameDirAccess::FileOnly);

    let bin = draft(&e, "/home/sam/Games/Thing/bin/thing.exe");
    assert_eq!(
        bin.root.as_deref(),
        Some(Path::new("/home/sam/Games/Thing"))
    );
    assert_eq!(
        bin.to_game(
            GameId::generate(&*e.clock),
            zaun_core::ids::ExecutableId::generate(&*e.clock),
            e.clock.now()
        )
        .executables[0]
            .rel_path
            .as_deref(),
        Some(Path::new("bin/thing.exe"))
    );

    // Not a Windows program.
    let fsd: Arc<dyn Fs> = e.fs.clone();
    let pe = FsPeInspector::new(fsd.clone(), e.clock.clone());
    let mounts = MountTable::default();
    let sh = shared();
    let denv = DraftEnv {
        fs: &*fsd,
        clock: &*e.clock,
        pe: &pe,
        limits: PeLimits::default(),
        icons: &NoIcons,
        shared: &sh,
        mounts: &mounts,
        index: e.lib.exe_index(),
        base_policy: presets::offline(),
        umu: &NoUmuTitles,
        check: &NoPolicyCheck,
    };
    let err = build_draft(&denv, Path::new("/home/sam/Downloads/readme.txt")).unwrap_err();
    assert_eq!(err.code, ErrorCode::IdNotWindowsProgram);
}

// ---- LIB-004 create and update -------------------------------------------------------------

#[test]
fn create_commits_rows_dirs_and_snapshot() {
    let mut e = env(hk_fs());
    let mut d = draft(&e, HK);
    d.tags = vec![tag("Metroidvania")];
    let r = e.lib.create(&d).unwrap();
    assert!(r.artwork_error.is_none());
    let p = &e.paths;
    for dir in [p.game_dir(r.id), p.home_dir(r.id), p.prefix_dir(r.id)] {
        let md = e.fs.metadata(&dir).unwrap();
        assert!(md.is_dir());
        assert_eq!(md.mode & 0o777, 0o700);
    }
    let s = e.lib.snapshot();
    let g = s.get(r.id).unwrap();
    assert_eq!(&*g.name, "Hollow Knight");
    assert_eq!(g.tags.len(), 1);
    assert_eq!(s.tags[0].count, 1);
    // A second create of the same file is refused.
    let err = e.lib.create(&d).unwrap_err();
    assert_eq!(err.code, ErrorCode::LibAlreadyInLibrary);
    // The matcher sees it.
    let fp = e.lib.db_mut().executable(r.exe).unwrap().unwrap().1;
    assert_eq!(
        e.lib.exe_index().by_hash(&fp.fingerprint.quick_hash).len(),
        1
    );
}

#[test]
fn failed_directory_creation_leaves_no_partial_game() {
    let fs = hk_fs();
    fs.fail(FsOp::CreateDir, paths().games_dir, FsErrorKind::NoSpace);
    let mut e = env(fs);
    let d = draft(&e, HK);
    let err = e.lib.create(&d).unwrap_err();
    assert_eq!(err.code, ErrorCode::FsDiskFull);
    assert_eq!(e.lib.db_mut().game_count().unwrap(), 0);
    assert!(
        e.lib
            .db_mut()
            .executable_by_canonical_path(Path::new(HK))
            .unwrap()
            .is_none()
    );
    assert!(e.lib.snapshot().games.is_empty());
    e.fs.clear_faults();
    assert!(
        e.lib.create(&d).is_ok(),
        "nothing left behind blocks a retry"
    );
}

struct Blocking;
impl PolicyCheck for Blocking {
    fn issues(&self, game: &Game, _: &[PathBuf]) -> Vec<PolicyIssue> {
        if game.policy.network == NetworkAccess::Full {
            vec![PolicyIssue::error(
                IssueCode::NoDisplay,
                None,
                "test|blocked",
            )]
        } else {
            vec![]
        }
    }
}

#[test]
fn updates_are_validated_with_cross_field_rules() {
    let mut e = env_with(hk_fs(), Arc::new(Blocking));
    let id = e.lib.create(&draft(&e, HK)).unwrap().id;
    let game = e.lib.game(id).unwrap();
    let with_policy = |f: &dyn Fn(&mut SandboxPolicy)| {
        let mut p = game.policy.clone();
        f(&mut p);
        GamePatch {
            policy: Some(p),
            ..GamePatch::default()
        }
    };
    let code = |e: &mut Env, p: GamePatch| e.lib.update(id, p).unwrap_err().code;
    // Rule 2: custom working directory outside every mounted location.
    let mut launch = game.launch.clone();
    launch.working_dir = WorkingDir::Custom("/etc".into());
    assert_eq!(
        code(
            &mut e,
            GamePatch {
                launch: Some(launch),
                ..GamePatch::default()
            }
        ),
        ErrorCode::LibInvalidField
    );
    // Rule 3: a folder game cannot be "this file only".
    assert_eq!(
        code(
            &mut e,
            with_policy(&|p| p.game_dir = GameDirAccess::FileOnly)
        ),
        ErrorCode::LibInvalidField
    );
    // Rule 5: some display is needed.
    assert_eq!(
        code(
            &mut e,
            with_policy(&|p| {
                p.display.wayland = SocketAccess::Deny;
                p.display.x11 = SocketAccess::Deny;
            })
        ),
        ErrorCode::LibInvalidField
    );
    // Rule 7: saves inside a read-only game folder.
    assert_eq!(
        code(
            &mut e,
            with_policy(&|p| {
                p.game_dir = GameDirAccess::ReadOnly;
                p.save_data = SaveDataLocation::External {
                    dir: "/home/sam/Games/Hollow Knight/saves".into(),
                };
            })
        ),
        ErrorCode::LibInvalidField
    );
    // A blocking policy issue from the host check.
    assert_ne!(
        code(&mut e, with_policy(&|p| p.network = NetworkAccess::Full)),
        ErrorCode::LibInvalidField
    );
    // Name limits.
    assert_eq!(
        code(
            &mut e,
            GamePatch {
                name: Some("x".repeat(81)),
                ..GamePatch::default()
            }
        ),
        ErrorCode::LibInvalidField
    );
    // Nothing changed so far.
    assert_eq!(e.lib.game(id).unwrap().policy, game.policy);
    // A valid edit: name trimmed, sort name derived, snapshot updated.
    let mut launch = game.launch.clone();
    launch.working_dir = WorkingDir::Custom(e.paths.prefix_dir(id).join("drive_c"));
    e.lib
        .update(
            id,
            GamePatch {
                name: Some("  The Hollow Knight ".into()),
                launch: Some(launch),
                ..GamePatch::default()
            },
        )
        .unwrap();
    let g = e.lib.game(id).unwrap();
    assert_eq!(g.name, "The Hollow Knight");
    assert_eq!(
        g.sort_name,
        zaun_core::identity::naming::sort_name("The Hollow Knight")
    );
    assert_eq!(
        &*e.lib.snapshot().get(id).unwrap().name,
        "The Hollow Knight"
    );
}

#[test]
fn executables_tags_flags_and_limits() {
    let fs = hk_fs();
    for i in 0..33 {
        fs.file(
            format!("/home/sam/Games/Hollow Knight/tool{i}.exe"),
            PeBuilder::new()
                .string("ProductName", &format!("Tool {i}"))
                .build(),
        );
    }
    let mut e = env(fs);
    let id = e.lib.create(&draft(&e, HK)).unwrap().id;
    let alt = e
        .lib
        .add_executable(
            id,
            Path::new("/home/sam/Games/Hollow Knight/tool0.exe"),
            ExeRole::Primary,
        )
        .unwrap();
    let g = e.lib.game(id).unwrap();
    assert_eq!(g.primary().unwrap().id, alt);
    assert_eq!(g.executables.len(), 2);
    for i in 1..31 {
        e.lib
            .add_executable(
                id,
                Path::new(&format!("/home/sam/Games/Hollow Knight/tool{i}.exe")),
                ExeRole::Tool,
            )
            .unwrap();
    }
    let err = e
        .lib
        .add_executable(
            id,
            Path::new("/home/sam/Games/Hollow Knight/tool32.exe"),
            ExeRole::Tool,
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::LibInvalidField);
    assert_eq!(err.context_value("problem"), Some("TooMany { max: 32 }"));

    // Tags: 16 per game.
    let many: Vec<TagName> = (0..17).map(|i| tag(&format!("t{i}"))).collect();
    assert_eq!(
        e.lib.set_tags(id, &many).unwrap_err().code,
        ErrorCode::LibInvalidField
    );
    let undo = e.lib.set_tags(id, &many[..16]).unwrap();
    assert!(undo.previous.is_empty());
    assert_eq!(e.lib.snapshot().get(id).unwrap().tags.len(), 16);
    e.lib.undo_tags(&undo).unwrap();
    assert!(e.lib.snapshot().get(id).unwrap().tags.is_empty());
    // Duplicate names are refused, renaming onto an existing name merges.
    let a = e
        .lib
        .create_tag(&tag("Alpha"), TagColor::default())
        .unwrap();
    assert_eq!(
        e.lib
            .create_tag(&tag("alpha"), TagColor::default())
            .unwrap_err()
            .code,
        ErrorCode::LibInvalidField
    );
    let b = e.lib.create_tag(&tag("Beta"), TagColor::default()).unwrap();
    e.lib.add_tag_to_games(b, &[id]).unwrap();
    assert_eq!(e.lib.rename_tag(b, &tag("ALPHA")).unwrap(), a);
    assert_eq!(e.lib.snapshot().get(id).unwrap().tags.as_slice(), &[a]);
    // Favourite and hidden with undo.
    let u = e.lib.set_favorite(&[id], true).unwrap();
    assert!(e.lib.snapshot().get(id).unwrap().favorite);
    e.lib.undo_favorite(&u).unwrap();
    assert!(!e.lib.snapshot().get(id).unwrap().favorite);
    e.lib.set_hidden(&[id], true).unwrap();
    assert_eq!(e.lib.snapshot().totals.hidden, 1);
    // Flow lock.
    e.lib.begin_flow(id).unwrap();
    assert_eq!(e.lib.begin_flow(id).unwrap_err().code, ErrorCode::LibBusy);
    e.lib.end_flow(id);
    assert!(e.lib.begin_flow(id).is_ok());
}

// ---- LIB-012 orphans ----------------------------------------------------------------------------

#[test]
fn orphan_sweep_respects_age_and_content() {
    let fs = MemFs::new();
    let p = paths();
    let old_ns = (BASE_MS - 2 * 3_600_000) * 1_000_000;
    let young = p.games_dir.join(games::game_id(90).to_string());
    let empty = p.games_dir.join(games::game_id(91).to_string());
    let full = p.games_dir.join(games::game_id(92).to_string());
    fs.set_now(old_ns)
        .dir(empty.join("home"))
        .dir(empty.join("prefix"))
        .file(full.join("prefix/drive_c/save.dat"), b"x")
        .file(p.data_dir.join("library.db.tmp-abc"), b"partial")
        .set_now(BASE_MS * 1_000_000)
        .dir(young.join("prefix"))
        .file(p.data_dir.join("config.tmp-new"), b"partial");
    let mut e = env(fs);
    let r = e.lib.sweep_orphans().unwrap();
    assert_eq!(r.removed, vec![empty.clone()]);
    assert_eq!(r.listed.len(), 1);
    assert_eq!(r.listed[0].path, full);
    assert_eq!(r.temp_files_removed, 1);
    assert!(e.fs.exists(&young), "younger than one hour");
    assert!(e.fs.exists(&full));
    assert!(!e.fs.exists(&empty));
    assert!(e.fs.exists(&p.data_dir.join("config.tmp-new")));
    e.lib.delete_orphan(&r.listed[0]).unwrap();
    assert!(!e.fs.exists(&full));
}

// ---- LIB-006 availability -------------------------------------------------------------------

#[test]
fn hanging_mount_delays_only_its_own_games() {
    let mem = Arc::new(MemFs::new());
    mem.file("/home/sam/Games/a.exe", b"MZ")
        .mount("/mnt/nas", 7, "nfs")
        .file("/mnt/nas/Games/b.exe", b"MZ")
        .dir("/run/media/sam")
        .file("/home/sam/locked/c.exe", b"MZ")
        .fail(
            FsOp::MetadataFollow,
            "/home/sam/locked",
            FsErrorKind::PermissionDenied,
        );
    let mounts = MountTable::parse(mem.mountinfo().as_bytes());
    let mut wrap = Wrap::new(mem);
    wrap.slow = Some(("/mnt/nas".into(), Duration::from_secs(3)));
    let fs: Arc<dyn Fs> = Arc::new(wrap);
    let id = |n| games::game_id(n);
    let t = |n, p: &str, mp: Option<&str>| ScanTarget {
        game: id(n),
        path: p.into(),
        mount_point: mp.map(PathBuf::from),
    };
    let targets = vec![
        t(0, "/home/sam/Games/a.exe", Some("/")),
        t(1, "/mnt/nas/Games/b.exe", Some("/mnt/nas")),
        t(
            2,
            "/run/media/sam/Drive/game.exe",
            Some("/run/media/sam/Drive"),
        ),
        t(3, "/run/media/sam/Other/game.exe", None),
        t(4, "/home/sam/Games/gone.exe", Some("/")),
        t(5, "/home/sam/locked/c.exe", None),
    ];
    let clock = SystemClock::new();
    let start = clock.mono();
    let r = missing::scan(
        fs,
        &clock,
        &mounts,
        &missing::media_roots("sam"),
        &targets,
        Duration::from_millis(300),
    );
    let took = clock.mono().saturating_duration_since(start);
    assert!(took < Duration::from_secs(2), "took {took:?}");
    assert_eq!(
        r.iter().map(|(_, a)| *a).collect::<Vec<_>>(),
        [
            Availability::Present,
            Availability::Unreadable,
            Availability::DriveDisconnected,
            Availability::DriveDisconnected,
            Availability::Missing,
            Availability::Unreadable,
        ]
    );
    let mut th = ScanThrottle::default();
    let t0 = zaun_core::clock::MonoInstant::from_duration(Duration::from_secs(100));
    assert!(th.should_scan(ScanTrigger::Startup, t0));
    assert!(!th.should_scan(
        ScanTrigger::Focus,
        t0.saturating_add(Duration::from_secs(30))
    ));
    assert!(th.should_scan(
        ScanTrigger::Manual,
        t0.saturating_add(Duration::from_secs(31))
    ));
    assert!(th.should_scan(
        ScanTrigger::Focus,
        t0.saturating_add(Duration::from_secs(95))
    ));
}

// ---- LIB-007 relink -------------------------------------------------------------------------

#[test]
fn moving_a_game_updates_every_path_atomically() {
    let fs = MemFs::new();
    let exe_bytes = PeBuilder::new()
        .string("ProductName", "Old Game")
        .string("CompanyName", "Acme")
        .build();
    fs.file("/games/old/Quest/bin/game.exe", &exe_bytes)
        .file(
            "/games/old/Quest/bin/config.exe",
            PeBuilder::new().string("ProductName", "Config").build(),
        )
        .dir("/games/old/Quest/mods");
    let mut e = env(fs);
    let id = e
        .lib
        .create(&draft(&e, "/games/old/Quest/bin/game.exe"))
        .unwrap()
        .id;
    assert_eq!(
        e.lib.game(id).unwrap().root.as_deref(),
        Some(Path::new("/games/old/Quest"))
    );
    e.lib
        .add_executable(
            id,
            Path::new("/games/old/Quest/bin/config.exe"),
            ExeRole::Tool,
        )
        .unwrap();
    let mut g = e.lib.game(id).unwrap();
    g.policy.folders.push(FolderGrant {
        path: "/games/old/Quest/mods".into(),
        canonical: "/games/old/Quest/mods".into(),
        access: FolderAccess::ReadWrite,
        origin: GrantOrigin::UserChosen,
        drive_letter: Some('m'),
        granted_at: e.clock.now(),
    });
    e.lib
        .update(
            id,
            GamePatch {
                policy: Some(g.policy.clone()),
                ..GamePatch::default()
            },
        )
        .unwrap();
    let primary = g.primary().unwrap().id;

    // Same content at a new place.
    e.fs.rename(Path::new("/games/old"), Path::new("/games/new"))
        .unwrap();
    let r = e
        .lib
        .relink(primary, Path::new("/games/new/Quest/bin/game.exe"), false)
        .unwrap();
    assert_eq!(
        r,
        RelinkOutcome::Relinked {
            game: id,
            new_root: Some("/games/new/Quest".into()),
            verdict: Verdict::SameContent
        }
    );
    let g = e.lib.game(id).unwrap();
    assert_eq!(g.root.as_deref(), Some(Path::new("/games/new/Quest")));
    for x in &g.executables {
        assert!(
            x.canonical_path.starts_with("/games/new/Quest/bin"),
            "{:?}",
            x.canonical_path
        );
    }
    assert_eq!(
        g.policy.folders[0].canonical,
        Path::new("/games/new/Quest/mods")
    );
    assert_eq!(
        &*e.lib.snapshot().get(id).unwrap().primary_exe_path,
        Path::new("/games/new/Quest/bin/game.exe")
    );

    // A new version (same identity) needs confirmation.
    e.fs.file(
        "/games/v2/Quest/bin/game.exe",
        PeBuilder::new()
            .string("ProductName", "Old Game")
            .string("CompanyName", "Acme")
            .timestamp(7)
            .build(),
    );
    let r = e
        .lib
        .relink(primary, Path::new("/games/v2/Quest/bin/game.exe"), false)
        .unwrap();
    assert_eq!(
        r,
        RelinkOutcome::NeedsConfirmation {
            game: id,
            verdict: Verdict::NewVersion
        }
    );
    assert_eq!(
        e.lib.game(id).unwrap().root.as_deref(),
        Some(Path::new("/games/new/Quest")),
        "nothing changed"
    );
    // A different program: stronger wording; confirmed, it is accepted.
    e.fs.file(
        "/games/other/x.exe",
        PeBuilder::new().string("ProductName", "Other").build(),
    );
    let r = e
        .lib
        .relink(primary, Path::new("/games/other/x.exe"), false)
        .unwrap();
    assert_eq!(
        r,
        RelinkOutcome::NeedsConfirmation {
            game: id,
            verdict: Verdict::DifferentProgram
        }
    );
    let r = e
        .lib
        .relink(primary, Path::new("/games/v2/Quest/bin/game.exe"), true)
        .unwrap();
    assert!(matches!(
        r,
        RelinkOutcome::Relinked {
            verdict: Verdict::NewVersion,
            ..
        }
    ));
    assert_eq!(
        e.lib.game(id).unwrap().root.as_deref(),
        Some(Path::new("/games/v2/Quest"))
    );
}

// ---- LIB-008 removal ------------------------------------------------------------------------

#[test]
fn removal_touches_only_the_games_own_directories() {
    let fs = hk_fs();
    fs.file("/home/sam/precious/keep.txt", b"keep");
    let mut e = env(fs);
    let id = e.lib.create(&draft(&e, HK)).unwrap().id;
    let p = e.paths.clone();
    let prefix = p.prefix_dir(id);
    e.fs.file(
        prefix.join("drive_c/users/steamuser/Saved Games/slot1.sav"),
        b"save",
    )
    .link(prefix.join("dosdevices/z:"), "/")
    .link(prefix.join("drive_c/evil"), "/home/sam/precious")
    .link(p.home_dir(id).join("realhome"), HOME)
    .file(
        p.artwork_game_dir(id).join("icon-0123456789abcdef.png"),
        b"png",
    )
    .file(
        "/home/sam/Games/Hollow Knight/save_in_game_folder.dat",
        b"x",
    );
    let allowed = |x: &Path| {
        x.starts_with(p.game_dir(id))
            || x.starts_with(p.artwork_game_dir(id))
            || x.starts_with(&p.saved_data_dir)
    };
    let before: Vec<PathBuf> = e.fs.paths().into_iter().filter(|x| !allowed(x)).collect();
    let r = e
        .lib
        .remove(id, RemoveOptions::default(), &NoHooks)
        .unwrap();
    assert!(
        r.complete,
        "{:?}",
        r.errors.iter().map(|e| e.code).collect::<Vec<_>>()
    );
    let kept = r.saved_data.unwrap();
    assert!(kept.starts_with(&p.saved_data_dir));
    assert_eq!(
        e.fs.contents(kept.join("Saved Games/slot1.sav")).unwrap(),
        b"save"
    );
    assert!(!e.fs.exists(&p.game_dir(id)));
    assert!(!e.fs.exists(&p.artwork_game_dir(id)));
    let after: Vec<PathBuf> =
        e.fs.paths()
            .into_iter()
            .filter(|x| !allowed(x))
            .filter(|x| !p.saved_data_dir.starts_with(x))
            .collect();
    let before: Vec<PathBuf> = before
        .into_iter()
        .filter(|x| !p.saved_data_dir.starts_with(x))
        .collect();
    assert_eq!(before, after, "nothing else was touched");
    assert_eq!(
        e.fs.contents("/home/sam/precious/keep.txt").unwrap(),
        b"keep"
    );
    assert!(e.lib.db_mut().game_record(id).unwrap().is_none(), "purged");
    assert!(e.lib.snapshot().get(id).is_none());
}

#[test]
fn failed_save_move_aborts_the_removal() {
    let mut e = env(hk_fs());
    let id = e.lib.create(&draft(&e, HK)).unwrap().id;
    let profile = e.paths.prefix_dir(id).join("drive_c/users/steamuser");
    e.fs.file(profile.join("save.dat"), b"save");
    e.fs.fail(FsOp::Rename, &profile, FsErrorKind::PermissionDenied);
    assert!(
        e.lib
            .remove(id, RemoveOptions::default(), &NoHooks)
            .is_err()
    );
    assert!(e.fs.exists(&profile.join("save.dat")));
    assert_eq!(
        e.lib.db_mut().game_record(id).unwrap().unwrap().deleted_at,
        None
    );
    assert!(e.lib.snapshot().get(id).is_some());
    // A crash after Detach: the start-up retry keeps the saves, then finishes.
    e.fs.clear_faults();
    e.lib
        .db_mut()
        .set_game_deleted(id, Some(e.clock.now()))
        .unwrap();
    assert_eq!(e.lib.retry_removals(true, &NoHooks).unwrap(), vec![id]);
    assert!(!e.fs.exists(&e.paths.game_dir(id)));
    let saved: Vec<_> =
        e.fs.paths()
            .into_iter()
            .filter(|x| x.starts_with(&e.paths.saved_data_dir) && x.ends_with("save.dat"))
            .collect();
    assert_eq!(saved.len(), 1);
}

// ---- LIB-009 prefix operations ---------------------------------------------------------------

fn prefix_env() -> (Env, GameId) {
    let mut e = env(hk_fs());
    let id = e.lib.create(&draft(&e, HK)).unwrap().id;
    let prefix = e.paths.prefix_dir(id);
    e.fs.file(
        prefix.join("drive_c/users/steamuser/AppData/save.dat"),
        b"save",
    )
    .file(prefix.join("drive_c/windows/system32/x.dll"), b"dll")
    .file(prefix.join("system.reg"), b"WINE REGISTRY Version 2\n")
    .file(prefix.join("user.reg"), b"WINE REGISTRY Version 2\n")
    .file(e.paths.home_dir(id).join(".config/x"), b"x");
    (e, id)
}

#[test]
fn reset_keeps_saves_and_rolls_back_on_failure() {
    let (mut e, id) = prefix_env();
    let p = e.paths.clone();
    let prefix = p.prefix_dir(id);
    let profile = prefix.join("drive_c/users/steamuser");
    // Failure at each rename leaves the tree exactly as it was.
    for (fail_at, opts) in [
        (profile.clone(), ResetOptions::default()),
        (
            prefix.clone(),
            ResetOptions {
                keep_save_data: false,
                also_reset_home: true,
            },
        ),
        (p.home_dir(id), ResetOptions::default()),
        (p.game_dir(id).join(".stash-users"), ResetOptions::default()),
    ] {
        let before = e.fs.paths();
        e.fs.fail_times(FsOp::Rename, &fail_at, FsErrorKind::PermissionDenied, 1);
        assert!(
            e.lib.reset_prefix(id, opts).is_err(),
            "failing at {fail_at:?}"
        );
        e.fs.clear_faults();
        assert_eq!(
            e.fs.paths(),
            before,
            "rolled back after failing at {fail_at:?}"
        );
    }
    e.lib
        .db_mut()
        .set_prefix_tainted_at(id, Some(e.clock.now()))
        .unwrap();
    let r = e.lib.reset_prefix(id, ResetOptions::default()).unwrap();
    assert!(r.kept_save_data);
    assert!(r.leftover_trash.is_empty());
    assert_eq!(
        e.fs.contents(profile.join("AppData/save.dat")).unwrap(),
        b"save"
    );
    assert!(!e.fs.exists(&prefix.join("drive_c/windows")));
    assert!(!e.fs.exists(&p.home_dir(id).join(".config")));
    assert!(
        e.fs.read_dir(&p.game_dir(id))
            .unwrap()
            .iter()
            .all(|d| !d.name.to_string_lossy().starts_with('.'))
    );
    let g = e.lib.game(id).unwrap();
    assert_eq!(g.prefix_tainted_at, None);
    assert_eq!(g.prefix_prepared_with, None);
}

#[test]
fn drive_letters_health_and_size() {
    let (e, id) = prefix_env();
    let prefix = e.paths.prefix_dir(id);
    e.fs.link(prefix.join("dosdevices/c:"), "../drive_c")
        .link(prefix.join("dosdevices/q:"), "/old/grant")
        .dir("/home/sam/Music");
    let grant = |l: char| FolderGrant {
        path: "/home/sam/Music".into(),
        canonical: "/home/sam/Music".into(),
        access: FolderAccess::ReadOnly,
        origin: GrantOrigin::WellKnown(UserDir::Music),
        drive_letter: Some(l),
        granted_at: e.clock.now(),
    };
    let r = prefix_ops::sync_drive_letters(&*e.fs, &prefix, &[grant('m')]).unwrap();
    assert_eq!(r.created, ['m']);
    assert_eq!(r.removed, ['q']);
    assert_eq!(
        e.fs.read_link(&prefix.join("dosdevices/m:")).unwrap(),
        Path::new("/home/sam/Music")
    );
    assert!(e.fs.exists(&prefix.join("dosdevices/c:")));
    assert_eq!(
        prefix_ops::sync_drive_letters(&*e.fs, &prefix, &[grant('m')]).unwrap(),
        Default::default()
    );
    e.fs.dir(prefix.join("dosdevices/n:"));
    assert!(prefix_ops::sync_drive_letters(&*e.fs, &prefix, &[grant('n')]).is_err());

    assert!(prefix_ops::prefix_health(&*e.fs, &prefix, true, 0).is_empty());
    e.fs.file(prefix.join("user.reg"), b"garbage");
    e.fs.file(prefix.join("system.reg"), b"");
    assert_eq!(
        prefix_ops::prefix_health(&*e.fs, &prefix, true, 2),
        [
            PrefixProblem::PrepareFailedTwice,
            PrefixProblem::RegistryEmpty("system.reg"),
            PrefixProblem::RegistryHeader("user.reg")
        ]
    );
    assert!(prefix_ops::log_indicates_corruption(
        "err: wine: could not load kernel32.dll, status c0000135"
    ));

    let size = prefix_ops::tree_size(
        &*e.fs,
        &*e.clock,
        &prefix,
        Duration::from_secs(5),
        &zaun_core::jobs::CancelToken::new(),
    )
    .unwrap();
    assert!(size.complete);
    assert!(size.bytes >= 4 + 3);
}

// ---- LIB-010 installer ---------------------------------------------------------------------

#[test]
fn installer_discovery_stays_inside_the_prefix_and_converts() {
    let fs = hk_fs();
    fs.file(
        "/home/sam/Downloads/setup_cool.exe",
        PeBuilder::new()
            .string("ProductName", "Cool Game Setup")
            .build(),
    )
    .file("/etc/secret.exe", b"MZ secret");
    let mut e = env(fs);
    let mem = e.fs.clone();
    let clock = e.clock.clone();
    let d = draft(&e, "/home/sam/Downloads/setup_cool.exe");
    let lib = &mut e.lib;
    assert_eq!(d.kind, EntryKind::Installer);
    let id = lib.create(&d).unwrap().id;
    assert_eq!(lib.snapshot().get(id).unwrap().kind, EntryKind::Installer);
    let prefix = paths().prefix_dir(id);
    let dc = prefix.join("drive_c");
    let start_menu =
        dc.join("users/steamuser/AppData/Roaming/Microsoft/Windows/Start Menu/Programs");
    let since = Timestamp::from_millis(BASE_MS);
    mem.set_now((BASE_MS - 1000) * 1_000_000)
        .file(dc.join("Program Files/Old/old.exe"), vec![0u8; 900_000])
        .set_now((BASE_MS + 1000) * 1_000_000)
        .file(
            dc.join("Games/Cool/cool.exe"),
            PeBuilder::new().string("ProductName", "Cool Game").build(),
        )
        .file(dc.join("Games/Cool/unins000.exe"), b"MZ")
        .file(dc.join("Games/Cool/big_helper.exe"), vec![1u8; 50_000])
        .file(dc.join("windows/system32/new.exe"), b"MZ")
        .file(dc.join("Program Files/Common Files/x.exe"), b"MZ")
        .file(
            start_menu.join("Cool Game.lnk"),
            build_lnk(&LnkSpec {
                local_base_path: Some(r"C:\Games\Cool\cool.exe".into()),
                ..LnkSpec::default()
            }),
        )
        .file(
            start_menu.join("Evil.lnk"),
            build_lnk(&LnkSpec {
                local_base_path: Some(r"Z:\etc\secret.exe".into()),
                relative_path: Some(r"..\..\..\..\..\..\..\..\..\..\..\etc\secret.exe".into()),
                ..LnkSpec::default()
            }),
        )
        .file(
            start_menu.join("Uninstall.lnk"),
            build_lnk(&LnkSpec {
                local_base_path: Some(r"C:\Games\Cool\unins000.exe".into()),
                ..LnkSpec::default()
            }),
        )
        .link(dc.join("escape"), "/")
        .link(dc.join("users/steamuser/Desktop"), "/etc");
    let wrap = Wrap::new(mem.clone());
    let found = installer::discover(&wrap, &*clock, &prefix, since);
    let names: Vec<_> = found
        .iter()
        .map(|p| p.path.strip_prefix(&dc).unwrap().to_path_buf())
        .collect();
    assert_eq!(
        names,
        [
            PathBuf::from("Games/Cool/cool.exe"),
            PathBuf::from("Games/Cool/big_helper.exe")
        ]
    );
    assert!(found[0].from_shortcut);
    for p in wrap.log.lock().iter() {
        assert!(p.starts_with(&prefix), "read outside the prefix: {p:?}");
    }
    let exe = lib.convert_installer(id, &found[0].path).unwrap();
    let g = lib.game(id).unwrap();
    assert_eq!(g.primary().unwrap().id, exe);
    assert_eq!(g.name, "Cool Game");
    assert!(
        g.executables
            .iter()
            .any(|x| x.role == ExeRole::Installer && x.canonical_path.ends_with("setup_cool.exe"))
    );
    assert_eq!(lib.snapshot().get(id).unwrap().kind, EntryKind::Game);
    assert!(
        lib.convert_installer(id, Path::new(HK)).is_err(),
        "only programs inside the prefix"
    );
}

// ---- LIB-011 export and import --------------------------------------------------------------

fn preset_policy(b: BasePreset) -> SandboxPolicy {
    match b {
        BasePreset::Offline => presets::offline(),
        BasePreset::Online => presets::online(),
        BasePreset::Standard => presets::standard(&zaun_core::host::HostFacts::empty(
            Timestamp::from_millis(BASE_MS),
        )),
    }
}

#[test]
fn export_then_import_reproduces_settings() {
    let mut a = env(hk_fs());
    let mut d = draft(&a, HK);
    d.tags = vec![tag("metroidvania"), tag("finished")];
    let id = a.lib.create(&d).unwrap().id;
    let g = a.lib.game(id).unwrap();
    let mut policy = presets::online();
    policy.game_dir = GameDirAccess::ReadWrite;
    policy.folders.push(FolderGrant {
        path: "/home/sam/Music".into(),
        canonical: "/home/sam/Music".into(),
        access: FolderAccess::ReadOnly,
        origin: GrantOrigin::WellKnown(UserDir::Music),
        drive_letter: Some('m'),
        granted_at: a.clock.now(),
    });
    policy.folders.push(FolderGrant {
        path: "/data/mods".into(),
        canonical: "/data/mods".into(),
        access: FolderAccess::ReadWrite,
        origin: GrantOrigin::UserChosen,
        drive_letter: None,
        granted_at: a.clock.now(),
    });
    let mut launch = g.launch.clone();
    launch.args = vec!["-screen-fullscreen".into(), "1".into()];
    launch.compat.mangohud = true;
    launch.env.push(zaun_core::model::EnvVar {
        name: "DXVK_HUD".into(),
        value: "fps".into(),
    });
    a.lib
        .update(
            id,
            GamePatch {
                policy: Some(policy),
                launch: Some(launch),
                umu_id: Some(Some("umu-367520".into())),
                store: Some(Some(zaun_core::model::Store::Gog)),
                runtime: Some(zaun_core::model::RuntimeRef::Channel(
                    zaun_core::model::Channel::GeProton,
                )),
                ..GamePatch::default()
            },
        )
        .unwrap();
    let text = a.lib.export_game(id).unwrap();
    assert!(text.contains("zaun_export = 1"));
    assert!(text.contains("layer = \"channel:ge-proton\""));

    let mut b = env(hk_fs());
    let bid = b.lib.create(&draft(&b, HK)).unwrap().id;
    let user_dirs = vec![(UserDir::Music, PathBuf::from("/home/sam/Music"))];
    let (settings, _) = b
        .lib
        .import_settings(bid, &text, &preset_policy, &user_dirs)
        .unwrap();
    assert_eq!(
        settings.notes,
        [ImportNote::Regrant {
            path: "/data/mods".into(),
            access: FolderAccess::ReadWrite,
            drive_letter: None
        }]
    );
    let (ga, gb) = (a.lib.game(id).unwrap(), b.lib.game(bid).unwrap());
    assert_eq!(gb.launch.args, ga.launch.args);
    assert_eq!(gb.launch.env, ga.launch.env);
    assert_eq!(gb.launch.compat, ga.launch.compat);
    assert_eq!(gb.runtime, ga.runtime);
    assert_eq!(gb.umu_id, ga.umu_id);
    assert_eq!(gb.store, ga.store);
    assert_eq!(gb.policy.network, NetworkAccess::Full);
    assert_eq!(
        gb.policy.folders.len(),
        1,
        "the absolute grant is listed, not applied"
    );
    assert_eq!(gb.policy.folders[0].drive_letter, Some('m'));
    assert_eq!(b.lib.snapshot().get(bid).unwrap().tags.len(), 2);
}

#[test]
fn hostile_imports_are_rejected() {
    let ctx_fp = zaun_core::model::Fingerprint {
        size: 1,
        mtime_ns: 0,
        quick_hash: [9; 32],
        full_hash: None,
    };
    let ctx = export::ImportContext {
        preset_policy: &preset_policy,
        user_dirs: &[],
        exe_fingerprint: Some(&ctx_fp),
        has_root: false,
        root: None,
        now: Timestamp::from_millis(BASE_MS),
    };
    let base = r#"zaun_export = 1
exported_at = "2026-11-12T10:15:00Z"
[game]
name = "X"
[program]
file_name = "x.exe"
quick_hash = "sha256:00"
working_dir = "exe-dir"
[compatibility]
layer = "default"
follow_updates = true
[protection]
preset = "offline"
network = "none"
game_dir = "read-write"
save_data = "external"
save_dir = "/home/sam/Saves"
[protection.devices]
audio = "full"
controllers = "detected"
"#;
    let ok = export::import(&export::parse(base).unwrap(), &ctx).unwrap();
    assert_eq!(
        ok.policy.game_dir,
        GameDirAccess::FileOnly,
        "loose file rule wins"
    );
    assert_eq!(ok.policy.save_data, SaveDataLocation::Internal);
    assert!(ok.notes.contains(&ImportNote::QuickHashMismatch));
    assert!(
        ok.notes
            .contains(&ImportNote::ExternalSaveDir("/home/sam/Saves".into()))
    );
    let bad = |t: String| {
        export::parse(&t)
            .and_then(|f| export::import(&f, &ctx))
            .is_err()
    };
    assert!(
        bad(base.replace("[game]", "evil = true\n[game]")),
        "unknown key"
    );
    assert!(bad(base.replace("zaun_export = 1", "zaun_export = 2")));
    assert!(bad(base.replace("name = \"X\"", "name = \"\"")));
    assert!(bad(
        base.replace("layer = \"default\"", "layer = \"pinned:../../x\"")
    ));
    assert!(bad(base.replace("network = \"none\"", "network = \"host\"")));
    assert!(bad(format!(
        "{base}[[protection.folders]]\nwell_known = \"music\"\naccess = \"read-write\"\ndrive_letter = \"c\"\n"
    )));
    assert!(bad(format!(
        "{base}[[environment]]\nname = \"LD_PRELOAD=\"\nvalue = \"x\"\n"
    )));
    assert!(bad(base.replace(
        "follow_updates = true",
        "follow_updates = true\n[compatibility.options]\nbogus = 1"
    )));
    assert!(bad(format!("{base}#{}", "x".repeat(300 * 1024))));
    let t = base.replace(
        "working_dir = \"exe-dir\"",
        "working_dir = \"custom\"\nworking_dir_relative = \"../../etc\"",
    );
    let f = export::parse(&t).unwrap();
    let rooted = export::ImportContext {
        has_root: true,
        root: Some(Path::new("/g")),
        ..ctx
    };
    assert!(export::import(&f, &rooted).is_err());
}

// ---- ART-003 icons and the steam_appid.txt reader ------------------------------------------

#[test]
fn pe_icon_source_and_app_id_file() {
    let mem = Arc::new(hk_fs());
    mem.file("/x/plain.exe", PeBuilder::new().build())
        .file(
            "/x/text.exe",
            b"just some text that is not an executable at all, really",
        )
        .file("/x/trunc.exe", &sample_game()[..200])
        .link("/x/link.exe", HK);
    let fs: Arc<dyn Fs> = mem.clone();
    let clock: Arc<dyn Clock> = Arc::new(FakeClock::at_millis(BASE_MS));
    let src = PeIconSource::new(Arc::new(FsPeInspector::new(fs.clone(), clock)));
    let ico = src.largest_icon(Path::new(HK)).unwrap().unwrap();
    assert_eq!(&ico[..6], &[0, 0, 1, 0, 1, 0]);
    assert_eq!(ico[6], 0, "256 px is written as 0");
    assert_eq!(src.largest_icon(Path::new("/x/plain.exe")).unwrap(), None);
    assert_eq!(src.largest_icon(Path::new("/x/text.exe")).unwrap(), None);
    assert!(
        src.largest_icon(Path::new("/x/trunc.exe"))
            .map_or(true, |i| i.is_none())
    );
    assert!(
        src.largest_icon(Path::new("/x/link.exe")).is_err(),
        "final symlinks are not followed"
    );

    let app = FsAppIdFile::new(fs);
    assert_eq!(
        app.read_app_id(Path::new("/home/sam/Games/Hollow Knight")),
        Some(367_520)
    );
    assert_eq!(app.read_app_id(Path::new("/x")), None);
}

#[test]
fn default_runtime_labels() {
    use zaun_core::ids::RuntimeName;
    use zaun_core::library::RuntimeInfo;
    use zaun_core::model::{Channel, RuntimeRef};
    let r = DefaultRuntimeInfo;
    let n = RuntimeName::new("GE-Proton10-25").unwrap();
    assert_eq!(
        &*r.label(&RuntimeRef::Default, Some(&n)),
        "Default (GE-Proton10-25)"
    );
    assert_eq!(
        &*r.label(&RuntimeRef::Channel(Channel::GeProton), None),
        "GE-Proton (newest)"
    );
    assert_eq!(
        &*r.label(&RuntimeRef::Pinned(n.clone()), None),
        "GE-Proton10-25"
    );
    let _ = lindex::RECENT_MAX;
}
