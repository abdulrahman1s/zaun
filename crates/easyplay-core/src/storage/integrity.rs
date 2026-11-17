//! Integrity checks and salvage (PERSISTENCE §8).
//!
//! `quick_check` runs at every start; a damaged library is salvaged by copying every
//! readable row, table by table, into a fresh database with the current schema. Rows that
//! cannot be read or that violate a constraint are skipped and counted. Within a table, rows
//! are first copied in bulk; if that fails the table is walked by key in ascending order up
//! to the damage and then in descending order from the end, so rows on both sides of a
//! damaged page survive. The damaged file is never modified (it is attached read-only) and
//! is kept as `backups/corrupt-<ts>.db` by [`swap_in`].

use std::path::{Path, PathBuf};

use super::db::{SqlConn, exec};
use super::migrate::MigrateOptions;
use super::{Db, LibraryFiles, MigrationOutcome, ReadDb, StorageError, StorageResult, Store};
use crate::clock::Timestamp;
use crate::fsx::{Fs, RealFs};

/// Result of `quick_check` / `integrity_check`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntegrityReport {
    /// `ok`.
    Ok,
    /// The problems SQLite listed (at most 100).
    Problems(Vec<String>),
}

impl IntegrityReport {
    /// Whether the check passed.
    pub fn is_ok(&self) -> bool {
        matches!(self, IntegrityReport::Ok)
    }
}

async fn check(c: &mut SqlConn, pragma: &str) -> StorageResult<IntegrityReport> {
    let rows: Vec<(String,)> =
        ormlite::query_as(ormlite::AssertSqlSafe(format!("PRAGMA {pragma}(100)")))
            .fetch_all(&mut *c)
            .await?;
    let lines: Vec<String> = rows.into_iter().map(|(l,)| l).collect();
    Ok(if lines.len() == 1 && lines[0] == "ok" {
        IntegrityReport::Ok
    } else {
        IntegrityReport::Problems(lines)
    })
}

fn corruption_report(e: StorageError) -> StorageResult<IntegrityReport> {
    match e {
        StorageError::Corrupt(m) => Ok(IntegrityReport::Problems(vec![m])),
        StorageError::NotADatabase => Ok(IntegrityReport::Problems(vec![
            "file is not a database".into(),
        ])),
        other => Err(other),
    }
}

/// `PRAGMA quick_check`; corruption errors raised while checking become problems.
pub fn quick_check(store: &mut (impl Store + ?Sized)) -> StorageResult<IntegrityReport> {
    super::repo::conn(store)
        .run(async |c| check(c, "quick_check").await)
        .or_else(corruption_report)
}

/// `PRAGMA integrity_check` (slower; restore validation and salvage output).
pub fn integrity_check(store: &mut (impl Store + ?Sized)) -> StorageResult<IntegrityReport> {
    super::repo::conn(store)
        .run(async |c| check(c, "integrity_check").await)
        .or_else(corruption_report)
}

/// Open `path` read-only and `quick_check` it. A file that cannot be opened because it is
/// damaged yields `Problems`; foreign files and I/O errors are errors.
pub fn check_file(path: &Path) -> StorageResult<IntegrityReport> {
    match ReadDb::open(path) {
        Ok(mut db) => quick_check(&mut db),
        Err(e) => corruption_report(e),
    }
}

/// What salvage did with one table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableSalvage {
    /// Table name.
    pub table: &'static str,
    /// Rows the damaged file reports (`None` when it cannot even count them).
    pub source_rows: Option<u64>,
    /// Rows copied.
    pub copied: u64,
    /// Rows lost (`source_rows − copied`), `None` when unknown.
    pub lost: Option<u64>,
    /// Whether the bulk copy failed and the table was walked row by row.
    pub walked: bool,
}

/// The report consumed by the corruption flow (PERSISTENCE §8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SalvageReport {
    /// Per table, in copy order.
    pub tables: Vec<TableSalvage>,
    /// Whether every row of every table was recovered.
    pub complete: bool,
    /// Whether every game was recovered (the "repaired" banner case).
    pub all_games_present: bool,
    /// Games copied.
    pub games_recovered: u64,
    /// Rows of history tables (`sessions`, `session_events`) lost, when known.
    pub history_rows_lost: Option<u64>,
}

impl SalvageReport {
    /// The entry of one table.
    pub fn table(&self, name: &str) -> Option<&TableSalvage> {
        self.tables.iter().find(|t| t.table == name)
    }
}

struct TableSpec {
    name: &'static str,
    /// Ordering key: `rowid` for rowid tables, the primary-key columns otherwise.
    key: &'static [&'static str],
}

/// Copy order respects foreign keys.
const TABLES: &[TableSpec] = &[
    TableSpec {
        name: "meta",
        key: &["key"],
    },
    TableSpec {
        name: "games",
        key: &["rowid"],
    },
    TableSpec {
        name: "executables",
        key: &["rowid"],
    },
    TableSpec {
        name: "tags",
        key: &["rowid"],
    },
    TableSpec {
        name: "game_tags",
        key: &["game_id", "tag_id"],
    },
    TableSpec {
        name: "artwork",
        key: &["game_id", "slot"],
    },
    TableSpec {
        name: "sessions",
        key: &["rowid"],
    },
    TableSpec {
        name: "session_events",
        key: &["rowid"],
    },
    TableSpec {
        name: "game_stats",
        key: &["game_id"],
    },
    TableSpec {
        name: "playtime_daily",
        key: &["game_key", "day"],
    },
    TableSpec {
        name: "runtimes",
        key: &["name"],
    },
    TableSpec {
        name: "slr_installs",
        key: &["variant"],
    },
    TableSpec {
        name: "runtime_audit",
        key: &["variant"],
    },
    TableSpec {
        name: "runtime_events",
        key: &["rowid"],
    },
    TableSpec {
        name: "kv",
        key: &["key"],
    },
];

/// A `file:` URI opening `path` read-only.
fn ro_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::from("file:");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'-' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out.push_str("?mode=ro");
    out
}

async fn columns(c: &mut SqlConn, schema: &str, table: &str) -> StorageResult<Vec<String>> {
    let rows: Vec<(String,)> = ormlite::query_as(ormlite::AssertSqlSafe(format!(
        "SELECT name FROM pragma_table_info('{table}', '{schema}') ORDER BY cid"
    )))
    .fetch_all(&mut *c)
    .await?;
    Ok(rows.into_iter().map(|(n,)| n).collect())
}

async fn count(c: &mut SqlConn, sql: &str) -> StorageResult<u64> {
    let (n,): (i64,) = ormlite::query_as(ormlite::AssertSqlSafe(sql.to_owned()))
        .fetch_one(&mut *c)
        .await?;
    Ok(u64::try_from(n).unwrap_or(0))
}

async fn changes(c: &mut SqlConn, sql: &str) -> StorageResult<u64> {
    let done = ormlite::query(ormlite::AssertSqlSafe(sql.to_owned()))
        .execute(&mut *c)
        .await?;
    Ok(done.rows_affected())
}

async fn next_key(c: &mut SqlConn, sql: &str) -> StorageResult<Option<String>> {
    let row: Option<(String,)> = ormlite::query_as(ormlite::AssertSqlSafe(sql.to_owned()))
        .fetch_optional(&mut *c)
        .await?;
    Ok(row.map(|(k,)| k))
}

async fn salvage_table(c: &mut SqlConn, spec: &TableSpec) -> StorageResult<TableSalvage> {
    let t = spec.name;
    let ours = columns(c, "main", t).await?;
    let theirs = columns(c, "old", t).await.unwrap_or_default();
    let cols: Vec<&String> = ours.iter().filter(|c| theirs.contains(c)).collect();
    if cols.is_empty() {
        let source_rows = count(c, &format!("SELECT count(*) FROM old.\"{t}\""))
            .await
            .ok();
        return Ok(TableSalvage {
            table: t,
            source_rows,
            copied: 0,
            lost: source_rows,
            walked: false,
        });
    }
    let col_list = cols
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    // Keep sessions of games that did not survive, detached like after a removal.
    let select_list = cols
        .iter()
        .map(|col| {
            if t == "sessions" && col.as_str() == "game_id" {
                "CASE WHEN game_id IN (SELECT id FROM main.games) THEN game_id END".to_owned()
            } else {
                format!("\"{col}\"")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let verb = if t == "meta" {
        "INSERT OR REPLACE"
    } else {
        "INSERT OR IGNORE"
    };
    let insert =
        format!("{verb} INTO main.\"{t}\"({col_list}) SELECT {select_list} FROM old.\"{t}\"");
    let source_rows = count(c, &format!("SELECT count(*) FROM old.\"{t}\""))
        .await
        .ok();

    // One statement: it either copies the whole table or (on a read error or a foreign-key
    // violation) changes nothing. Statements run in autocommit mode because SQLite may roll
    // back an enclosing transaction after reporting corruption.
    if let Ok(n) = changes(c, &insert).await {
        return Ok(TableSalvage {
            table: t,
            source_rows,
            copied: n,
            lost: source_rows.map(|s| s.saturating_sub(n)),
            walked: false,
        });
    }

    // Walk by key: ascending up to the damage, then descending from the end.
    let keys = spec.key.join(", ");
    let quoted = spec
        .key
        .iter()
        .map(|k| format!("quote({k})"))
        .collect::<Vec<_>>()
        .join(" || ',' || ");
    let mut copied = 0u64;
    let mut asc_last: Option<String> = None;
    let mut complete = false;
    loop {
        let filter = asc_last
            .as_ref()
            .map(|l| format!("WHERE ({keys}) > ({l})"))
            .unwrap_or_default();
        let sql = format!("SELECT {quoted} FROM old.\"{t}\" {filter} ORDER BY {keys} LIMIT 1");
        match next_key(c, &sql).await {
            Ok(Some(k)) => {
                copied += copy_row(c, &insert, &keys, &k).await;
                asc_last = Some(k);
            }
            Ok(None) => {
                complete = true;
                break;
            }
            Err(_) => break,
        }
    }
    if !complete {
        let mut desc_last: Option<String> = None;
        loop {
            let mut conds = Vec::new();
            if let Some(l) = &desc_last {
                conds.push(format!("({keys}) < ({l})"));
            }
            if let Some(l) = &asc_last {
                conds.push(format!("({keys}) > ({l})"));
            }
            let filter = if conds.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conds.join(" AND "))
            };
            let sql = format!(
                "SELECT {quoted} FROM old.\"{t}\" {filter} ORDER BY {} LIMIT 1",
                spec.key
                    .iter()
                    .map(|k| format!("{k} DESC"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            match next_key(c, &sql).await {
                Ok(Some(k)) => {
                    copied += copy_row(c, &insert, &keys, &k).await;
                    desc_last = Some(k);
                }
                Ok(None) | Err(_) => break,
            }
        }
    }
    Ok(TableSalvage {
        table: t,
        source_rows,
        copied,
        lost: source_rows.map(|s| s.saturating_sub(copied)),
        walked: true,
    })
}

/// Restore the model invariant "exactly one primary executable" (DATA_MODEL §2) on the
/// salvaged rows: a game whose primary was lost gets its oldest remaining executable as
/// primary; a game without any executable cannot be launched or recognised and is dropped
/// (counted as lost).
async fn enforce_game_invariants(
    c: &mut SqlConn,
    tables: &mut [TableSalvage],
) -> StorageResult<()> {
    exec(
        c,
        "UPDATE main.executables SET role = 'primary' WHERE id IN ( \
           SELECT (SELECT e.id FROM main.executables e WHERE e.game_id = g.id \
                   ORDER BY e.added_at, e.id LIMIT 1) \
           FROM main.games g WHERE NOT EXISTS (SELECT 1 FROM main.executables p \
                 WHERE p.game_id = g.id AND p.role = 'primary'))",
    )
    .await?;
    let dropped = changes(
        c,
        "DELETE FROM main.games WHERE NOT EXISTS \
         (SELECT 1 FROM main.executables e WHERE e.game_id = main.games.id)",
    )
    .await?;
    if let Some(g) = tables.iter_mut().find(|t| t.table == "games") {
        g.copied = g.copied.saturating_sub(dropped);
        g.lost = g.source_rows.map(|s| s.saturating_sub(g.copied));
    }
    Ok(())
}

/// Copy the row with key literal `key`; 0 when unreadable or rejected by a constraint.
async fn copy_row(c: &mut SqlConn, insert: &str, keys: &str, key: &str) -> u64 {
    let sql = format!("{insert} WHERE ({keys}) = ({key})");
    changes(c, &sql).await.unwrap_or(0)
}

/// Copy every readable row of `damaged` into a new database at `out` (which must not
/// exist), created with the current schema. `damaged` is opened read-only and not changed.
pub fn salvage(damaged: &Path, out: &Path, now: Timestamp) -> StorageResult<SalvageReport> {
    let fs = RealFs::new();
    if fs.exists(out) {
        return Err(StorageError::Backup(format!(
            "salvage target exists: {}",
            out.display()
        )));
    }
    let mut db = Db::open_rw(out)?;
    match db.migrate(&MigrateOptions {
        backups_dir: None,
        now,
    })? {
        MigrationOutcome::Migrated { .. } | MigrationOutcome::Ready => {}
        other => return Err(StorageError::Sql(format!("salvage schema: {other:?}"))),
    }
    let uri = ro_uri(damaged);
    let tables = super::repo::conn(&mut db).run(async move |c| {
        ormlite::query("ATTACH DATABASE ? AS old")
            .bind(uri)
            .execute(&mut *c)
            .await?;
        // Reading the attached schema fails when the damaged file's header or schema page is
        // unreadable: then nothing can be recovered.
        let readable = count(c, "SELECT count(*) FROM old.sqlite_schema").await;
        let mut out = Vec::new();
        if readable.is_ok() {
            for spec in TABLES {
                out.push(salvage_table(c, spec).await?);
            }
            enforce_game_invariants(c, &mut out).await?;
        } else {
            for spec in TABLES {
                out.push(TableSalvage {
                    table: spec.name,
                    source_rows: None,
                    copied: 0,
                    lost: None,
                    walked: false,
                });
            }
        }
        exec(c, "DETACH DATABASE old").await?;
        Ok(out)
    })?;

    match integrity_check(&mut db)? {
        IntegrityReport::Ok => {}
        IntegrityReport::Problems(p) => return Err(StorageError::Corrupt(p.join("; "))),
    }
    let fk: u64 = super::repo::conn(&mut db)
        .run(async |c| count(c, "SELECT count(*) FROM pragma_foreign_key_check").await)?;
    if fk != 0 {
        return Err(StorageError::Sql(format!(
            "salvage left {fk} foreign-key violations"
        )));
    }
    db.close()?;

    let games = tables.iter().find(|t| t.table == "games");
    let history_rows_lost = tables
        .iter()
        .filter(|t| matches!(t.table, "sessions" | "session_events"))
        .map(|t| t.lost)
        .sum::<Option<u64>>();
    Ok(SalvageReport {
        complete: tables.iter().all(|t| t.lost == Some(0)),
        all_games_present: games.is_some_and(|g| g.lost == Some(0)),
        games_recovered: games.map_or(0, |g| g.copied),
        history_rows_lost,
        tables,
    })
}

/// Replace the library with a salvaged (or otherwise repaired) file: the damaged database
/// is kept as `backups/corrupt-<ts>.db`, which is returned. Every handle on the library must
/// be closed.
pub fn swap_in(
    files: &LibraryFiles,
    repaired: &Path,
    now: Timestamp,
) -> StorageResult<Option<PathBuf>> {
    let kept = super::backup::move_aside(files, "corrupt", now)?;
    let fs = RealFs::new();
    if fs.rename(repaired, &files.db_file).is_err() {
        super::backup::copy_database_file(repaired, &files.db_file)?;
        let _ = fs.remove_file(repaired);
    }
    Ok(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_escapes_special_bytes() {
        assert_eq!(
            ro_uri(Path::new("/tmp/a b?#%.db")),
            "file:/tmp/a%20b%3F%23%25.db?mode=ro"
        );
    }

    #[test]
    fn healthy_database_passes_both_checks() {
        let mut db = Db::open_in_memory().unwrap();
        assert!(quick_check(&mut db).unwrap().is_ok());
        assert!(integrity_check(&mut db).unwrap().is_ok());
    }
}
