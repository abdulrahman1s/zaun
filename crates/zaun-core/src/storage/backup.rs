//! Online backups with rotation, and restore (PERSISTENCE §7).
//!
//! A backup is `VACUUM INTO` a temporary file next to the destination (a consistent,
//! compacted snapshot of a live WAL database, ADR-0017), then `fsync`, rename, `fsync` of the
//! directory. Kinds and retention: daily (`daily-YYYYMMDD.db`, keep 7), pre-migration
//! (`pre-migration-v<n>-YYYYMMDDTHHMMSS.db`, keep 5), manual (any path, never rotated).
//! Timestamps in names are UTC.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::db::{exec, scalar};
use super::integrity::IntegrityReport;
use super::{Db, KvRepo, LibraryFiles, MetaKey, ReadDb, StorageError, StorageResult, Store};
use crate::clock::Timestamp;
use crate::consts::DB_SCHEMA_VERSION;
use crate::fsx::{Fs, FsError, FsErrorKind, FsOp, RealFs};

/// Daily backups kept.
pub const KEEP_DAILY: usize = 7;
/// Pre-migration backups kept.
pub const KEEP_PRE_MIGRATION: usize = 5;
/// Minimum age of the last daily backup before a new one is due.
pub const DAILY_INTERVAL: Duration = Duration::from_secs(24 * 3600);
/// Largest database file restore will copy (sanity bound).
const MAX_DB_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Which backup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BackupKind {
    /// Rolling daily backup.
    Daily,
    /// Taken before migrating from schema version `from`.
    PreMigration {
        /// The version being migrated from.
        from: u32,
    },
}

/// A file in the backups directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupEntry {
    /// Full path.
    pub path: PathBuf,
    /// File name.
    pub file_name: String,
    /// Its kind, when the name follows the scheme (`replaced-*`/`corrupt-*` are `None`).
    pub kind: Option<BackupKind>,
}

/// What [`validate`] found in a candidate backup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupInfo {
    /// `user_version`.
    pub schema_version: u32,
    /// Games that are not soft-deleted.
    pub games: u64,
}

fn utc(now: Timestamp, fmt: &str) -> String {
    match jiff::Timestamp::from_millisecond(now.as_millis()) {
        Ok(t) => t
            .to_zoned(jiff::tz::TimeZone::UTC)
            .strftime(fmt)
            .to_string(),
        Err(_) => "00000000".to_owned(),
    }
}

/// The file name a backup of `kind` taken at `now` gets.
pub fn file_name(kind: BackupKind, now: Timestamp) -> String {
    match kind {
        BackupKind::Daily => format!("daily-{}.db", utc(now, "%Y%m%d")),
        BackupKind::PreMigration { from } => {
            format!("pre-migration-v{from}-{}.db", utc(now, "%Y%m%dT%H%M%S"))
        }
    }
}

/// A timestamp suffix for `replaced-*` and `corrupt-*` files.
pub(crate) fn stamp(now: Timestamp) -> String {
    utc(now, "%Y%m%dT%H%M%S")
}

fn parse_kind(name: &str) -> Option<BackupKind> {
    let stem = name.strip_suffix(".db")?;
    if let Some(date) = stem.strip_prefix("daily-") {
        return (date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit()))
            .then_some(BackupKind::Daily);
    }
    let rest = stem.strip_prefix("pre-migration-v")?;
    let (v, _ts) = rest.split_once('-')?;
    Some(BackupKind::PreMigration {
        from: v.parse().ok()?,
    })
}

/// The sortable time part of a backup name.
fn time_key(name: &str) -> &str {
    name.trim_end_matches(".db")
        .rsplit('-')
        .next()
        .unwrap_or(name)
}

/// Snapshot the database behind `store` into `dest` (replacing it): `VACUUM INTO` a temp
/// file, `fsync`, rename, `fsync` the directory. Must not be called inside a transaction.
pub fn snapshot_to(store: &mut (impl Store + ?Sized), dest: &Path) -> StorageResult<()> {
    let dir = dest
        .parent()
        .ok_or_else(|| StorageError::Backup(format!("no parent directory: {}", dest.display())))?;
    let fs = RealFs::new();
    fs.create_dir_all(dir, 0o700)?;
    let name = dest
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| StorageError::Backup("backup name is not UTF-8".into()))?;
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    if fs.exists(&tmp) {
        fs.remove_file(&tmp)?;
    }
    let target = tmp
        .to_str()
        .ok_or_else(|| StorageError::Backup("backup path is not UTF-8".into()))?
        .to_owned();
    let conn = super::repo::conn(store);
    // `query_only` forbids VACUUM INTO even though only the new file is written; a read-only
    // handle lifts it for this one statement (the connection itself stays SQLITE_OPEN_READONLY).
    let read_only = conn.is_read_only();
    let r = conn.run(async move |c| {
        if read_only {
            exec(c, "PRAGMA query_only = OFF").await?;
        }
        let r = ormlite::query("VACUUM INTO ?")
            .bind(target)
            .execute(&mut *c)
            .await
            .map(|_| ())
            .map_err(StorageError::from);
        if read_only {
            exec(c, "PRAGMA query_only = ON").await?;
        }
        r
    });
    if let Err(e) = r {
        let _ = fs.remove_file(&tmp);
        return Err(match e {
            StorageError::DiskFull => StorageError::DiskFull,
            other => StorageError::Backup(other.to_string()),
        });
    }
    let finish = || -> StorageResult<()> {
        fsync_path(&tmp, false)?;
        fs.rename(&tmp, dest)?;
        fsync_path(dir, true)?;
        Ok(())
    };
    finish().inspect_err(|_| {
        let _ = fs.remove_file(&tmp);
    })
}

fn fsync_path(path: &Path, dir: bool) -> StorageResult<()> {
    use rustix::fs::{Mode, OFlags};
    let flags = if dir {
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC
    } else {
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW
    };
    let io = |e: rustix::io::Errno| {
        StorageError::Fs(FsError::from_io(FsOp::Write, path, std::io::Error::from(e)))
    };
    let fd = rustix::fs::open(path, flags, Mode::empty()).map_err(io)?;
    rustix::fs::fsync(&fd).map_err(io)
}

/// Take a backup of `kind` into `backups_dir` and rotate that kind. Returns the file.
pub fn create(
    store: &mut (impl Store + ?Sized),
    kind: BackupKind,
    backups_dir: &Path,
    now: Timestamp,
) -> StorageResult<PathBuf> {
    let dest = backups_dir.join(file_name(kind, now));
    snapshot_to(store, &dest)?;
    rotate(backups_dir)?;
    Ok(dest)
}

/// A manual backup to a chosen location (never rotated).
pub fn create_manual(store: &mut (impl Store + ?Sized), dest: &Path) -> StorageResult<()> {
    snapshot_to(store, dest)
}

/// Whether a daily backup is due: `meta.last_backup_at` missing or older than 24 h. The
/// caller also requires a passing `quick_check` (PERSISTENCE §7).
pub fn daily_due(store: &mut (impl Store + ?Sized), now: Timestamp) -> StorageResult<bool> {
    let last = store
        .meta_get(MetaKey::LastBackupAt)?
        .and_then(|v| v.parse::<i64>().ok())
        .map(Timestamp::from_millis);
    Ok(match last {
        None => true,
        Some(t) => now.since(t).is_none_or(|age| age >= DAILY_INTERVAL) || t > now,
    })
}

/// Take the daily backup if due and record `meta.last_backup_at`.
pub fn run_daily(
    db: &mut Db,
    backups_dir: &Path,
    now: Timestamp,
) -> StorageResult<Option<PathBuf>> {
    if !daily_due(db, now)? {
        return Ok(None);
    }
    let p = create(db, BackupKind::Daily, backups_dir, now)?;
    db.meta_set(MetaKey::LastBackupAt, &now.as_millis().to_string())?;
    Ok(Some(p))
}

/// Backups in `dir`, newest first within each kind.
pub fn list(dir: &Path) -> StorageResult<Vec<BackupEntry>> {
    let fs = RealFs::new();
    let entries = match fs.read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind == FsErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut out: Vec<BackupEntry> = entries
        .into_iter()
        .filter(|e| e.file_type == crate::fsx::FileType::File)
        .filter_map(|e| {
            let name = e.name.to_str()?.to_owned();
            (name.ends_with(".db") && !name.starts_with('.')).then(|| BackupEntry {
                kind: parse_kind(&name),
                file_name: name,
                path: e.path,
            })
        })
        .collect();
    out.sort_by(|a, b| time_key(&b.file_name).cmp(time_key(&a.file_name)));
    Ok(out)
}

/// Apply the retention of PERSISTENCE §7 to `dir`.
pub fn rotate(dir: &Path) -> StorageResult<()> {
    let fs = RealFs::new();
    let all = list(dir)?;
    let daily = all.iter().filter(|e| e.kind == Some(BackupKind::Daily));
    let pre = all
        .iter()
        .filter(|e| matches!(e.kind, Some(BackupKind::PreMigration { .. })));
    for e in daily.skip(KEEP_DAILY).chain(pre.skip(KEEP_PRE_MIGRATION)) {
        fs.remove_file(&e.path)?;
    }
    Ok(())
}

/// Check that `path` is a usable Zaun backup: an Zaun file (`application_id`),
/// passing `integrity_check`, with a schema version not newer than this build.
pub fn validate(path: &Path) -> StorageResult<BackupInfo> {
    let mut db = ReadDb::open(path)?;
    let v = db.schema_version()?;
    if v > DB_SCHEMA_VERSION {
        return Err(StorageError::NewerSchema {
            found: v,
            supported: DB_SCHEMA_VERSION,
        });
    }
    if v == 0 {
        return Err(StorageError::Backup("the file contains no library".into()));
    }
    match super::integrity::integrity_check(&mut db)? {
        IntegrityReport::Ok => {}
        IntegrityReport::Problems(p) => return Err(StorageError::Corrupt(p.join("; "))),
    }
    let games: i64 = super::repo::conn(&mut db)
        .run(async |c| scalar(c, "SELECT count(*) FROM games WHERE deleted_at IS NULL").await)?;
    db.close()?;
    Ok(BackupInfo {
        schema_version: v,
        games: u64::try_from(games).unwrap_or(0),
    })
}

/// Copy a database file over `dest` (atomic replace), removing `dest`'s `-wal`/`-shm`.
pub(crate) fn copy_database_file(src: &Path, dest: &Path) -> StorageResult<()> {
    let fs = RealFs::new();
    let bytes = fs.read(src, MAX_DB_BYTES)?;
    for suffix in ["-wal", "-shm"] {
        let side = side_file(dest, suffix);
        if fs.exists(&side) {
            fs.remove_file(&side)?;
        }
    }
    fs.atomic_write(dest, &bytes, 0o600)?;
    Ok(())
}

pub(crate) fn side_file(db: &Path, suffix: &str) -> PathBuf {
    let mut s = db.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Move the current database (and its WAL) aside as `backups/<prefix>-<stamp>.db`.
pub(crate) fn move_aside(
    files: &LibraryFiles,
    prefix: &str,
    now: Timestamp,
) -> StorageResult<Option<PathBuf>> {
    let fs = RealFs::new();
    if !fs.exists(&files.db_file) {
        return Ok(None);
    }
    fs.create_dir_all(&files.backups_dir, 0o700)?;
    let dest = files
        .backups_dir
        .join(format!("{prefix}-{}.db", stamp(now)));
    fs.rename(&files.db_file, &dest)?;
    let wal = side_file(&files.db_file, "-wal");
    if fs.exists(&wal) {
        fs.rename(&wal, &side_file(&dest, "-wal"))?;
    }
    let shm = side_file(&files.db_file, "-shm");
    if fs.exists(&shm) {
        fs.remove_file(&shm)?;
    }
    Ok(Some(dest))
}

/// Restore `source` as the library (Settings → Advanced → "Restore library…"). The caller
/// guarantees that no session runs and that every handle on the library is closed. The
/// file is validated first; the current database is kept as `backups/replaced-<ts>.db`.
/// Returns that path. Opening and migrating the restored file is the caller's next step.
pub fn restore(
    files: &LibraryFiles,
    source: &Path,
    now: Timestamp,
) -> StorageResult<Option<PathBuf>> {
    validate(source)?;
    let replaced = move_aside(files, "replaced", now)?;
    if let Err(e) = copy_database_file(source, &files.db_file) {
        // Put the previous library back.
        if let Some(prev) = &replaced {
            let fs = RealFs::new();
            let _ = fs.rename(prev, &files.db_file);
        }
        return Err(e);
    }
    Ok(replaced)
}

/// Remove leftovers of interrupted snapshots in `dir` (start-up sweep helper).
pub fn sweep_temporaries(dir: &Path) -> StorageResult<usize> {
    let fs = RealFs::new();
    let entries = match fs.read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind == FsErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e.into()),
    };
    let mut n = 0;
    for e in entries {
        if e.name
            .to_str()
            .is_some_and(|s| s.starts_with('.') && s.contains(".db.tmp-"))
        {
            fs.remove_file(&e.path)?;
            n += 1;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::migrate::MigrateOptions;

    const T0: i64 = 1_794_480_000_000; // 2026-11-12T10:40:00Z

    fn ts(ms: i64) -> Timestamp {
        Timestamp::from_millis(ms)
    }

    fn library(dir: &Path) -> (Db, LibraryFiles) {
        let files = LibraryFiles {
            db_file: dir.join("library.db"),
            backups_dir: dir.join("backups"),
        };
        let mut db = Db::open_rw(&files.db_file).unwrap();
        db.migrate(&MigrateOptions::default()).unwrap();
        (db, files)
    }

    #[test]
    fn names_follow_the_scheme() {
        assert_eq!(file_name(BackupKind::Daily, ts(T0)), "daily-20261112.db");
        assert_eq!(
            file_name(BackupKind::PreMigration { from: 1 }, ts(T0 + 15_000)),
            "pre-migration-v1-20261112T104015.db"
        );
        assert_eq!(parse_kind("daily-20261112.db"), Some(BackupKind::Daily));
        assert_eq!(
            parse_kind("pre-migration-v12-20261112T101500.db"),
            Some(BackupKind::PreMigration { from: 12 })
        );
        assert_eq!(parse_kind("replaced-20261112T101500.db"), None);
    }

    #[test]
    fn backup_while_another_connection_writes_is_consistent() {
        let dir = tempfile::tempdir().unwrap();
        let (mut db, files) = library(dir.path());
        let path = files.db_file.clone();
        let writer = std::thread::spawn(move || {
            let mut w = Db::open_rw(&path).unwrap();
            for i in 0..300 {
                w.kv_set(&format!("k{i}"), "v", ts(T0)).unwrap();
            }
        });
        let mut ro = ReadDb::open(&files.db_file).unwrap();
        let mut made = Vec::new();
        for i in 0..5 {
            let p = files.backups_dir.join(format!("manual-{i}.db"));
            create_manual(&mut ro, &p).unwrap();
            made.push(p);
        }
        writer.join().unwrap();
        for p in made {
            let info = validate(&p).unwrap();
            assert_eq!(info.schema_version, DB_SCHEMA_VERSION);
        }
        // The read-write handle can back up too.
        let p = create(&mut db, BackupKind::Daily, &files.backups_dir, ts(T0)).unwrap();
        validate(&p).unwrap();
    }

    #[test]
    fn rotation_keeps_seven_daily_and_five_pre_migration() {
        let dir = tempfile::tempdir().unwrap();
        let (mut db, files) = library(dir.path());
        for d in 0..10 {
            create(
                &mut db,
                BackupKind::Daily,
                &files.backups_dir,
                ts(T0 + d * 86_400_000),
            )
            .unwrap();
        }
        for h in 0..8 {
            create(
                &mut db,
                BackupKind::PreMigration { from: 1 },
                &files.backups_dir,
                ts(T0 + h * 3_600_000),
            )
            .unwrap();
        }
        let all = list(&files.backups_dir).unwrap();
        let daily: Vec<_> = all
            .iter()
            .filter(|e| e.kind == Some(BackupKind::Daily))
            .collect();
        assert_eq!(daily.len(), KEEP_DAILY);
        assert_eq!(daily[0].file_name, "daily-20261121.db");
        assert_eq!(
            all.iter()
                .filter(|e| matches!(e.kind, Some(BackupKind::PreMigration { .. })))
                .count(),
            KEEP_PRE_MIGRATION
        );
    }

    #[test]
    fn daily_backup_is_due_once_a_day() {
        let dir = tempfile::tempdir().unwrap();
        let (mut db, files) = library(dir.path());
        assert!(
            run_daily(&mut db, &files.backups_dir, ts(T0))
                .unwrap()
                .is_some()
        );
        assert!(
            run_daily(&mut db, &files.backups_dir, ts(T0 + 3_600_000))
                .unwrap()
                .is_none()
        );
        assert!(
            run_daily(&mut db, &files.backups_dir, ts(T0 + 86_400_000))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn restore_validates_and_keeps_the_replaced_file() {
        let dir = tempfile::tempdir().unwrap();
        let (mut db, files) = library(dir.path());
        db.kv_set("which", "old", ts(T0)).unwrap();
        let good = dir.path().join("good.db");
        create_manual(&mut db, &good).unwrap();
        db.kv_set("which", "new", ts(T0)).unwrap();
        db.close().unwrap();

        // Foreign and garbage files are rejected.
        let junk = dir.path().join("junk.db");
        crate::fsx::atomic_write_real(&junk, b"definitely not sqlite", 0o600).unwrap();
        assert!(restore(&files, &junk, ts(T0)).is_err());
        let empty = dir.path().join("empty.db");
        Db::open_rw(&empty).unwrap().close().unwrap();
        assert!(restore(&files, &empty, ts(T0)).is_err());

        let replaced = restore(&files, &good, ts(T0)).unwrap().unwrap();
        assert!(
            replaced
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("replaced-")
        );
        let mut db = Db::open_rw(&files.db_file).unwrap();
        assert_eq!(db.kv_get("which").unwrap().as_deref(), Some("old"));
        let mut prev = Db::open_rw(&replaced).unwrap();
        assert_eq!(prev.kv_get("which").unwrap().as_deref(), Some("new"));
    }
}
