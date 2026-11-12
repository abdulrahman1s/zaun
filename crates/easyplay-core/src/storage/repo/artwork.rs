//! `artwork` (composite key: raw queries with a derived `FromRow`).

use std::collections::BTreeSet;

use super::{Arg, ArtworkRepo, conn, exec_args, hash32, id_parse, ts, uint32};
use crate::ids::GameId;
use crate::model::{ArtOrigin, ArtSlot, ArtworkRef, Sha256};
use crate::storage::db::{SqlConn, map_sqlx};
use crate::storage::{StorageError, StorageResult, Store};

#[derive(ormlite::FromRow)]
struct ArtRow {
    game_id: String,
    slot: String,
    file: String,
    hash: Vec<u8>,
    width: i64,
    height: i64,
    source: String,
    updated_at: i64,
}

fn slot_parse(s: &str) -> StorageResult<ArtSlot> {
    ArtSlot::ALL
        .into_iter()
        .find(|v| v.as_str() == s)
        .ok_or_else(|| StorageError::Decode(format!("artwork.slot {s:?}")))
}

fn origin_parse(s: &str) -> StorageResult<ArtOrigin> {
    [
        ArtOrigin::File,
        ArtOrigin::ExecutableIcon,
        ArtOrigin::Generated,
    ]
    .into_iter()
    .find(|v| v.as_str() == s)
    .ok_or_else(|| StorageError::Decode(format!("artwork.source {s:?}")))
}

impl ArtRow {
    fn into_ref(self) -> StorageResult<ArtworkRef> {
        Ok(ArtworkRef {
            game_id: id_parse("artwork.game_id", &self.game_id)?,
            slot: slot_parse(&self.slot)?,
            file: self.file,
            hash: hash32("artwork.hash", &self.hash)?,
            width: uint32(self.width),
            height: uint32(self.height),
            source: origin_parse(&self.source)?,
            updated_at: ts(self.updated_at),
        })
    }
}

pub(super) async fn fetch_all(c: &mut SqlConn) -> StorageResult<Vec<ArtworkRef>> {
    ormlite::query_as::<_, ArtRow>("SELECT * FROM artwork ORDER BY game_id, slot")
        .fetch_all(&mut *c)
        .await
        .map_err(map_sqlx)?
        .into_iter()
        .map(ArtRow::into_ref)
        .collect()
}

impl<S: Store + ?Sized> ArtworkRepo for S {
    fn upsert_artwork(&mut self, art: &ArtworkRef) -> StorageResult<()> {
        if art.file.is_empty() || art.file.contains('/') || art.file.contains('\0') {
            return Err(StorageError::Invalid(crate::model::ValidationError::new(
                "artwork.file",
                crate::model::Problem::InvalidFormat,
            )));
        }
        let args = [
            Arg::text(art.game_id.to_string()),
            Arg::text(art.slot.as_str()),
            Arg::text(art.file.clone()),
            Arg::Blob(Some(art.hash.to_vec())),
            Arg::int(i64::from(art.width)),
            Arg::int(i64::from(art.height)),
            Arg::text(art.source.as_str()),
            Arg::int(art.updated_at.as_millis()),
        ];
        conn(self).atomic(async move |c| {
            exec_args(
                c,
                "INSERT INTO artwork(game_id, slot, file, hash, width, height, source, updated_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT(game_id, slot) DO UPDATE SET file = excluded.file, \
                 hash = excluded.hash, width = excluded.width, height = excluded.height, \
                 source = excluded.source, updated_at = excluded.updated_at",
                &args,
            )
            .await
            .map(|_| ())
        })
    }

    fn artwork_of(&mut self, game: GameId) -> StorageResult<Vec<ArtworkRef>> {
        conn(self).run(async move |c| {
            ormlite::query_as::<_, ArtRow>("SELECT * FROM artwork WHERE game_id = ? ORDER BY slot")
                .bind(game.to_string())
                .fetch_all(&mut *c)
                .await
                .map_err(map_sqlx)?
                .into_iter()
                .map(ArtRow::into_ref)
                .collect()
        })
    }

    fn delete_artwork(&mut self, game: GameId, slot: ArtSlot) -> StorageResult<bool> {
        conn(self).atomic(async move |c| {
            exec_args(
                c,
                "DELETE FROM artwork WHERE game_id = ? AND slot = ?",
                &[Arg::text(game.to_string()), Arg::text(slot.as_str())],
            )
            .await
            .map(|n| n > 0)
        })
    }

    fn referenced_artwork_hashes(&mut self) -> StorageResult<BTreeSet<Sha256>> {
        conn(self).run(async |c| {
            let rows: Vec<(Vec<u8>,)> = ormlite::query_as("SELECT DISTINCT hash FROM artwork")
                .fetch_all(&mut *c)
                .await
                .map_err(map_sqlx)?;
            rows.iter().map(|(h,)| hash32("artwork.hash", h)).collect()
        })
    }

    fn unreferenced_artwork(&mut self, candidates: &[Sha256]) -> StorageResult<Vec<Sha256>> {
        let used = self.referenced_artwork_hashes()?;
        let mut out: Vec<Sha256> = candidates
            .iter()
            .filter(|h| !used.contains(*h))
            .copied()
            .collect();
        out.sort();
        out.dedup();
        Ok(out)
    }
}
