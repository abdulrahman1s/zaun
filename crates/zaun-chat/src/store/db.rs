//! One SQLite connection and the private current-thread runtime that drives it (ADR-0017).

use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use ormlite::sqlite::{
    SqliteConnectOptions, SqliteConnection, SqliteJournalMode, SqliteSynchronous,
};
use ormlite::{ConnectOptions, Connection, Executor};

use super::StoreError;

pub(super) struct Db {
    rt: tokio::runtime::Runtime,
    conn: Option<SqliteConnection>,
}

impl Db {
    pub(super) fn open(path: &Path) -> Result<Db, StoreError> {
        // Created private before SQLite makes it, so the -wal and -shm files inherit 0600.
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)?;
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .map_err(|e| StoreError::Open(format!("runtime: {e}")))?;
        let options = SqliteConnectOptions::new()
            .filename(path)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal);
        let conn = rt.block_on(options.connect())?;
        Ok(Db {
            rt,
            conn: Some(conn),
        })
    }

    pub(super) fn run<R>(
        &mut self,
        f: impl AsyncFnOnce(&mut SqliteConnection) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        let conn = self
            .conn
            .as_mut()
            .ok_or(StoreError::Invalid("store is closed"))?;
        self.rt.block_on(f(conn))
    }

    /// `f` inside `BEGIN IMMEDIATE … COMMIT`; any error rolls everything back.
    pub(super) fn atomic<R>(
        &mut self,
        f: impl AsyncFnOnce(&mut SqliteConnection) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        self.run(async |c| {
            exec(c, "BEGIN IMMEDIATE").await?;
            match f(c).await {
                Ok(v) => match exec(c, "COMMIT").await {
                    Ok(()) => Ok(v),
                    Err(e) => {
                        let _ = exec(c, "ROLLBACK").await; // the commit error is the one to report
                        Err(e)
                    }
                },
                Err(e) => {
                    let _ = exec(c, "ROLLBACK").await; // the failing op's error is the one to report
                    Err(e)
                }
            }
        })
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            let _ = self.rt.block_on(conn.close()); // nothing to do about a failed close on exit
        }
    }
}

pub(super) async fn exec(c: &mut SqliteConnection, sql: &str) -> Result<(), StoreError> {
    c.execute(ormlite::AssertSqlSafe(sql.to_owned())).await?;
    Ok(())
}
