//! `kv` (UI state) and `meta`.

use ormlite::Model;

use super::{Arg, KvRepo, MetaKey, conn, exec_args};
use crate::clock::Timestamp;
use crate::storage::db::{map_orm, map_sqlx};
use crate::storage::{StorageResult, Store};

/// A `kv` row.
#[derive(Model, Clone, Debug, PartialEq)]
#[ormlite(table = "kv")]
struct KvRow {
    #[ormlite(primary_key)]
    key: String,
    value: String,
    updated_at: i64,
}

/// A `meta` row.
#[derive(Model, Clone, Debug, PartialEq)]
#[ormlite(table = "meta")]
struct MetaRow {
    #[ormlite(primary_key)]
    key: String,
    value: String,
}

impl<S: Store + ?Sized> KvRepo for S {
    fn kv_get(&mut self, key: &str) -> StorageResult<Option<String>> {
        let key = key.to_owned();
        conn(self).run(async move |c| {
            Ok(KvRow::select()
                .where_bind("key = ?", key)
                .fetch_optional(&mut *c)
                .await
                .map_err(map_orm)?
                .map(|r| r.value))
        })
    }

    fn kv_set(&mut self, key: &str, value: &str, now: Timestamp) -> StorageResult<()> {
        let args = [Arg::text(key), Arg::text(value), Arg::int(now.as_millis())];
        conn(self).atomic(async move |c| {
            exec_args(
                c,
                "INSERT INTO kv(key, value, updated_at) VALUES (?, ?, ?) ON CONFLICT(key) DO \
                 UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                &args,
            )
            .await
            .map(|_| ())
        })
    }

    fn kv_delete(&mut self, key: &str) -> StorageResult<bool> {
        let args = [Arg::text(key)];
        conn(self).atomic(async move |c| {
            exec_args(c, "DELETE FROM kv WHERE key = ?", &args)
                .await
                .map(|n| n > 0)
        })
    }

    fn kv_with_prefix(&mut self, prefix: &str) -> StorageResult<Vec<(String, String)>> {
        let prefix = prefix.to_owned();
        conn(self).run(async move |c| {
            Ok(
                KvRow::query("SELECT * FROM kv WHERE substr(key, 1, length(?)) = ? ORDER BY key")
                    .bind(prefix.clone())
                    .bind(prefix)
                    .fetch_all(&mut *c)
                    .await
                    .map_err(map_sqlx)?
                    .into_iter()
                    .map(|r| (r.key, r.value))
                    .collect(),
            )
        })
    }

    fn meta_get(&mut self, key: MetaKey) -> StorageResult<Option<String>> {
        conn(self).run(async move |c| {
            Ok(MetaRow::select()
                .where_bind("key = ?", key.as_str())
                .fetch_optional(&mut *c)
                .await
                .map_err(map_orm)?
                .map(|r| r.value))
        })
    }

    fn meta_set(&mut self, key: MetaKey, value: &str) -> StorageResult<()> {
        let args = [Arg::text(key.as_str()), Arg::text(value)];
        conn(self).atomic(async move |c| {
            exec_args(
                c,
                "INSERT INTO meta(key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET \
                 value = excluded.value",
                &args,
            )
            .await
            .map(|_| ())
        })
    }
}
