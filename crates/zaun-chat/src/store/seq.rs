//! Never-reused ids for friends, groups and members.

use super::StoreError;
use super::handle::Store;

impl Store {
    /// The next id of the counter `name`, starting at 1. Deleting rows never lowers a counter.
    pub fn next_id(&mut self, name: &str) -> Result<i64, StoreError> {
        let (id,): (i64,) = self.db.run(async |c| {
            Ok(ormlite::query_as(
                "INSERT INTO seq (name, next) VALUES (?1, 1) \
                 ON CONFLICT (name) DO UPDATE SET next = next + 1 RETURNING next",
            )
            .bind(name)
            .fetch_one(c)
            .await?)
        })?;
        Ok(id)
    }
}
