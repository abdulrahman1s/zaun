//! M2 completion scenario (IMPLEMENTATION_ORDER M2) and schema fixtures (TESTING_STRATEGY §5).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use zaun_core::clock::Timestamp;
use zaun_core::fsx::{Fs, RealFs};
use zaun_core::storage::backup::{self, BackupKind};
use zaun_core::storage::integrity::{self, IntegrityReport};
use zaun_core::storage::{
    Db, GameRepo, LibraryFiles, MigrateOptions, MigrationOutcome, ReadDb, Store,
};
use zaun_testkit::TestEnv;
use zaun_testkit::fixtures::games::{self, BASE_MS};

const PAGE: usize = 4096;

fn now() -> Timestamp {
    Timestamp::from_millis(BASE_MS)
}

fn open_migrated(files: &LibraryFiles) -> Db {
    let mut db = Db::open_rw(&files.db_file).unwrap();
    let out = db
        .migrate(&MigrateOptions {
            backups_dir: Some(files.backups_dir.clone()),
            now: now(),
        })
        .unwrap();
    assert!(matches!(
        out,
        MigrationOutcome::Ready | MigrationOutcome::Migrated { .. }
    ));
    db
}

/// Overwrite page `page` (0-based) of a copy of `src` with garbage.
fn damaged_copy(src: &Path, dest: &Path, page: usize) {
    let fs = RealFs::new();
    let mut bytes = fs.read(src, 1 << 30).unwrap();
    assert!(bytes.len() > (page + 1) * PAGE, "database too small");
    for b in &mut bytes[page * PAGE..(page + 1) * PAGE] {
        *b = 0xA5;
    }
    fs.atomic_write(dest, &bytes, 0o600).unwrap();
}

#[test]
fn create_close_reopen_backup_corrupt_and_salvage() {
    let env = TestEnv::new().unwrap();
    let paths = env.paths();
    let files = LibraryFiles::from_paths(&paths);
    RealFs::new()
        .create_dir_all(&paths.data_dir, 0o700)
        .unwrap();

    // Create entirely through repositories.
    let mut db = open_migrated(&files);
    let populated = games::populate(&mut db, 60, 8, 42);
    assert!(populated.sessions > 50);
    let dump = games::semantic_dump(&mut db);
    db.checkpoint_truncate().unwrap();
    db.close().unwrap();

    // Reopen: same content, schema ready.
    let mut db = open_migrated(&files);
    assert_eq!(games::semantic_dump(&mut db), dump);
    assert!(integrity::quick_check(&mut db).unwrap().is_ok());

    // Back up.
    let daily = backup::run_daily(&mut db, &files.backups_dir, now())
        .unwrap()
        .unwrap();
    let info = backup::validate(&daily).unwrap();
    assert_eq!(info.games, 60);
    db.close().unwrap();
    let mut b = ReadDb::open(&daily).unwrap();
    assert_eq!(games::semantic_dump(&mut b), dump);
    b.close().unwrap();

    // Corrupt a copy of the backup (a compact file: every page is in use) in the middle.
    let pages = RealFs::new().metadata(&daily).unwrap().size as usize / PAGE;
    let broken = paths.data_dir.join("broken.db");
    damaged_copy(&daily, &broken, pages / 2);
    match integrity::check_file(&broken).unwrap() {
        IntegrityReport::Problems(p) => assert!(!p.is_empty()),
        IntegrityReport::Ok => panic!("damage not detected"),
    }
    assert!(backup::validate(&broken).is_err());

    // Salvage.
    let out = paths.data_dir.join("salvaged.db");
    let report = integrity::salvage(&broken, &out, now()).unwrap();
    let copied: u64 = report.tables.iter().map(|t| t.copied).sum();
    assert!(copied > 0, "{report:?}");
    let mut s = Db::open_rw(&out).unwrap();
    assert!(integrity::integrity_check(&mut s).unwrap().is_ok());
    let lib = s.load_library().unwrap();
    assert_eq!(lib.games.len() as u64, report.games_recovered);
    // Every recovered game is readable and satisfies the model's constraints.
    for g in lib.games {
        let game = g.into_game().unwrap();
        game.validate().unwrap();
    }
    s.close().unwrap();
    // The damaged file was not modified by salvage.
    let fs = RealFs::new();
    assert!(integrity::check_file(&broken).unwrap() != IntegrityReport::Ok);

    // Swap the repaired file in; the damaged library is kept.
    let kept = integrity::swap_in(&files, &out, now()).unwrap().unwrap();
    assert!(
        kept.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("corrupt-")
    );
    assert!(fs.exists(&kept));
    let mut db = open_migrated(&files);
    assert_eq!(db.game_count().unwrap(), report.games_recovered);

    // An undamaged copy salvages completely.
    let clean_out = paths.data_dir.join("clean-salvage.db");
    let full = integrity::salvage(&daily, &clean_out, now()).unwrap();
    assert!(full.complete && full.all_games_present, "{full:?}");
    let mut c = Db::open_rw(&clean_out).unwrap();
    assert_eq!(games::semantic_dump(&mut c), dump);
}

#[test]
fn damaged_header_is_detected_and_salvage_recovers_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let files = LibraryFiles {
        db_file: dir.path().join("library.db"),
        backups_dir: dir.path().join("backups"),
    };
    let mut db = open_migrated(&files);
    games::populate(&mut db, 5, 2, 1);
    let snap = dir.path().join("snap.db");
    backup::create_manual(&mut db, &snap).unwrap();
    let broken = dir.path().join("broken.db");
    damaged_copy(&snap, &broken, 0);
    assert!(!integrity::check_file(&broken).unwrap().is_ok());
    let out = dir.path().join("out.db");
    match integrity::salvage(&broken, &out, now()) {
        Ok(r) => {
            assert!(!r.all_games_present);
            assert_eq!(r.games_recovered, 0);
        }
        Err(e) => assert!(e.is_corruption(), "{e:?}"),
    }
}

#[test]
fn pre_migration_backup_kind_is_listed() {
    let dir = tempfile::tempdir().unwrap();
    let files = LibraryFiles {
        db_file: dir.path().join("library.db"),
        backups_dir: dir.path().join("backups"),
    };
    let mut db = open_migrated(&files);
    backup::create(
        &mut db,
        BackupKind::PreMigration { from: 1 },
        &files.backups_dir,
        now(),
    )
    .unwrap();
    let list = backup::list(&files.backups_dir).unwrap();
    assert_eq!(list[0].kind, Some(BackupKind::PreMigration { from: 1 }));
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../zaun-testkit/fixtures/db")
}

/// The v1 fixture database migrates (trivially, for now) to the current version, passes
/// `integrity_check` and has the recorded semantic dump. Regenerate with
/// `ZAUN_WRITE_FIXTURES=1 cargo test -p zaun-core --test it storage_m2::v1_fixture`
/// (only when a new schema version is released, from that release's code).
#[test]
fn v1_fixture_migrates_and_matches_its_dump() {
    let fs = RealFs::new();
    let fixture = fixture_dir().join("v1.sqlite");
    let dump_file = fixture_dir().join("v1.dump.txt");
    if std::env::var_os("ZAUN_WRITE_FIXTURES").is_some() {
        let dir = tempfile::tempdir().unwrap();
        let files = LibraryFiles {
            db_file: dir.path().join("library.db"),
            backups_dir: dir.path().join("backups"),
        };
        let mut db = open_migrated(&files);
        games::populate(&mut db, 12, 4, 2026);
        fs.create_dir_all(&fixture_dir(), 0o755).unwrap();
        if fs.exists(&fixture) {
            fs.remove_file(&fixture).unwrap();
        }
        backup::create_manual(&mut db, &fixture).unwrap();
        let dump = games::semantic_dump(&mut db);
        fs.atomic_write(&dump_file, dump.as_bytes(), 0o644).unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let files = LibraryFiles {
        db_file: dir.path().join("library.db"),
        backups_dir: dir.path().join("backups"),
    };
    let bytes = fs
        .read(&fixture, 1 << 26)
        .expect("fixture v1.sqlite is committed");
    fs.atomic_write(&files.db_file, &bytes, 0o600).unwrap();
    let mut db = open_migrated(&files);
    assert_eq!(db.schema_version().unwrap(), Db::SCHEMA_VERSION);
    assert!(integrity::integrity_check(&mut db).unwrap().is_ok());
    let expected = String::from_utf8(fs.read(&dump_file, 1 << 24).unwrap()).unwrap();
    // The dump was recorded at version 1; migrations after it (0002, ADR-0031) add tables and
    // change no data, so everything but the version line must be identical.
    let data = |s: &str| -> String {
        s.lines()
            .filter(|l| !l.starts_with("schema_version "))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(expected.starts_with("schema_version 1\n"));
    assert_eq!(data(&games::semantic_dump(&mut db)), data(&expected));
}
