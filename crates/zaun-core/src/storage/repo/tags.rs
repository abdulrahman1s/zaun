//! `tags` and `game_tags` (GAME_LIBRARY §7, limits §13).

use ormlite::Model;

use super::{Arg, TagRepo, conn, exec_args, id_parse, not_found, tag_color, tag_name, ts, uint32};
use crate::clock::Timestamp;
use crate::ids::{GameId, TagId, TagName};
use crate::model::{Tag, TagColor};
use crate::storage::db::SqlConn;
use crate::storage::{StorageError, StorageResult, Store};

/// Maximum number of tags in the library (GAME_LIBRARY §13).
// Candidate for `consts` (outside this task's files).
pub const MAX_TAGS: usize = 200;
/// Maximum number of tags on one game (GAME_LIBRARY §13).
pub const MAX_TAGS_PER_GAME: usize = 16;

/// A `tags` row.
#[derive(Model, Clone, Debug, PartialEq)]
#[ormlite(table = "tags")]
pub(super) struct TagRow {
    #[ormlite(primary_key)]
    pub id: i64,
    pub name: String,
    pub color: i64,
    pub created_at: i64,
}

impl TagRow {
    fn into_tag(self) -> StorageResult<Tag> {
        Ok(Tag {
            id: TagId::new(self.id),
            name: tag_name(&self.name)?,
            color: tag_color(self.color)?,
            created_at: ts(self.created_at),
        })
    }
}

pub(super) async fn fetch_all(c: &mut SqlConn) -> StorageResult<Vec<Tag>> {
    TagRow::query("SELECT * FROM tags ORDER BY name COLLATE NOCASE, id")
        .fetch_all(&mut *c)
        .await?
        .into_iter()
        .map(TagRow::into_tag)
        .collect()
}

pub(super) async fn fetch_links(c: &mut SqlConn) -> StorageResult<Vec<(GameId, TagId)>> {
    let rows: Vec<(String, i64)> =
        ormlite::query_as("SELECT game_id, tag_id FROM game_tags ORDER BY game_id, tag_id")
            .fetch_all(&mut *c)
            .await?;
    rows.into_iter()
        .map(|(g, t)| Ok((id_parse("game_tags.game_id", &g)?, TagId::new(t))))
        .collect()
}

/// The tag whose name equals `name` case-insensitively (full Unicode folding, which
/// `COLLATE NOCASE` lacks), other than `except`.
async fn find_by_key(
    c: &mut SqlConn,
    name: &TagName,
    except: Option<TagId>,
) -> StorageResult<Option<Tag>> {
    Ok(fetch_all(c)
        .await?
        .into_iter()
        .find(|t| t.name == *name && Some(t.id) != except))
}

async fn tag_exists(c: &mut SqlConn, id: TagId) -> StorageResult<bool> {
    let (n,): (i64,) = ormlite::query_as("SELECT count(*) FROM tags WHERE id = ?")
        .bind(id.get())
        .fetch_one(&mut *c)
        .await?;
    Ok(n > 0)
}

async fn tag_count_of(c: &mut SqlConn, game: GameId) -> StorageResult<usize> {
    let (n,): (i64,) = ormlite::query_as("SELECT count(*) FROM game_tags WHERE game_id = ?")
        .bind(game.to_string())
        .fetch_one(&mut *c)
        .await?;
    Ok(usize::try_from(n).unwrap_or(usize::MAX))
}

impl<S: Store + ?Sized> TagRepo for S {
    fn create_tag(
        &mut self,
        name: &TagName,
        color: TagColor,
        now: Timestamp,
    ) -> StorageResult<Tag> {
        let name = name.clone();
        conn(self).atomic(async move |c| {
            if find_by_key(c, &name, None).await?.is_some() {
                return Err(StorageError::DuplicateTag);
            }
            let (n,): (i64,) = ormlite::query_as("SELECT count(*) FROM tags")
                .fetch_one(&mut *c)
                .await?;
            if usize::try_from(n).unwrap_or(usize::MAX) >= MAX_TAGS {
                return Err(StorageError::Limit {
                    what: "tags",
                    max: MAX_TAGS,
                });
            }
            let row: TagRow = TagRow::query(
                "INSERT INTO tags(name, color, created_at) VALUES (?, ?, ?) RETURNING *",
            )
            .bind(name.as_str().to_owned())
            .bind(i64::from(color.index()))
            .bind(now.as_millis())
            .fetch_one(&mut *c)
            .await?;
            row.into_tag()
        })
    }

    fn tag_by_name(&mut self, name: &TagName) -> StorageResult<Option<Tag>> {
        let name = name.clone();
        conn(self).run(async move |c| find_by_key(c, &name, None).await)
    }

    fn tags_with_counts(&mut self) -> StorageResult<Vec<(Tag, u32)>> {
        conn(self).snapshot(async |c| {
            let counts: Vec<(i64, i64)> = ormlite::query_as(
                "SELECT gt.tag_id, count(*) FROM game_tags gt JOIN games g ON g.id = gt.game_id \
                 WHERE g.deleted_at IS NULL GROUP BY gt.tag_id",
            )
            .fetch_all(&mut *c)
            .await?;
            let counts: std::collections::HashMap<i64, i64> = counts.into_iter().collect();
            Ok(fetch_all(c)
                .await?
                .into_iter()
                .map(|t| {
                    let n = counts.get(&t.id.get()).copied().unwrap_or(0);
                    (t, uint32(n))
                })
                .collect())
        })
    }

    fn rename_tag(&mut self, id: TagId, name: &TagName) -> StorageResult<()> {
        let name = name.clone();
        conn(self).atomic(async move |c| {
            if find_by_key(c, &name, Some(id)).await?.is_some() {
                return Err(StorageError::DuplicateTag);
            }
            let n = exec_args(
                c,
                "UPDATE tags SET name = ? WHERE id = ?",
                &[Arg::text(name.as_str()), Arg::int(id.get())],
            )
            .await?;
            if n == 0 {
                return Err(not_found("tag", id));
            }
            Ok(())
        })
    }

    fn set_tag_color(&mut self, id: TagId, color: TagColor) -> StorageResult<()> {
        conn(self).atomic(async move |c| {
            let n = exec_args(
                c,
                "UPDATE tags SET color = ? WHERE id = ?",
                &[Arg::int(i64::from(color.index())), Arg::int(id.get())],
            )
            .await?;
            if n == 0 {
                return Err(not_found("tag", id));
            }
            Ok(())
        })
    }

    fn merge_tags(&mut self, from: TagId, into: TagId) -> StorageResult<u32> {
        if from == into {
            return Ok(0);
        }
        conn(self).atomic(async move |c| {
            if !tag_exists(c, from).await? {
                return Err(not_found("tag", from));
            }
            if !tag_exists(c, into).await? {
                return Err(not_found("tag", into));
            }
            let moved = exec_args(
                c,
                "INSERT OR IGNORE INTO game_tags(game_id, tag_id) \
                 SELECT game_id, ? FROM game_tags WHERE tag_id = ?",
                &[Arg::int(into.get()), Arg::int(from.get())],
            )
            .await?;
            let affected = exec_args(
                c,
                "DELETE FROM game_tags WHERE tag_id = ?",
                &[Arg::int(from.get())],
            )
            .await?;
            exec_args(c, "DELETE FROM tags WHERE id = ?", &[Arg::int(from.get())]).await?;
            let _ = moved;
            Ok(u32::try_from(affected).unwrap_or(u32::MAX))
        })
    }

    fn delete_tag(&mut self, id: TagId) -> StorageResult<u32> {
        conn(self).atomic(async move |c| {
            let affected = exec_args(
                c,
                "DELETE FROM game_tags WHERE tag_id = ?",
                &[Arg::int(id.get())],
            )
            .await?;
            let n = exec_args(c, "DELETE FROM tags WHERE id = ?", &[Arg::int(id.get())]).await?;
            if n == 0 {
                return Err(not_found("tag", id));
            }
            Ok(u32::try_from(affected).unwrap_or(u32::MAX))
        })
    }

    fn tags_of(&mut self, game: GameId) -> StorageResult<Vec<TagId>> {
        conn(self).run(async move |c| {
            let rows: Vec<(i64,)> =
                ormlite::query_as("SELECT tag_id FROM game_tags WHERE game_id = ? ORDER BY tag_id")
                    .bind(game.to_string())
                    .fetch_all(&mut *c)
                    .await?;
            Ok(rows.into_iter().map(|(t,)| TagId::new(t)).collect())
        })
    }

    fn set_game_tags(&mut self, game: GameId, tags: &[TagId]) -> StorageResult<()> {
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
            let g = game.to_string();
            exec_args(
                c,
                "DELETE FROM game_tags WHERE game_id = ?",
                &[Arg::text(g.clone())],
            )
            .await?;
            for t in tags {
                exec_args(
                    c,
                    "INSERT INTO game_tags(game_id, tag_id) VALUES (?, ?)",
                    &[Arg::text(g.clone()), Arg::int(t.get())],
                )
                .await?;
            }
            Ok(())
        })
    }

    fn add_tag_to_games(&mut self, tag: TagId, games: &[GameId]) -> StorageResult<Vec<GameId>> {
        let games = games.to_vec();
        conn(self).atomic(async move |c| {
            if !tag_exists(c, tag).await? {
                return Err(not_found("tag", tag));
            }
            let mut skipped = Vec::new();
            for g in games {
                let (has,): (i64,) = ormlite::query_as(
                    "SELECT count(*) FROM game_tags WHERE game_id = ? AND tag_id = ?",
                )
                .bind(g.to_string())
                .bind(tag.get())
                .fetch_one(&mut *c)
                .await?;
                if has > 0 {
                    continue;
                }
                if tag_count_of(c, g).await? >= MAX_TAGS_PER_GAME {
                    skipped.push(g);
                    continue;
                }
                exec_args(
                    c,
                    "INSERT INTO game_tags(game_id, tag_id) VALUES (?, ?)",
                    &[Arg::text(g.to_string()), Arg::int(tag.get())],
                )
                .await?;
            }
            Ok(skipped)
        })
    }
}
