//! File transfer records, sealed against their id.

use ormlite::sqlite::SqliteConnection;

use super::StoreError;
use super::handle::Store;
use super::seal::Sealer;

pub(super) async fn put(
    c: &mut SqliteConnection,
    s: &Sealer,
    id: i64,
    v: &[u8],
) -> Result<(), StoreError> {
    let sealed = s.seal(&[b"files", &id.to_le_bytes()], v)?;
    ormlite::query(
        "INSERT INTO files (id, v) VALUES (?1, ?2) ON CONFLICT (id) DO UPDATE SET v = excluded.v",
    )
    .bind(id)
    .bind(sealed)
    .execute(c)
    .await?;
    Ok(())
}

pub(super) async fn delete(c: &mut SqliteConnection, id: i64) -> Result<(), StoreError> {
    ormlite::query("DELETE FROM files WHERE id = ?1")
        .bind(id)
        .execute(c)
        .await?;
    Ok(())
}

impl Store {
    pub fn put_file(&mut self, id: i64, v: &[u8]) -> Result<(), StoreError> {
        let s = &self.sealer;
        self.db.run(async |c| put(c, s, id, v).await)
    }

    pub fn get_file(&mut self, id: i64) -> Result<Option<Vec<u8>>, StoreError> {
        let row: Option<(Vec<u8>,)> = self.db.run(async |c| {
            Ok(ormlite::query_as("SELECT v FROM files WHERE id = ?1")
                .bind(id)
                .fetch_optional(c)
                .await?)
        })?;
        row.map(|(sealed,)| self.sealer.open(&[b"files", &id.to_le_bytes()], &sealed))
            .transpose()
    }

    pub fn delete_file(&mut self, id: i64) -> Result<(), StoreError> {
        self.db.run(async |c| delete(c, id).await)
    }
}
