//! Failure-injection matrix (TEST-002, TESTING_STRATEGY §10.2).
//!
//! Every `Fs` mutation call site of a flow is found by a counting pass and then failed, one
//! at a time, with `ENOSPC`, `EACCES`, `EIO` and `EROFS` (`testkit::inject`). After each
//! faulted run the test asserts that
//!
//! * a failure surfaces as a catalogued `UserError` (title and message filled, a file-system
//!   code rather than an internal one);
//! * the state is consistent (no half-created game, a removal either done or resumable, a
//!   preferences file holding the old or the new content);
//! * the same flow succeeds once the fault is removed.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;

use easyplay_core::artwork::PeIconSource;
use easyplay_core::clock::Clock;
use easyplay_core::config::{self, Preferences};
use easyplay_core::error::catalog::{DescribeCtx, describe};
use easyplay_core::error::{Error, ErrorCode};
use easyplay_core::fsx::{Fs, FsErrorKind, MountTable};
use easyplay_core::identity::SharedLocations;
use easyplay_core::identity::pe::{FsPeInspector, PeLimits};
use easyplay_core::ids::GameId;
use easyplay_core::library::draft::{DraftEnv, NoUmuTitles, build_draft};
use easyplay_core::library::index::DefaultRuntimeInfo;
use easyplay_core::library::remove::{NoHooks, RemoveOptions};
use easyplay_core::library::service::NoPolicyCheck;
use easyplay_core::library::{GameDraft, Library, LibraryDeps};
use easyplay_core::paths::{AppPaths, PathEnv};
use easyplay_core::sandbox::presets;
use easyplay_core::storage::{Db, GameRepo};
use easyplay_testkit::fixtures::games::BASE_MS;
use easyplay_testkit::fixtures::pe::sample_game;
use easyplay_testkit::inject::{Calls, FaultFs, fault_matrix};
use easyplay_testkit::{FakeClock, MemFs};

const HOME: &str = "/home/sam";
const EXE: &str = "/home/sam/Games/Hollow Knight/hollow_knight.exe";

/// `ENOSPC`, `EACCES`, `EIO`, `EROFS`.
const KINDS: &[FsErrorKind] = &[
    FsErrorKind::NoSpace,
    FsErrorKind::PermissionDenied,
    FsErrorKind::Other,
    FsErrorKind::ReadOnly,
];

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
        user_dirs: vec![],
        temp_dirs: vec!["/tmp".into()],
        mount_points: vec!["/".into()],
        media_parents: vec!["/mnt".into()],
        easyplay_dirs: vec!["/home/sam/.local/share/easyplay".into()],
        game_roots: vec![],
        deny: None,
    }
}

/// The library over a fault-injecting filesystem.
struct World {
    fs: Arc<FaultFs>,
    mem: Arc<MemFs>,
    lib: RefCell<Library>,
    draft: GameDraft,
    game: Option<GameId>,
}

impl AsRef<FaultFs> for World {
    fn as_ref(&self) -> &FaultFs {
        &self.fs
    }
}

fn world(with_game: bool) -> World {
    let mem = Arc::new(MemFs::new());
    mem.file(EXE, sample_game())
        .file("/home/sam/Games/Hollow Knight/UnityPlayer.dll", b"MZ");
    let fs = Arc::new(FaultFs::new(mem.clone()));
    let dynfs: Arc<dyn Fs> = fs.clone();
    let clock = Arc::new(FakeClock::at_millis(BASE_MS));
    let dynclock: Arc<dyn Clock> = clock.clone();
    let pe = FsPeInspector::new(dynfs.clone(), dynclock.clone());
    let mounts = MountTable::parse(mem.mountinfo().as_bytes());
    let deps = LibraryDeps {
        fs: dynfs.clone(),
        clock: dynclock.clone(),
        paths: paths(),
        runtime: Arc::new(DefaultRuntimeInfo),
        check: Arc::new(NoPolicyCheck),
        pe: Arc::new(pe.clone()),
        codec: None,
        mounts: mounts.clone(),
        shared: shared(),
        overlay_supported: true,
    };
    let mut lib = Library::open(Db::open_in_memory().unwrap(), deps).unwrap();
    let icons = PeIconSource::new(Arc::new(pe.clone()));
    let sh = shared();
    let draft = build_draft(
        &DraftEnv {
            fs: &*dynfs,
            clock: &*dynclock,
            pe: &pe,
            limits: PeLimits::default(),
            icons: &icons,
            shared: &sh,
            mounts: &mounts,
            index: lib.exe_index(),
            base_policy: presets::offline(),
            umu: &NoUmuTitles,
            check: &NoPolicyCheck,
        },
        Path::new(EXE),
    )
    .unwrap();
    let game = with_game.then(|| {
        let id = lib.create(&draft).unwrap().id;
        // Something to keep and something to delete.
        let prefix = paths().prefix_dir(id);
        mem.file(
            prefix.join("drive_c/users/steamuser/Saved Games/slot1.sav"),
            b"save",
        )
        .file(prefix.join("system.reg"), b"WINE REGISTRY");
        id
    });
    World {
        fs,
        mem,
        lib: RefCell::new(lib),
        draft,
        game,
    }
}

/// A failure the user sees: a catalogued description and a file-system code.
fn assert_catalogued(e: &Error) {
    let u = describe(e, &DescribeCtx::plain());
    assert!(
        !u.title.trim().is_empty() && !u.message.trim().is_empty(),
        "{e}"
    );
    let code = e.code.as_str();
    assert!(
        code.starts_with("EP-FS-") || code.starts_with("EP-LIB-") || code.starts_with("EP-DB-"),
        "unexpected code {code} for {e}"
    );
    assert!(
        !matches!(e.code, ErrorCode::IntTaskFailed | ErrorCode::IntFatal),
        "{e}"
    );
}

#[test]
fn adding_a_game_under_every_fault() {
    let report = fault_matrix(
        Calls::Mutations,
        KINDS,
        || world(false),
        |w| w.lib.borrow_mut().create(&w.draft).map(|r| r.id),
        |w, r| {
            let mut lib = w.lib.borrow_mut();
            match r {
                Ok(id) => assert!(lib.snapshot().get(*id).is_some()),
                Err(e) => {
                    assert_catalogued(e);
                    assert_eq!(lib.db_mut().game_count().unwrap(), 0, "half-created game");
                    assert!(lib.snapshot().games.is_empty());
                    // Retry after the fault is gone.
                    w.fs.disarm();
                    let id = lib.create(&w.draft).expect("retry after the fault").id;
                    assert!(lib.snapshot().get(id).is_some());
                }
            }
        },
    );
    println!("create: {} call sites, {} runs", report.sites, report.runs);
    assert!(report.sites >= 3, "{:?}", report.log);
}

#[test]
fn removing_a_game_under_every_fault() {
    let report = fault_matrix(
        Calls::Mutations,
        KINDS,
        || world(true),
        |w| {
            w.lib
                .borrow_mut()
                .remove(w.game.unwrap(), RemoveOptions::default(), &NoHooks)
        },
        |w, r| {
            let id = w.game.unwrap();
            let p = paths();
            let mut lib = w.lib.borrow_mut();
            let complete = matches!(r, Ok(rep) if rep.complete);
            if let Err(e) = r {
                assert_catalogued(e);
            }
            if !complete {
                // Either nothing happened (row live, files in place) or the row is
                // soft-deleted and the start-up retry finishes the job.
                w.fs.disarm();
                let row = lib.db_mut().game_record(id).unwrap();
                match row {
                    Some(rec) if rec.deleted_at.is_none() => {
                        assert!(lib.snapshot().get(id).is_some());
                        lib.remove(id, RemoveOptions::default(), &NoHooks)
                            .expect("retry after the fault");
                    }
                    Some(_) => {
                        lib.retry_removals(true, &NoHooks).expect("start-up retry");
                    }
                    None => {}
                }
            }
            assert!(
                lib.db_mut().game_record(id).unwrap().is_none(),
                "removal not finished after the retry"
            );
            assert!(!w.mem.exists(&p.game_dir(id)));
            // The save survived every interleaving.
            let saved = w
                .mem
                .paths()
                .into_iter()
                .filter(|x| x.ends_with("slot1.sav"))
                .count();
            assert_eq!(saved, 1, "the save file was lost or duplicated");
        },
    );
    println!("remove: {} call sites, {} runs", report.sites, report.runs);
    assert!(report.sites >= 3, "{:?}", report.log);
}

#[test]
fn saving_preferences_under_every_fault() {
    struct Prefs {
        fs: Arc<FaultFs>,
        mem: Arc<MemFs>,
    }
    impl AsRef<FaultFs> for Prefs {
        fn as_ref(&self) -> &FaultFs {
            &self.fs
        }
    }
    let file = paths().config_file;
    let old = Preferences::default();
    let mut new = Preferences::default();
    new.appearance.ui_scale = 1.5;
    let old_text = {
        let mem = MemFs::new();
        mem.dir(file.parent().unwrap());
        config::save(&mem, &file, &old).unwrap();
        mem.contents(&file).unwrap()
    };
    let report = fault_matrix(
        Calls::All,
        KINDS,
        || {
            let mem = Arc::new(MemFs::new());
            mem.dir(file.parent().unwrap());
            config::save(&*mem, &file, &old).unwrap();
            Prefs {
                fs: Arc::new(FaultFs::new(mem.clone())),
                mem,
            }
        },
        |w| config::save(&*w.fs, &file, &new).map_err(Error::from),
        |w, r| {
            let text = w.mem.contents(&file).unwrap();
            match r {
                Ok(()) => assert_ne!(text, old_text),
                Err(e) => {
                    assert_catalogued(e);
                    assert_eq!(text, old_text, "a failed save left a mix");
                    w.fs.disarm();
                    config::save(&*w.fs, &file, &new).unwrap();
                }
            }
        },
    );
    assert!(report.sites >= 1);
}
