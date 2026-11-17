//! The migration runner (LOGIC_SPECS §15, ADR-0011, PERSISTENCE §5).
//!
//! Migrations are numbered SQL files embedded from `storage/migrations/`, each with an
//! optional Rust step. Every migration runs in its own `BEGIN IMMEDIATE` transaction that
//! ends by setting `user_version`. Before upgrading an existing library a pre-migration
//! backup is taken; when a step fails after an earlier step of the same run committed, the
//! file is restored from that backup. A newer database is refused without modification.
//! Checking for live supervisors (`EP-DB-011`) is the caller's job, before calling.

use std::path::PathBuf;

use super::db::{Conn, exec, scalar};
use super::integrity::{self, IntegrityReport};
use super::{Db, KvRepo, MetaKey, StorageError, StorageResult, Store, backup};
use crate::clock::Timestamp;
use crate::consts::{APP_VERSION, DB_SCHEMA_VERSION};

/// A data transformation run inside a migration's transaction, after its SQL.
pub type RustStep = fn(&mut MigrationTx<'_>) -> StorageResult<()>;

/// One migration.
#[derive(Clone, Copy)]
pub struct Migration {
    /// The `user_version` after this migration.
    pub version: u32,
    /// File stem, e.g. `0001_initial`.
    pub name: &'static str,
    /// The SQL text.
    pub sql: &'static str,
    /// Optional Rust step.
    pub rust: Option<RustStep>,
}

impl std::fmt::Debug for Migration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Migration")
            .field("version", &self.version)
            .field("name", &self.name)
            .field("rust", &self.rust.is_some())
            .finish()
    }
}

/// The embedded migrations, in order.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "0001_initial",
        sql: include_str!("migrations/0001_initial.sql"),
        rust: None,
    },
    Migration {
        version: 2,
        name: "0002_steam",
        sql: include_str!("migrations/0002_steam.sql"),
        rust: None,
    },
];

/// The connection as seen by a Rust step (inside the migration's transaction).
pub struct MigrationTx<'a> {
    conn: &'a mut Conn,
}

impl MigrationTx<'_> {
    /// Execute statements without parameters.
    pub fn execute(&mut self, sql: &str) -> StorageResult<()> {
        let sql = sql.to_owned();
        self.conn.run(async move |c| exec(c, &sql).await)
    }

    /// The first column of the first row as an integer.
    pub fn query_i64(&mut self, sql: &str) -> StorageResult<i64> {
        let sql = sql.to_owned();
        self.conn.run(async move |c| scalar(c, &sql).await)
    }
}

/// Inputs of a migration run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrateOptions {
    /// Where the pre-migration backup goes (`backups/`). `None` skips it (in-memory
    /// databases and tests only).
    pub backups_dir: Option<PathBuf>,
    /// Current time (backup names, `meta.created_at`).
    pub now: Timestamp,
}

impl Default for MigrateOptions {
    fn default() -> Self {
        Self {
            backups_dir: None,
            now: Timestamp::EPOCH,
        }
    }
}

/// What the caller learns (STORE-003).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MigrationOutcome {
    /// The schema was already current.
    Ready,
    /// The schema was created (`from = 0`) or upgraded.
    Migrated {
        /// Version before.
        from: u32,
        /// Version after.
        to: u32,
        /// The pre-migration backup, if one was taken.
        backup: Option<PathBuf>,
    },
    /// The file is newer than this build (`EP-DB-010`); nothing was changed.
    RefusedNewer {
        /// Its version.
        found: u32,
        /// `DB_SCHEMA_VERSION`.
        supported: u32,
    },
    /// A step failed (`EP-DB-013`); the database is at `left_at` (restored from the backup
    /// when `restored`). The caller starts in recovery mode.
    Failed {
        /// The version the failing step would have produced.
        step: u32,
        /// The cause.
        error: String,
        /// The version the database is left at.
        left_at: u32,
        /// Whether the file was restored from the pre-migration backup.
        restored: bool,
    },
}

impl Db {
    /// Bring the schema to [`DB_SCHEMA_VERSION`] (LOGIC_SPECS §15). Errors are returned only
    /// for failures before any change (unreadable file, backup failure `EP-DB-012`) and
    /// for a failed post-migration integrity check.
    pub fn migrate(&mut self, opts: &MigrateOptions) -> StorageResult<MigrationOutcome> {
        migrate_with(self, MIGRATIONS, opts)
    }
}

/// The runner with an explicit migration list (tests inject failing steps).
pub fn migrate_with(
    db: &mut Db,
    migrations: &[Migration],
    opts: &MigrateOptions,
) -> StorageResult<MigrationOutcome> {
    let target = migrations.last().map_or(0, |m| m.version);
    let from = db.schema_version()?;
    if from > target.max(DB_SCHEMA_VERSION) {
        return Ok(MigrationOutcome::RefusedNewer {
            found: from,
            supported: target.max(DB_SCHEMA_VERSION),
        });
    }
    if from == target {
        record_app_version(db)?;
        return Ok(MigrationOutcome::Ready);
    }

    // Pre-migration backup of an existing library (a fresh file has nothing to lose).
    let backup = match (&opts.backups_dir, from > 0, db.file_path().is_some()) {
        (Some(dir), true, true) => Some(
            backup::create(db, backup::BackupKind::PreMigration { from }, dir, opts.now)
                .map_err(|e| StorageError::Backup(e.to_string()))?,
        ),
        _ => None,
    };

    for m in migrations.iter().filter(|m| m.version > from) {
        if let Err(e) = run_one(db.conn_mut(), m) {
            let mut restored = false;
            if let Some(b) = &backup
                && m.version > from + 1
            {
                // Earlier steps of this run committed: put the file back.
                restored = restore_in_place(db, b).is_ok();
            }
            let left_at = db.schema_version().unwrap_or(from);
            return Ok(MigrationOutcome::Failed {
                step: m.version,
                error: e.to_string(),
                left_at,
                restored,
            });
        }
    }

    if from == 0 {
        db.meta_set(MetaKey::CreatedAt, &opts.now.as_millis().to_string())?;
        db.meta_set(MetaKey::InstallId, &install_id(opts.now))?;
    }
    record_app_version(db)?;
    match integrity::quick_check(db)? {
        IntegrityReport::Ok => {}
        IntegrityReport::Problems(p) => return Err(StorageError::Corrupt(p.join("; "))),
    }
    Ok(MigrationOutcome::Migrated {
        from,
        to: target,
        backup,
    })
}

fn run_one(conn: &mut Conn, m: &Migration) -> StorageResult<()> {
    let sql = m.sql;
    let version = m.version;
    conn.run(async |c| exec(c, "BEGIN IMMEDIATE").await)?;
    let result = (|| -> StorageResult<()> {
        conn.run(async move |c| exec(c, sql).await)?;
        if let Some(step) = m.rust {
            step(&mut MigrationTx { conn: &mut *conn })?;
        }
        conn.run(async move |c| exec(c, &format!("PRAGMA user_version = {version}")).await)?;
        conn.run(async |c| exec(c, "COMMIT").await)
    })();
    if result.is_err() {
        let _ = conn.run(async |c| exec(c, "ROLLBACK").await);
    }
    result.map_err(|e| StorageError::Migration {
        step: version,
        message: e.to_string(),
    })
}

fn restore_in_place(db: &mut Db, backup_file: &std::path::Path) -> StorageResult<()> {
    let Some(path) = db.file_path() else {
        return Err(StorageError::Backup("in-memory database".into()));
    };
    db.conn_mut().close()?;
    backup::copy_database_file(backup_file, &path)?;
    db.reopen()
}

fn record_app_version(db: &mut Db) -> StorageResult<()> {
    if db.meta_get(MetaKey::LastAppVersion)?.as_deref() != Some(APP_VERSION) {
        db.meta_set(MetaKey::LastAppVersion, APP_VERSION)?;
    }
    Ok(())
}

/// A random 128-bit identifier as 32 hex digits (PERSISTENCE §11). Read from the kernel;
/// falls back to the random part of two UUIDv7 values.
fn install_id(now: Timestamp) -> String {
    use crate::fsx::Fs;
    let fs = crate::fsx::RealFs::new();
    if let Ok(bytes) = fs.read_range(std::path::Path::new("/dev/urandom"), 0, 16)
        && bytes.len() == 16
    {
        return crate::model::to_hex(&bytes);
    }
    let secs = u64::try_from(now.as_millis() / 1000).unwrap_or(0);
    let t = uuid::Timestamp::from_unix(uuid::NoContext, secs, 0);
    let a = uuid::Uuid::new_v7(t);
    let b = uuid::Uuid::new_v7(t);
    let mut bytes = a.as_bytes()[8..].to_vec();
    bytes.extend_from_slice(&b.as_bytes()[8..]);
    crate::model::to_hex(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::Sealed;

    fn schema_dump(conn: &mut Conn) -> Vec<(String, String, String)> {
        conn.run(async |c| {
            ormlite::query_as::<_, (String, String, String)>(
                "SELECT type, name, coalesce(sql, '') FROM sqlite_schema \
                 WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
            )
            .fetch_all(&mut *c)
            .await
            .map_err(crate::storage::db::map_sqlx)
        })
        .unwrap()
    }

    /// The `sql` block of DATA_MODEL §6.
    fn documented_schema() -> String {
        let doc = include_str!("../../../../docs/DATA_MODEL.md");
        let start = doc.find("## 6. SQLite schema").unwrap();
        let rest = &doc[start..];
        let open = rest.find("```sql\n").unwrap() + "```sql\n".len();
        let close = rest[open..].find("```").unwrap();
        rest[open..open + close].to_owned()
    }

    fn normalise(sql: &str) -> String {
        sql.lines()
            .map(|l| l.split("--").next().unwrap_or("").trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn fresh_database_has_exactly_the_documented_schema() {
        let mut db = Db::open_in_memory().unwrap();
        assert_eq!(db.schema_version().unwrap(), DB_SCHEMA_VERSION);
        let ours = schema_dump(db.conn());

        // A raw connection that runs only the documented SQL.
        let mut raw = Conn::open_raw_memory().unwrap();
        let sql = documented_schema();
        raw.run(async move |c| exec(c, &sql).await).unwrap();
        let theirs = schema_dump(&mut raw);

        let norm = |v: Vec<(String, String, String)>| {
            v.into_iter()
                .map(|(t, n, s)| (t, n, normalise(&s)))
                .collect::<Vec<_>>()
        };
        assert_eq!(norm(ours), norm(theirs));
        let app: i64 = db
            .conn()
            .run(async |c| scalar(c, "PRAGMA application_id").await)
            .unwrap();
        assert_eq!(app, 0x4550_4C59);
        assert!(
            db.meta_get(MetaKey::InstallId)
                .unwrap()
                .is_some_and(|v| v.len() == 32)
        );
        assert_eq!(
            db.meta_get(MetaKey::LastAppVersion).unwrap().as_deref(),
            Some(APP_VERSION)
        );
    }

    #[test]
    fn current_database_is_ready_and_newer_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let mut db = Db::open_rw(&path).unwrap();
        let opts = MigrateOptions {
            backups_dir: Some(dir.path().join("backups")),
            now: Timestamp::from_millis(1_794_480_000_000),
        };
        assert!(matches!(
            db.migrate(&opts).unwrap(),
            MigrationOutcome::Migrated {
                from: 0,
                to: 2,
                backup: None
            }
        ));
        assert_eq!(db.migrate(&opts).unwrap(), MigrationOutcome::Ready);
        db.conn()
            .run(async |c| exec(c, "PRAGMA user_version = 7").await)
            .unwrap();
        assert_eq!(
            db.migrate(&opts).unwrap(),
            MigrationOutcome::RefusedNewer {
                found: 7,
                supported: DB_SCHEMA_VERSION
            }
        );
        assert_eq!(db.schema_version().unwrap(), 7);
        assert!(matches!(
            Db::open_ro(&path),
            Err(StorageError::NewerSchema { found: 7, .. })
        ));
    }

    fn add_column(tx: &mut MigrationTx<'_>) -> StorageResult<()> {
        tx.execute("UPDATE kv SET value = 'migrated' WHERE key = 'probe'")?;
        assert_eq!(
            tx.query_i64("SELECT count(*) FROM kv WHERE value = 'migrated'")?,
            1
        );
        Ok(())
    }

    fn failing(tx: &mut MigrationTx<'_>) -> StorageResult<()> {
        tx.execute("DELETE FROM kv")?;
        Err(StorageError::Sql("injected failure".into()))
    }

    #[test]
    fn rust_steps_run_and_failing_steps_roll_back_and_restore() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let backups = dir.path().join("backups");
        let opts = MigrateOptions {
            backups_dir: Some(backups.clone()),
            now: Timestamp::from_millis(1_794_480_000_000),
        };
        let mut db = Db::open_rw(&path).unwrap();
        db.migrate(&opts).unwrap();
        db.kv_set("probe", "v1", opts.now).unwrap();

        // Synthetic steps after the real ones.
        let v3 = Migration {
            version: 3,
            name: "0003_test",
            sql: "CREATE TABLE extra (x INTEGER) STRICT;",
            rust: Some(add_column),
        };
        let v4_bad = Migration {
            version: 4,
            name: "0004_bad",
            sql: "CREATE TABLE more (y INTEGER) STRICT;",
            rust: Some(failing),
        };
        let list = [MIGRATIONS[0], MIGRATIONS[1], v3, v4_bad];
        let out = migrate_with(&mut db, &list, &opts).unwrap();
        match out {
            MigrationOutcome::Failed {
                step,
                left_at,
                restored,
                ..
            } => {
                assert_eq!(step, 4);
                assert!(restored, "v3 committed, so the backup is restored");
                assert_eq!(left_at, 2);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(db.schema_version().unwrap(), 2);
        assert_eq!(db.kv_get("probe").unwrap().as_deref(), Some("v1"));
        let names = backup::list(&backups).unwrap();
        assert_eq!(names.len(), 1);
        assert!(names[0].file_name.starts_with("pre-migration-v2-"));

        // A single failing step only rolls back.
        let out = migrate_with(
            &mut db,
            &[MIGRATIONS[0], MIGRATIONS[1], v4_bad.with_version(3)],
            &opts,
        )
        .unwrap();
        assert!(matches!(
            out,
            MigrationOutcome::Failed {
                step: 3,
                left_at: 2,
                restored: false,
                ..
            }
        ));
        assert_eq!(db.kv_get("probe").unwrap().as_deref(), Some("v1"));

        // And a good chain applies.
        let out = migrate_with(&mut db, &[MIGRATIONS[0], MIGRATIONS[1], v3], &opts).unwrap();
        assert!(matches!(
            out,
            MigrationOutcome::Migrated {
                from: 2,
                to: 3,
                backup: Some(_)
            }
        ));
        assert_eq!(db.kv_get("probe").unwrap().as_deref(), Some("migrated"));
    }

    impl Migration {
        fn with_version(mut self, v: u32) -> Self {
            self.version = v;
            self
        }
    }
}
