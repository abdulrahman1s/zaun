//! `game_stats` and `playtime_daily` (STATISTICS §4).

use ormlite::Model;

use super::{
    Arg, DailyPlaytime, StatsRepo, conn, exec_args, id_parse, int, ms_opt, ts_opt, uint, uint32,
};
use crate::ids::GameId;
use crate::model::{GameKey, GameStats, PlaytimeDay};
use crate::storage::db::{SqlConn, map_orm, map_sqlx};
use crate::storage::{StorageError, StorageResult, Store};

/// A `game_stats` row.
#[derive(Model, Clone, Debug, PartialEq)]
#[ormlite(table = "game_stats")]
pub(super) struct StatsRow {
    #[ormlite(primary_key)]
    pub game_id: String,
    pub total_ms: i64,
    pub sessions: i64,
    pub failed_launches: i64,
    pub first_played_at: Option<i64>,
    pub last_played_at: Option<i64>,
    pub longest_ms: i64,
    pub last_launch_at: Option<i64>,
}

impl StatsRow {
    fn from_stats(s: &GameStats) -> Self {
        Self {
            game_id: s.game_id.to_string(),
            total_ms: int(s.total_ms),
            sessions: i64::from(s.sessions),
            failed_launches: i64::from(s.failed_launches),
            first_played_at: ms_opt(s.first_played_at),
            last_played_at: ms_opt(s.last_played_at),
            longest_ms: int(s.longest_ms),
            last_launch_at: ms_opt(s.last_launch_at),
        }
    }

    fn into_stats(self) -> StorageResult<GameStats> {
        Ok(GameStats {
            game_id: id_parse("game_stats.game_id", &self.game_id)?,
            total_ms: uint(self.total_ms),
            sessions: uint32(self.sessions),
            failed_launches: uint32(self.failed_launches),
            first_played_at: ts_opt(self.first_played_at),
            last_played_at: ts_opt(self.last_played_at),
            longest_ms: uint(self.longest_ms),
            last_launch_at: ts_opt(self.last_launch_at),
        })
    }
}

#[derive(ormlite::FromRow)]
struct DayRow {
    game_key: String,
    day: i64,
    ms: i64,
    sessions: i64,
}

impl DayRow {
    fn into_day(self) -> StorageResult<PlaytimeDay> {
        Ok(PlaytimeDay {
            game_key: GameKey::from_column(&self.game_key).ok_or_else(|| {
                StorageError::Decode(format!("playtime_daily.game_key {:?}", self.game_key))
            })?,
            day: uint32(self.day),
            ms: uint(self.ms),
            sessions: uint32(self.sessions),
        })
    }
}

pub(super) async fn fetch_all(c: &mut SqlConn) -> StorageResult<Vec<GameStats>> {
    StatsRow::query("SELECT * FROM game_stats ORDER BY game_id")
        .fetch_all(&mut *c)
        .await
        .map_err(map_sqlx)?
        .into_iter()
        .map(StatsRow::into_stats)
        .collect()
}

pub(super) async fn add_playtime(
    c: &mut SqlConn,
    key: &GameKey,
    day: u32,
    ms: u64,
    sessions: u32,
) -> StorageResult<()> {
    exec_args(
        c,
        "INSERT INTO playtime_daily(game_key, day, ms, sessions) VALUES (?, ?, ?, ?) \
         ON CONFLICT(game_key, day) DO UPDATE SET ms = ms + excluded.ms, \
         sessions = sessions + excluded.sessions",
        &[
            Arg::text(key.as_column()),
            Arg::int(i64::from(day)),
            Arg::int(int(ms)),
            Arg::int(i64::from(sessions)),
        ],
    )
    .await
    .map(|_| ())
}

impl<S: Store + ?Sized> StatsRepo for S {
    fn game_stats(&mut self, game: GameId) -> StorageResult<Option<GameStats>> {
        conn(self).run(async move |c| {
            StatsRow::select()
                .where_bind("game_id = ?", game.to_string())
                .fetch_optional(&mut *c)
                .await
                .map_err(map_orm)?
                .map(StatsRow::into_stats)
                .transpose()
        })
    }

    fn all_game_stats(&mut self) -> StorageResult<Vec<GameStats>> {
        conn(self).run(async |c| fetch_all(c).await)
    }

    fn put_game_stats(&mut self, stats: &GameStats) -> StorageResult<()> {
        let row = StatsRow::from_stats(stats);
        conn(self).atomic(async move |c| {
            exec_args(
                c,
                "DELETE FROM game_stats WHERE game_id = ?",
                &[Arg::text(row.game_id.clone())],
            )
            .await?;
            row.insert(&mut *c).await.map_err(map_orm)?;
            Ok(())
        })
    }

    fn add_playtime(
        &mut self,
        key: &GameKey,
        day: u32,
        ms: u64,
        sessions: u32,
    ) -> StorageResult<()> {
        let key = key.clone();
        conn(self).atomic(async move |c| add_playtime(c, &key, day, ms, sessions).await)
    }

    fn playtime_range(
        &mut self,
        key: Option<&GameKey>,
        from: u32,
        to: u32,
    ) -> StorageResult<Vec<PlaytimeDay>> {
        let key = key.map(GameKey::as_column);
        conn(self).run(async move |c| {
            let rows: Vec<DayRow> = match key {
                Some(k) => {
                    ormlite::query_as(
                        "SELECT * FROM playtime_daily WHERE game_key = ? AND day BETWEEN ? AND ? \
                     ORDER BY day",
                    )
                    .bind(k)
                    .bind(i64::from(from))
                    .bind(i64::from(to))
                    .fetch_all(&mut *c)
                    .await
                }
                None => ormlite::query_as(
                    "SELECT * FROM playtime_daily WHERE day BETWEEN ? AND ? ORDER BY day, game_key",
                )
                .bind(i64::from(from))
                .bind(i64::from(to))
                .fetch_all(&mut *c)
                .await,
            }
            .map_err(map_sqlx)?;
            rows.into_iter().map(DayRow::into_day).collect()
        })
    }

    fn daily_totals(&mut self, from: u32, to: u32) -> StorageResult<Vec<DailyPlaytime>> {
        conn(self).run(async move |c| {
            let rows: Vec<(i64, i64, i64)> = ormlite::query_as(
                "SELECT day, sum(ms), sum(sessions) FROM playtime_daily WHERE day BETWEEN ? AND ? \
                 GROUP BY day ORDER BY day",
            )
            .bind(i64::from(from))
            .bind(i64::from(to))
            .fetch_all(&mut *c)
            .await
            .map_err(map_sqlx)?;
            Ok(rows
                .into_iter()
                .map(|(day, ms, sessions)| DailyPlaytime {
                    day: uint32(day),
                    ms: uint(ms),
                    sessions: uint32(sessions),
                })
                .collect())
        })
    }

    fn playtime_sum(&mut self, key: Option<&GameKey>, from: u32, to: u32) -> StorageResult<u64> {
        let key = key.map(GameKey::as_column);
        conn(self).run(async move |c| {
            let (sum,): (i64,) =
                match key {
                    Some(k) => {
                        ormlite::query_as(
                            "SELECT coalesce(sum(ms), 0) FROM playtime_daily \
                     WHERE game_key = ? AND day BETWEEN ? AND ?",
                        )
                        .bind(k)
                        .bind(i64::from(from))
                        .bind(i64::from(to))
                        .fetch_one(&mut *c)
                        .await
                    }
                    None => ormlite::query_as(
                        "SELECT coalesce(sum(ms), 0) FROM playtime_daily WHERE day BETWEEN ? AND ?",
                    )
                    .bind(i64::from(from))
                    .bind(i64::from(to))
                    .fetch_one(&mut *c)
                    .await,
                }
                .map_err(map_sqlx)?;
            Ok(uint(sum))
        })
    }

    fn clear_aggregates(&mut self) -> StorageResult<()> {
        conn(self).atomic(async |c| {
            exec_args(c, "DELETE FROM game_stats", &[]).await?;
            exec_args(c, "DELETE FROM playtime_daily", &[]).await?;
            Ok(())
        })
    }
}
