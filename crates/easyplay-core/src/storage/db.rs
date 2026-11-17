//! Connections: open, pragmas (PERSISTENCE §3), read-only and read-write handles, the
//! `BEGIN IMMEDIATE` transaction helper, and the synchronous bridge onto `sqlx` (ADR-0017).

use std::path::{Path, PathBuf};
use std::time::Duration;

use ormlite::sqlite::{SqliteConnectOptions, SqliteConnection};
use ormlite::{ConnectOptions, Connection, Executor};

use super::{StorageError, StorageResult, migrate};
use crate::consts::{DB_APPLICATION_ID, DB_SCHEMA_VERSION};

/// The driver connection type (private to `storage`).
pub(crate) type SqlConn = SqliteConnection;

/// Busy timeout applied to every connection.
const BUSY_TIMEOUT: Duration = Duration::from_millis(5_000);
/// `journal_size_limit` in bytes.
const JOURNAL_SIZE_LIMIT: u64 = 16 * 1024 * 1024;
/// sqlx's per-connection prepared-statement cache (the `prepare_cached` of PERSISTENCE §3).
const STATEMENT_CACHE: usize = 128;

/// How a handle is opened.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenConfig {
    /// Use `journal_mode = DELETE` instead of WAL. `None` detects network filesystems
    /// (`nfs`, `cifs`, `smb3`, `9p`, `fuse.sshfs`) through `fsx::mounts`.
    pub rollback_journal: Option<bool>,
}

/// The journal mode in effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JournalMode {
    /// Write-ahead log (the normal case).
    Wal,
    /// Rollback journal, used on network filesystems (a Doctor warning).
    Delete,
    /// In-memory database.
    Memory,
    /// Anything else SQLite reported.
    Other,
}

impl JournalMode {
    fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "wal" => JournalMode::Wal,
            "delete" => JournalMode::Delete,
            "memory" => JournalMode::Memory,
            _ => JournalMode::Other,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Location {
    File(PathBuf),
    Memory,
}

/// One SQLite connection plus the private current-thread runtime that drives it.
///
/// Unnameable outside `storage` (it lives in a private module), so it never appears in a
/// public signature.
pub struct Conn {
    rt: tokio::runtime::Runtime,
    conn: Option<SqlConn>,
    depth: u32,
    read_only: bool,
    location: Location,
    journal: JournalMode,
}

impl Conn {
    fn runtime() -> StorageResult<tokio::runtime::Runtime> {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .map_err(|e| StorageError::Sql(format!("cannot create the storage runtime: {e}")))
    }

    fn open(location: Location, read_only: bool, cfg: &OpenConfig) -> StorageResult<Conn> {
        crate::ui_thread::assert_not_ui_thread("Store::open");
        let rt = Self::runtime()?;
        let options = match &location {
            Location::File(path) => SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(!read_only)
                .read_only(read_only),
            Location::Memory => SqliteConnectOptions::new().in_memory(true),
        }
        .foreign_keys(true)
        .busy_timeout(BUSY_TIMEOUT)
        .statement_cache_capacity(STATEMENT_CACHE);
        let conn = block_on(&rt, options.connect())?;
        let mut c = Conn {
            rt,
            conn: Some(conn),
            depth: 0,
            read_only,
            location,
            journal: JournalMode::Other,
        };
        c.check_identity()?;
        c.apply_pragmas(cfg)?;
        Ok(c)
    }

    /// Refuse SQLite files that belong to another application (STORE-002).
    fn check_identity(&mut self) -> StorageResult<()> {
        let (app_id, user_version, objects) = self.run(async |c| {
            let app: i64 = scalar(c, "PRAGMA application_id").await?;
            let ver: i64 = scalar(c, "PRAGMA user_version").await?;
            let n: i64 = scalar(c, "SELECT count(*) FROM sqlite_schema").await?;
            Ok((app, ver, n))
        })?;
        // `application_id` is a signed 32-bit header field.
        let app_id = u32::from_ne_bytes(i32::try_from(app_id).unwrap_or(0).to_ne_bytes());
        let empty = app_id == 0 && user_version == 0 && objects == 0;
        if app_id != DB_APPLICATION_ID && !empty {
            return Err(StorageError::Foreign { found: app_id });
        }
        Ok(())
    }

    fn apply_pragmas(&mut self, cfg: &OpenConfig) -> StorageResult<()> {
        let read_only = self.read_only;
        let want = match &self.location {
            Location::Memory => None,
            Location::File(path) => {
                let rollback = cfg.rollback_journal.unwrap_or_else(|| on_network_fs(path));
                Some(if rollback { "DELETE" } else { "WAL" })
            }
        };
        let journal = self.run(async move |c| {
            if let Some(mode) = want
                && !read_only
            {
                exec(c, &format!("PRAGMA journal_mode = {mode}")).await?;
            }
            for p in [
                "PRAGMA synchronous = NORMAL".to_owned(),
                "PRAGMA foreign_keys = ON".to_owned(),
                format!("PRAGMA busy_timeout = {}", BUSY_TIMEOUT.as_millis()),
                format!("PRAGMA journal_size_limit = {JOURNAL_SIZE_LIMIT}"),
                "PRAGMA wal_autocheckpoint = 1000".to_owned(),
                "PRAGMA temp_store = MEMORY".to_owned(),
                "PRAGMA trusted_schema = OFF".to_owned(),
                "PRAGMA cell_size_check = ON".to_owned(),
            ] {
                exec(c, &p).await?;
            }
            if read_only {
                exec(c, "PRAGMA query_only = ON").await?;
            }
            let mode: String = scalar(c, "PRAGMA journal_mode").await?;
            Ok(mode)
        })?;
        self.journal = JournalMode::parse(&journal);
        Ok(())
    }

    /// Run one async operation to completion on the private runtime.
    pub(crate) fn run<R>(
        &mut self,
        f: impl AsyncFnOnce(&mut SqlConn) -> StorageResult<R>,
    ) -> StorageResult<R> {
        crate::ui_thread::assert_not_ui_thread("Store (SQL)");
        let conn = self.conn.as_mut().ok_or(StorageError::Closed)?;
        block_on(&self.rt, f(conn))
    }

    /// Run `f` atomically: inside `BEGIN IMMEDIATE … COMMIT` at top level, or inside a
    /// savepoint when a transaction is already open. Any error rolls the work back.
    pub(crate) fn atomic<R>(
        &mut self,
        f: impl AsyncFnOnce(&mut SqlConn) -> StorageResult<R>,
    ) -> StorageResult<R> {
        let nested = self.depth > 0;
        if self.read_only && !nested {
            // A read-only connection cannot take the write lock; run the work in a deferred
            // transaction so that writes fail as `ReadOnly` and reads stay consistent.
            return self.snapshot(f);
        }
        let (begin, commit, rollback) = if nested {
            (
                "SAVEPOINT ep_atomic",
                "RELEASE ep_atomic",
                "ROLLBACK TO ep_atomic; RELEASE ep_atomic",
            )
        } else {
            ("BEGIN IMMEDIATE", "COMMIT", "ROLLBACK")
        };
        self.depth += 1;
        let result = self.run(async move |c| {
            exec(c, begin).await?;
            match f(c).await {
                Ok(v) => match exec(c, commit).await {
                    Ok(()) => Ok(v),
                    Err(e) => {
                        let _ = exec(c, rollback).await;
                        Err(e)
                    }
                },
                Err(e) => {
                    let _ = exec(c, rollback).await;
                    Err(e)
                }
            }
        });
        self.depth -= 1;
        result
    }

    /// Run read-only work in one deferred transaction (a consistent snapshot).
    pub(crate) fn snapshot<R>(
        &mut self,
        f: impl AsyncFnOnce(&mut SqlConn) -> StorageResult<R>,
    ) -> StorageResult<R> {
        if self.depth > 0 {
            return self.run(f);
        }
        self.depth += 1;
        let result = self.run(async move |c| {
            exec(c, "BEGIN").await?;
            let r = f(c).await;
            let end = exec(c, if r.is_ok() { "COMMIT" } else { "ROLLBACK" }).await;
            match (r, end) {
                (Ok(v), Ok(())) => Ok(v),
                (Err(e), _) | (Ok(_), Err(e)) => Err(e),
            }
        });
        self.depth -= 1;
        result
    }

    fn begin_tx(&mut self) -> StorageResult<()> {
        if self.read_only {
            return Err(StorageError::ReadOnly);
        }
        if self.depth > 0 {
            return Err(StorageError::Sql("a transaction is already open".into()));
        }
        self.run(async |c| exec(c, "BEGIN IMMEDIATE").await)?;
        self.depth = 1;
        Ok(())
    }

    fn end_tx(&mut self, commit: bool) -> StorageResult<()> {
        let r = self.run(async move |c| {
            if commit {
                match exec(c, "COMMIT").await {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        let _ = exec(c, "ROLLBACK").await;
                        Err(e)
                    }
                }
            } else {
                exec(c, "ROLLBACK").await
            }
        });
        self.depth = 0;
        r
    }

    /// An in-memory connection with the pragmas but no schema (tests).
    #[cfg(test)]
    pub(crate) fn open_raw_memory() -> StorageResult<Conn> {
        Conn::open(Location::Memory, false, &OpenConfig::default())
    }

    pub(crate) fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub(crate) fn file_path(&self) -> Option<&Path> {
        match &self.location {
            Location::File(p) => Some(p),
            Location::Memory => None,
        }
    }

    pub(crate) fn in_transaction(&self) -> bool {
        self.depth > 0
    }

    /// Close the connection (checkpointing the WAL when this was the last one).
    pub(crate) fn close(&mut self) -> StorageResult<()> {
        if let Some(conn) = self.conn.take() {
            block_on(&self.rt, conn.close())?;
        }
        Ok(())
    }

    /// Close and open the same file again (used after restoring a backup in place).
    pub(crate) fn reopen(&mut self, cfg: &OpenConfig) -> StorageResult<()> {
        self.close()?;
        let fresh = Conn::open(self.location.clone(), self.read_only, cfg)?;
        *self = fresh;
        Ok(())
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        if self.depth > 0 {
            let _ = self.run(async |c| exec(c, "ROLLBACK").await);
        }
        let _ = self.close();
    }
}

impl std::fmt::Debug for Conn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Conn")
            .field("location", &self.location)
            .field("read_only", &self.read_only)
            .field("journal", &self.journal)
            .field("open", &self.conn.is_some())
            .finish()
    }
}

fn block_on<F: Future>(rt: &tokio::runtime::Runtime, f: F) -> F::Output {
    debug_assert!(
        tokio::runtime::Handle::try_current().is_err(),
        "storage must not be called from inside a tokio runtime (ADR-0017)"
    );
    rt.block_on(f)
}

fn on_network_fs(db_file: &Path) -> bool {
    let fs = crate::fsx::RealFs::new();
    let Ok(table) = crate::fsx::MountTable::read(&fs) else {
        return false;
    };
    let probe = db_file.parent().unwrap_or(db_file);
    table
        .mount_point_of(probe)
        .is_some_and(crate::fsx::MountInfo::is_network_fs)
}

// ---- Low-level helpers used by every storage file ----------------------------------------

/// Execute one or more statements without parameters.
pub(crate) async fn exec(c: &mut SqlConn, sql: &str) -> StorageResult<()> {
    c.execute(ormlite::AssertSqlSafe(sql.to_owned())).await?;
    Ok(())
}

/// Fetch the first column of the first row.
pub(crate) async fn scalar<T>(c: &mut SqlConn, sql: &str) -> StorageResult<T>
where
    T: Send
        + Unpin
        + for<'r> ormlite::Decode<'r, ormlite::sqlite::Sqlite>
        + ormlite::types::Type<ormlite::sqlite::Sqlite>,
{
    let (v,) = ormlite::query_as::<_, (T,)>(ormlite::AssertSqlSafe(sql.to_owned()))
        .fetch_one(c)
        .await?;
    Ok(v)
}

impl From<ormlite::SqlxError> for StorageError {
    fn from(e: ormlite::SqlxError) -> Self {
        use ormlite::SqlxError as E;
        match e {
            E::Database(db) => {
                let ext: i64 = db.code().and_then(|c| c.parse().ok()).unwrap_or(0);
                classify(ext, db.message())
            }
            E::RowNotFound => StorageError::NotFound {
                what: "row",
                key: String::new(),
            },
            E::Io(io) if io.raw_os_error() == Some(28) => StorageError::DiskFull,
            other => StorageError::Sql(other.to_string()),
        }
    }
}

impl From<ormlite::Error> for StorageError {
    fn from(e: ormlite::Error) -> Self {
        match e {
            ormlite::Error::SqlxError(e) => e.into(),
            other => StorageError::Sql(other.to_string()),
        }
    }
}

fn classify(ext: i64, msg: &str) -> StorageError {
    use super::ConstraintKind as K;
    match ext & 0xff {
        5 | 6 => StorageError::Busy,
        8 => StorageError::ReadOnly,
        11 => StorageError::Corrupt(msg.to_owned()),
        13 => StorageError::DiskFull,
        26 => StorageError::NotADatabase,
        19 => {
            let target = msg.rsplit(": ").next().unwrap_or("");
            match ext {
                2067 | 1555 => match target {
                    "executables.canonical_path" => StorageError::DuplicateExecutablePath,
                    "executables.game_id" => StorageError::MultiplePrimaries,
                    "tags.name" => StorageError::DuplicateTag,
                    "runtimes.path" => StorageError::DuplicateRuntimePath,
                    _ => StorageError::Constraint {
                        kind: K::Unique,
                        detail: msg.to_owned(),
                    },
                },
                787 => StorageError::Constraint {
                    kind: K::ForeignKey,
                    detail: msg.to_owned(),
                },
                275 => StorageError::Constraint {
                    kind: K::Check,
                    detail: msg.to_owned(),
                },
                1299 => StorageError::Constraint {
                    kind: K::NotNull,
                    detail: msg.to_owned(),
                },
                _ => StorageError::Constraint {
                    kind: K::Other,
                    detail: msg.to_owned(),
                },
            }
        }
        _ if msg.contains("readonly") || msg.contains("read-only") => StorageError::ReadOnly,
        _ => StorageError::Sql(format!("{msg} (code {ext})")),
    }
}

// ---- Public handles ----------------------------------------------------------------------

mod sealed {
    /// Access to the connection behind a handle. Sealed: only `Db`, `ReadDb` and `Tx`.
    pub trait Sealed {
        fn conn(&mut self) -> &mut super::Conn;
    }
}

/// A database handle: [`Db`], [`ReadDb`] or [`Tx`]. Every repository trait is implemented
/// for every `Store`.
pub trait Store: Sealed {
    /// `PRAGMA user_version`.
    fn schema_version(&mut self) -> StorageResult<u32> {
        let v: i64 = self
            .conn()
            .run(async |c| scalar(c, "PRAGMA user_version").await)?;
        Ok(u32::try_from(v).unwrap_or(0))
    }

    /// `PRAGMA data_version`: changes when another connection commits (PERSISTENCE §6).
    fn data_version(&mut self) -> StorageResult<i64> {
        self.conn()
            .run(async |c| scalar(c, "PRAGMA data_version").await)
    }

    /// Whether the handle rejects writes.
    fn is_read_only(&mut self) -> bool {
        self.conn().is_read_only()
    }

    /// The database file, `None` for an in-memory database.
    fn file_path(&mut self) -> Option<PathBuf> {
        self.conn().file_path().map(Path::to_path_buf)
    }

    /// The value of a pragma as text (diagnostics and tests), e.g. `journal_mode`.
    fn pragma_text(&mut self, name: &str) -> StorageResult<String> {
        if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err(StorageError::Sql(format!("invalid pragma name {name:?}")));
        }
        let sql = format!("SELECT CAST((SELECT * FROM pragma_{name}) AS TEXT)");
        self.conn().run(async move |c| scalar(c, &sql).await)
    }
}

/// A read-write handle (AppService, supervisor, headless CLI).
#[derive(Debug)]
pub struct Db {
    conn: Conn,
    cfg: OpenConfig,
}

/// A read-only handle (`query_only = ON`), one per job-pool worker.
#[derive(Debug)]
pub struct ReadDb {
    conn: Conn,
}

/// An explicit `BEGIN IMMEDIATE` transaction. Dropping it without [`Tx::commit`] rolls it
/// back. Repository calls made through it run inside it.
#[derive(Debug)]
pub struct Tx<'a> {
    conn: &'a mut Conn,
    done: bool,
}

impl Sealed for Db {
    fn conn(&mut self) -> &mut Conn {
        &mut self.conn
    }
}
impl Store for Db {}

impl Sealed for ReadDb {
    fn conn(&mut self) -> &mut Conn {
        &mut self.conn
    }
}
impl Store for ReadDb {}

impl Sealed for Tx<'_> {
    fn conn(&mut self) -> &mut Conn {
        self.conn
    }
}
impl Store for Tx<'_> {}

pub(crate) use sealed::Sealed;

impl Db {
    /// Open (creating if missing) `path` read-write with the pragmas of PERSISTENCE §3.
    /// Refuses foreign SQLite files. Does not migrate: call [`Db::migrate`] next.
    pub fn open_rw(path: &Path) -> StorageResult<Db> {
        Self::open_rw_with(path, &OpenConfig::default())
    }

    /// [`Db::open_rw`] with explicit options.
    pub fn open_rw_with(path: &Path, cfg: &OpenConfig) -> StorageResult<Db> {
        let conn = Conn::open(Location::File(path.to_path_buf()), false, cfg)?;
        Ok(Db {
            conn,
            cfg: cfg.clone(),
        })
    }

    /// Open `path` read-only (`query_only = ON`). The file must exist and have the current
    /// schema version.
    pub fn open_ro(path: &Path) -> StorageResult<ReadDb> {
        ReadDb::open(path)
    }

    /// A fresh in-memory database with the current schema (tests, previews).
    pub fn open_in_memory() -> StorageResult<Db> {
        let conn = Conn::open(Location::Memory, false, &OpenConfig::default())?;
        let mut db = Db {
            conn,
            cfg: OpenConfig::default(),
        };
        match db.migrate(&migrate::MigrateOptions::default())? {
            migrate::MigrationOutcome::Ready | migrate::MigrationOutcome::Migrated { .. } => Ok(db),
            other => Err(StorageError::Sql(format!(
                "in-memory schema creation failed: {other:?}"
            ))),
        }
    }

    /// Start a `BEGIN IMMEDIATE` transaction (fails fast with [`StorageError::Busy`] after
    /// the busy timeout).
    pub fn begin(&mut self) -> StorageResult<Tx<'_>> {
        self.conn.begin_tx()?;
        Ok(Tx {
            conn: &mut self.conn,
            done: false,
        })
    }

    /// Run `f` inside a transaction: committed when it returns `Ok`, rolled back otherwise.
    pub fn transaction<R, E>(&mut self, f: impl FnOnce(&mut Tx<'_>) -> Result<R, E>) -> Result<R, E>
    where
        E: From<StorageError>,
    {
        let mut tx = self.begin()?;
        let r = f(&mut tx)?;
        tx.commit()?;
        Ok(r)
    }

    /// The journal mode in effect (`Delete` means a network filesystem was detected).
    pub fn journal_mode(&self) -> JournalMode {
        self.conn.journal
    }

    /// `PRAGMA wal_checkpoint(TRUNCATE)`, run at GUI exit when no session is active.
    pub fn checkpoint_truncate(&mut self) -> StorageResult<()> {
        self.conn
            .run(async |c| exec(c, "PRAGMA wal_checkpoint(TRUNCATE)").await)
    }

    /// Close the handle, reporting errors that dropping would swallow.
    pub fn close(mut self) -> StorageResult<()> {
        self.conn.close()
    }

    pub(crate) fn conn_mut(&mut self) -> &mut Conn {
        &mut self.conn
    }

    pub(crate) fn reopen(&mut self) -> StorageResult<()> {
        let cfg = self.cfg.clone();
        self.conn.reopen(&cfg)
    }

    /// Whether the handle is inside an explicit transaction (never true between calls).
    pub fn in_transaction(&self) -> bool {
        self.conn.in_transaction()
    }

    /// The database schema version this build expects.
    pub const SCHEMA_VERSION: u32 = DB_SCHEMA_VERSION;
}

impl ReadDb {
    /// Open `path` read-only.
    pub fn open(path: &Path) -> StorageResult<ReadDb> {
        Self::open_with(path, &OpenConfig::default())
    }

    /// [`ReadDb::open`] with explicit options.
    pub fn open_with(path: &Path, cfg: &OpenConfig) -> StorageResult<ReadDb> {
        let conn = Conn::open(Location::File(path.to_path_buf()), true, cfg)?;
        let mut db = ReadDb { conn };
        let v = db.schema_version()?;
        if v > DB_SCHEMA_VERSION {
            return Err(StorageError::NewerSchema {
                found: v,
                supported: DB_SCHEMA_VERSION,
            });
        }
        Ok(db)
    }

    /// The journal mode in effect.
    pub fn journal_mode(&self) -> JournalMode {
        self.conn.journal
    }

    /// Close the handle.
    pub fn close(mut self) -> StorageResult<()> {
        self.conn.close()
    }
}

impl Tx<'_> {
    /// Commit.
    pub fn commit(mut self) -> StorageResult<()> {
        self.done = true;
        self.conn.end_tx(true)
    }

    /// Roll back explicitly (dropping does the same).
    pub fn rollback(mut self) -> StorageResult<()> {
        self.done = true;
        self.conn.end_tx(false)
    }
}

impl Drop for Tx<'_> {
    fn drop(&mut self) {
        if !self.done {
            let _ = self.conn.end_tx(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn pragmas_are_applied() {
        let dir = tmp();
        let mut db = Db::open_rw(&dir.path().join("library.db")).unwrap();
        assert_eq!(db.journal_mode(), JournalMode::Wal);
        assert_eq!(db.pragma_text("journal_mode").unwrap(), "wal");
        assert_eq!(db.pragma_text("synchronous").unwrap(), "1");
        assert_eq!(db.pragma_text("foreign_keys").unwrap(), "1");
        assert_eq!(db.pragma_text("busy_timeout").unwrap(), "5000");
        assert_eq!(db.pragma_text("journal_size_limit").unwrap(), "16777216");
        assert_eq!(db.pragma_text("temp_store").unwrap(), "2");
        assert_eq!(db.pragma_text("trusted_schema").unwrap(), "0");
        assert_eq!(db.pragma_text("cell_size_check").unwrap(), "1");
        assert_eq!(db.pragma_text("query_only").unwrap(), "0");
    }

    #[test]
    fn rollback_journal_when_requested() {
        let dir = tmp();
        let cfg = OpenConfig {
            rollback_journal: Some(true),
        };
        let db = Db::open_rw_with(&dir.path().join("library.db"), &cfg).unwrap();
        assert_eq!(db.journal_mode(), JournalMode::Delete);
    }

    #[test]
    fn foreign_sqlite_file_is_refused() {
        let dir = tmp();
        let path = dir.path().join("other.db");
        {
            let mut conn =
                Conn::open(Location::File(path.clone()), false, &OpenConfig::default()).unwrap();
            conn.run(async |c| exec(c, "CREATE TABLE notes (x TEXT)").await)
                .unwrap();
        }
        assert!(matches!(
            Db::open_rw(&path),
            Err(StorageError::Foreign { found: 0 })
        ));
        assert!(matches!(
            Db::open_ro(&path),
            Err(StorageError::Foreign { .. })
        ));
    }

    #[test]
    fn non_database_file_is_reported() {
        let dir = tmp();
        let path = dir.path().join("junk.db");
        crate::fsx::atomic_write_real(&path, &[0x42; 8192], 0o600).unwrap();
        let err = Db::open_rw(&path).unwrap_err();
        assert!(matches!(err, StorageError::NotADatabase), "{err:?}");
        assert!(err.is_corruption());
    }

    #[test]
    fn read_only_handle_rejects_writes() {
        let dir = tmp();
        let path = dir.path().join("library.db");
        let mut db = Db::open_rw(&path).unwrap();
        db.migrate(&migrate::MigrateOptions::default()).unwrap();
        let mut ro = Db::open_ro(&path).unwrap();
        assert_eq!(ro.pragma_text("query_only").unwrap(), "1");
        let err = ro
            .conn()
            .run(async |c| {
                exec(
                    c,
                    "INSERT INTO kv(key, value, updated_at) VALUES ('a', 'b', 0)",
                )
                .await
            })
            .unwrap_err();
        assert!(matches!(err, StorageError::ReadOnly), "{err:?}");
        assert_eq!(ro.schema_version().unwrap(), DB_SCHEMA_VERSION);
    }

    #[test]
    fn tx_rolls_back_on_drop_and_commits_explicitly() {
        let mut db = Db::open_in_memory().unwrap();
        let insert = async |c: &mut SqlConn| {
            exec(
                c,
                "INSERT INTO kv(key, value, updated_at) VALUES ('k', 'v', 0)",
            )
            .await
        };
        {
            let mut tx = db.begin().unwrap();
            tx.conn().run(insert).unwrap();
        }
        let count = async |c: &mut SqlConn| scalar::<i64>(c, "SELECT count(*) FROM kv").await;
        assert_eq!(db.conn().run(count).unwrap(), 0);
        let mut tx = db.begin().unwrap();
        tx.conn().run(insert).unwrap();
        tx.commit().unwrap();
        assert_eq!(db.conn().run(count).unwrap(), 1);
        assert!(!db.in_transaction());
    }

    #[test]
    fn atomic_nests_as_savepoint() {
        let mut db = Db::open_in_memory().unwrap();
        let mut tx = db.begin().unwrap();
        let r: StorageResult<()> = tx.conn().atomic(async |c| {
            exec(
                c,
                "INSERT INTO kv(key, value, updated_at) VALUES ('a', 'v', 0)",
            )
            .await?;
            Err(StorageError::Busy)
        });
        assert!(r.is_err());
        tx.conn()
            .atomic(async |c| {
                exec(
                    c,
                    "INSERT INTO kv(key, value, updated_at) VALUES ('b', 'v', 0)",
                )
                .await
            })
            .unwrap();
        tx.commit().unwrap();
        let keys: Vec<String> = db
            .conn()
            .run(async |c| {
                ormlite::query_as::<_, (String,)>("SELECT key FROM kv ORDER BY key")
                    .fetch_all(c)
                    .await
                    .map(|v| v.into_iter().map(|(k,)| k).collect())
                    .map_err(StorageError::from)
            })
            .unwrap();
        assert_eq!(keys, vec!["b".to_owned()]);
    }
}
