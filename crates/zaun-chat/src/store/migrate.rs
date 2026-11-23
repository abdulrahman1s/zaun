//! Numbered SQL migrations, each in its own transaction ending with `user_version`.

use ormlite::sqlite::SqliteConnection;

use super::StoreError;
use super::db::exec;

const MIGRATIONS: &[&str] = &[include_str!("migrations/0001_init.sql")];

pub(super) async fn run(c: &mut SqliteConnection) -> Result<(), StoreError> {
    let (version,): (i64,) = ormlite::query_as("PRAGMA user_version")
        .fetch_one(&mut *c)
        .await?;
    let done = usize::try_from(version).unwrap_or(usize::MAX);
    if done > MIGRATIONS.len() {
        return Err(StoreError::Corrupt(
            "the store was written by a newer version".into(),
        ));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(done) {
        exec(c, "BEGIN IMMEDIATE").await?;
        let r = async {
            exec(c, sql).await?;
            exec(c, &format!("PRAGMA user_version = {}", i + 1)).await
        }
        .await;
        match r {
            Ok(()) => exec(c, "COMMIT").await?,
            Err(e) => {
                let _ = exec(c, "ROLLBACK").await; // the migration's error is the one to report
                return Err(e);
            }
        }
    }
    Ok(())
}
