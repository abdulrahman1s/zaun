//! `steam_games` and `steam_game_tags` (ADR-0031 §4, DATA_MODEL §6): what EasyPlay itself
//! knows about a Steam game — when it first appeared, favourite, hidden, notes, tags. The row
//! is kept when the game is uninstalled in Steam, so all of it returns with the game.

use super::tags::MAX_TAGS_PER_GAME;
use super::{Arg, conn, exec_args, ts};
use crate::clock::Timestamp;
use crate::ids::TagId;
use crate::storage::{StorageError, StorageResult, Store};

/// One Steam game's row with its tags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteamOverlay {
    /// App ID.
    pub app_id: u32,
    /// When EasyPlay first saw it installed (the library's "date added").
    pub first_seen_at: Timestamp,
    /// Last change.
    pub updated_at: Timestamp,
    /// Favourite.
    pub favorite: bool,
    /// Hidden.
    pub hidden: bool,
    /// Notes.
    pub notes: String,
    /// Tags, ascending.
    pub tags: Vec<TagId>,
}

/// A change to a Steam game's row (`None` = unchanged).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SteamPatch {
    /// Favourite.
    pub favorite: Option<bool>,
    /// Hidden.
    pub hidden: Option<bool>,
    /// Notes (validated by the caller).
    pub notes: Option<String>,
}

/// The Steam overlay repository.
pub trait SteamRepo {
    /// Every row with its tags, by app ID.
    fn steam_overlays(&mut self) -> StorageResult<Vec<SteamOverlay>>;
    /// Create rows for apps seen for the first time (existing rows are untouched).
    fn steam_seen(&mut self, app_ids: &[u32], now: Timestamp) -> StorageResult<()>;
    /// Apply a patch (the row is created when missing).
    fn steam_update(
        &mut self,
        app_id: u32,
        patch: &SteamPatch,
        now: Timestamp,
    ) -> StorageResult<()>;
    /// Replace a game's tags (at most 16; the row is created when missing).
    fn steam_set_tags(&mut self, app_id: u32, tags: &[TagId], now: Timestamp) -> StorageResult<()>;
}

const INSERT_SEEN: &str = "INSERT INTO steam_games(app_id, first_seen_at, updated_at) \
                           VALUES (?, ?, ?) ON CONFLICT(app_id) DO NOTHING";

impl<S: Store + ?Sized> SteamRepo for S {
    fn steam_overlays(&mut self) -> StorageResult<Vec<SteamOverlay>> {
        conn(self).run(async move |c| {
            let rows: Vec<(i64, i64, i64, i64, i64, String)> = ormlite::query_as(
                "SELECT app_id, first_seen_at, updated_at, favorite, hidden, notes \
                 FROM steam_games ORDER BY app_id",
            )
            .fetch_all(&mut *c)
            .await?;
            let tags: Vec<(i64, i64)> = ormlite::query_as(
                "SELECT app_id, tag_id FROM steam_game_tags ORDER BY app_id, tag_id",
            )
            .fetch_all(&mut *c)
            .await?;
            Ok(rows
                .into_iter()
                .filter_map(|(id, seen, upd, fav, hid, notes)| {
                    let app_id = u32::try_from(id).ok()?;
                    Some(SteamOverlay {
                        app_id,
                        first_seen_at: ts(seen),
                        updated_at: ts(upd),
                        favorite: fav != 0,
                        hidden: hid != 0,
                        notes,
                        tags: tags
                            .iter()
                            .filter(|(a, _)| *a == id)
                            .map(|(_, t)| TagId::new(*t))
                            .collect(),
                    })
                })
                .collect())
        })
    }

    fn steam_seen(&mut self, app_ids: &[u32], now: Timestamp) -> StorageResult<()> {
        let ids = app_ids.to_vec();
        conn(self).atomic(async move |c| {
            for id in ids {
                exec_args(
                    c,
                    INSERT_SEEN,
                    &[
                        Arg::int(i64::from(id)),
                        Arg::int(now.as_millis()),
                        Arg::int(now.as_millis()),
                    ],
                )
                .await?;
            }
            Ok(())
        })
    }

    fn steam_update(
        &mut self,
        app_id: u32,
        patch: &SteamPatch,
        now: Timestamp,
    ) -> StorageResult<()> {
        let patch = patch.clone();
        conn(self).atomic(async move |c| {
            let id = Arg::int(i64::from(app_id));
            let at = Arg::int(now.as_millis());
            exec_args(c, INSERT_SEEN, &[id.clone(), at.clone(), at.clone()]).await?;
            let mut changes: Vec<(&'static str, Arg)> = Vec::new();
            if let Some(f) = patch.favorite {
                changes.push(("favorite", Arg::int(i64::from(f))));
            }
            if let Some(h) = patch.hidden {
                changes.push(("hidden", Arg::int(i64::from(h))));
            }
            if let Some(n) = patch.notes {
                changes.push(("notes", Arg::text(n)));
            }
            for (col, v) in changes {
                let sql =
                    format!("UPDATE steam_games SET {col} = ?, updated_at = ? WHERE app_id = ?");
                exec_args(c, &sql, &[v, at.clone(), id.clone()]).await?;
            }
            Ok(())
        })
    }

    fn steam_set_tags(&mut self, app_id: u32, tags: &[TagId], now: Timestamp) -> StorageResult<()> {
        let mut tags = tags.to_vec();
        tags.sort();
        tags.dedup();
        if tags.len() > MAX_TAGS_PER_GAME {
            return Err(StorageError::Limit {
                what: "tags per game",
                max: MAX_TAGS_PER_GAME,
            });
        }
        conn(self).atomic(async move |c| {
            let id = Arg::int(i64::from(app_id));
            let at = Arg::int(now.as_millis());
            exec_args(c, INSERT_SEEN, &[id.clone(), at.clone(), at.clone()]).await?;
            exec_args(
                c,
                "DELETE FROM steam_game_tags WHERE app_id = ?",
                std::slice::from_ref(&id),
            )
            .await?;
            for t in tags {
                exec_args(
                    c,
                    "INSERT INTO steam_game_tags(app_id, tag_id) VALUES (?, ?)",
                    &[id.clone(), Arg::int(t.get())],
                )
                .await?;
            }
            exec_args(
                c,
                "UPDATE steam_games SET updated_at = ? WHERE app_id = ?",
                &[at, id],
            )
            .await?;
            Ok(())
        })
    }
}
