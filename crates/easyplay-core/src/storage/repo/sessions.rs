//! `sessions` and `session_events`, including the guarded close of LOGIC_SPECS §7.

use ormlite::Model;

use super::{
    Arg, CloseOutcome, PlaytimePiece, SessionClose, SessionEvent, SessionRepo, conn, enum_parse,
    exec_args, id_parse, int, ms_opt, runtime_name, ts, ts_opt, uint,
};
use crate::clock::Timestamp;
use crate::ids::{GameId, SessionId};
use crate::model::{
    EndReason, GameKey, Preset, Session, SessionContext, SessionKind, SessionState, SupervisorIdent,
};
use crate::storage::db::{SqlConn, map_orm, map_sqlx};
use crate::storage::{StorageError, StorageResult, Store};

/// A `sessions` row.
#[derive(Model, Clone, Debug, PartialEq)]
#[ormlite(table = "sessions")]
pub(super) struct SessionRow {
    #[ormlite(primary_key)]
    pub id: String,
    pub game_id: Option<String>,
    pub game_name: String,
    pub executable_id: Option<String>,
    pub kind: String,
    pub state: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub duration_ms: Option<i64>,
    pub active_ms: i64,
    pub last_heartbeat_at: Option<i64>,
    pub end_reason: Option<String>,
    pub end_detail: Option<String>,
    pub exit_status: Option<i64>,
    pub counted: bool,
    pub supervisor_pid: Option<i64>,
    pub supervisor_start_ticks: Option<i64>,
    pub boot_id: Option<String>,
    pub sandbox_pid: Option<i64>,
    pub pid_ns: Option<i64>,
    pub preset: Option<String>,
    pub network: Option<String>,
    pub runtime_name: Option<String>,
    pub slr_variant: Option<String>,
    pub isolation: Option<String>,
    pub tz_offset_min: i64,
}

impl SessionRow {
    fn from_session(s: &Session) -> StorageResult<Self> {
        let enum_opt = |v: Option<String>| v;
        Ok(Self {
            id: s.id.to_string(),
            game_id: s.game_id.map(|g| g.to_string()),
            game_name: s.game_name.clone(),
            executable_id: s.executable_id.map(|e| e.to_string()),
            kind: s.kind.as_str().to_owned(),
            state: s.state.as_str().to_owned(),
            started_at: s.started_at.as_millis(),
            ended_at: ms_opt(s.ended_at),
            duration_ms: s.duration_ms.map(int),
            active_ms: int(s.active_ms),
            last_heartbeat_at: ms_opt(s.last_heartbeat_at),
            end_reason: s.end_reason.map(|r| r.as_str().to_owned()),
            end_detail: s.end_reason.and_then(EndReason::detail),
            exit_status: s.exit_status.map(i64::from),
            counted: s.counted,
            supervisor_pid: s.supervisor.as_ref().map(|v| i64::from(v.pid)),
            supervisor_start_ticks: s.supervisor.as_ref().map(|v| int(v.start_ticks)),
            boot_id: s.supervisor.as_ref().map(|v| v.boot_id.clone()),
            sandbox_pid: s.sandbox_pid.map(i64::from),
            pid_ns: s.pid_ns.map(int),
            preset: s.context.preset.map(Preset::as_column),
            network: enum_opt(
                s.context
                    .network
                    .map(|n| super::enum_text(&n))
                    .transpose()?,
            ),
            runtime_name: s.context.runtime_name.as_ref().map(|r| r.to_string()),
            slr_variant: s.context.slr_variant.map(|v| v.as_str().to_owned()),
            isolation: s.context.isolation.map(|v| v.as_str().to_owned()),
            tz_offset_min: i64::from(s.tz_offset_min),
        })
    }

    fn into_session(self) -> StorageResult<Session> {
        let bad = |col: &str, v: &str| StorageError::Decode(format!("sessions.{col} {v:?}"));
        let supervisor = match (
            self.supervisor_pid,
            self.supervisor_start_ticks,
            self.boot_id,
        ) {
            (Some(pid), Some(ticks), Some(boot_id)) => Some(SupervisorIdent {
                pid: u32::try_from(pid).map_err(|_| bad("supervisor_pid", &pid.to_string()))?,
                start_ticks: uint(ticks),
                boot_id,
            }),
            _ => None,
        };
        let end_reason = match &self.end_reason {
            Some(r) => Some(
                EndReason::from_columns(r, self.end_detail.as_deref())
                    .ok_or_else(|| bad("end_reason", r))?,
            ),
            None => None,
        };
        Ok(Session {
            id: id_parse("sessions.id", &self.id)?,
            game_id: self
                .game_id
                .as_deref()
                .map(|g| id_parse("sessions.game_id", g))
                .transpose()?,
            game_name: self.game_name,
            executable_id: self
                .executable_id
                .as_deref()
                .map(|e| id_parse("sessions.executable_id", e))
                .transpose()?,
            kind: SessionKind::parse(&self.kind).ok_or_else(|| bad("kind", &self.kind))?,
            state: SessionState::parse(&self.state).ok_or_else(|| bad("state", &self.state))?,
            started_at: ts(self.started_at),
            ended_at: ts_opt(self.ended_at),
            duration_ms: self.duration_ms.map(uint),
            active_ms: uint(self.active_ms),
            last_heartbeat_at: ts_opt(self.last_heartbeat_at),
            end_reason,
            exit_status: self.exit_status.and_then(|v| i32::try_from(v).ok()),
            counted: self.counted,
            supervisor,
            sandbox_pid: self.sandbox_pid.and_then(|v| u32::try_from(v).ok()),
            pid_ns: self.pid_ns.map(uint),
            context: SessionContext {
                preset: self
                    .preset
                    .as_deref()
                    .map(|p| Preset::from_column(p).ok_or_else(|| bad("preset", p)))
                    .transpose()?,
                network: self
                    .network
                    .as_deref()
                    .map(|n| enum_parse("sessions.network", n))
                    .transpose()?,
                runtime_name: self.runtime_name.as_deref().map(runtime_name).transpose()?,
                slr_variant: self
                    .slr_variant
                    .as_deref()
                    .map(|v| {
                        crate::model::SlrVariant::parse(v).ok_or_else(|| bad("slr_variant", v))
                    })
                    .transpose()?,
                isolation: self
                    .isolation
                    .as_deref()
                    .map(|v| enum_parse("sessions.isolation", v))
                    .transpose()?,
            },
            tz_offset_min: i16::try_from(self.tz_offset_min).unwrap_or(0),
        })
    }
}

async fn fetch_row(c: &mut SqlConn, id: SessionId) -> StorageResult<Option<SessionRow>> {
    SessionRow::select()
        .where_bind("id = ?", id.to_string())
        .fetch_optional(&mut *c)
        .await
        .map_err(map_orm)
}

fn rows_to_sessions(rows: Vec<SessionRow>) -> StorageResult<Vec<Session>> {
    rows.into_iter().map(SessionRow::into_session).collect()
}

/// Recompute `longest_ms`, `first_played_at`, `last_played_at` of a game from its counted
/// sessions (after a deletion).
async fn recompute_extremes(c: &mut SqlConn, game: &str) -> StorageResult<()> {
    exec_args(
        c,
        "UPDATE game_stats SET \
           longest_ms = coalesce((SELECT max(duration_ms) FROM sessions \
                                  WHERE game_id = ?1 AND counted = 1), 0), \
           first_played_at = (SELECT min(started_at) FROM sessions \
                              WHERE game_id = ?1 AND counted = 1), \
           last_played_at = (SELECT max(started_at) FROM sessions \
                             WHERE game_id = ?1 AND counted = 1) \
         WHERE game_id = ?1",
        &[Arg::text(game)],
    )
    .await
    .map(|_| ())
}

impl<S: Store + ?Sized> SessionRepo for S {
    fn insert_session(&mut self, session: &Session) -> StorageResult<()> {
        let row = SessionRow::from_session(session)?;
        conn(self).atomic(async move |c| {
            row.insert(&mut *c).await.map_err(map_orm)?;
            Ok(())
        })
    }

    fn session(&mut self, id: SessionId) -> StorageResult<Option<Session>> {
        conn(self).run(async move |c| {
            fetch_row(c, id)
                .await?
                .map(SessionRow::into_session)
                .transpose()
        })
    }

    fn set_session_state(&mut self, id: SessionId, state: SessionState) -> StorageResult<bool> {
        if state == SessionState::Ended {
            return Err(StorageError::Sql(
                "sessions are ended only through close_session".into(),
            ));
        }
        conn(self).atomic(async move |c| {
            exec_args(
                c,
                "UPDATE sessions SET state = ? WHERE id = ? AND state != 'ended'",
                &[Arg::text(state.as_str()), Arg::text(id.to_string())],
            )
            .await
            .map(|n| n > 0)
        })
    }

    fn heartbeat(&mut self, id: SessionId, active_ms: u64, at: Timestamp) -> StorageResult<bool> {
        conn(self).atomic(async move |c| {
            exec_args(
                c,
                "UPDATE sessions SET active_ms = ?, last_heartbeat_at = ? \
                 WHERE id = ? AND state != 'ended'",
                &[
                    Arg::int(int(active_ms)),
                    Arg::int(at.as_millis()),
                    Arg::text(id.to_string()),
                ],
            )
            .await
            .map(|n| n > 0)
        })
    }

    fn set_session_sandbox(
        &mut self,
        id: SessionId,
        sandbox_pid: Option<u32>,
        pid_ns: Option<u64>,
    ) -> StorageResult<bool> {
        conn(self).atomic(async move |c| {
            exec_args(
                c,
                "UPDATE sessions SET sandbox_pid = ?, pid_ns = ? WHERE id = ? AND state != 'ended'",
                &[
                    Arg::Int(sandbox_pid.map(i64::from)),
                    Arg::Int(pid_ns.map(int)),
                    Arg::text(id.to_string()),
                ],
            )
            .await
            .map(|n| n > 0)
        })
    }

    fn close_session(&mut self, close: &SessionClose) -> StorageResult<CloseOutcome> {
        let close = close.clone();
        conn(self).atomic(async move |c| {
            let id = close.id.to_string();
            let changed = exec_args(
                c,
                "UPDATE sessions SET state = 'ended', ended_at = ?, duration_ms = ?, \
                 end_reason = ?, end_detail = ?, exit_status = ?, counted = ? \
                 WHERE id = ? AND state != 'ended'",
                &[
                    Arg::int(close.ended_at.as_millis()),
                    Arg::int(int(close.duration_ms)),
                    Arg::text(close.end_reason.as_str()),
                    Arg::Text(close.end_reason.detail()),
                    Arg::Int(close.exit_status.map(i64::from)),
                    Arg::int(i64::from(close.counted)),
                    Arg::text(id.clone()),
                ],
            )
            .await?;
            if changed == 0 {
                return Ok(match fetch_row(c, close.id).await? {
                    Some(_) => CloseOutcome::AlreadyClosed,
                    None => CloseOutcome::NotFound,
                });
            }
            let row = fetch_row(c, close.id)
                .await?
                .ok_or_else(|| super::not_found("session", close.id))?;
            let Some(game) = row.game_id.clone() else {
                return Ok(CloseOutcome::Closed {
                    game_id: None,
                    counted: close.counted,
                });
            };
            let game_id: GameId = id_parse("sessions.game_id", &game)?;
            let started = row.started_at;
            if close.counted {
                exec_args(
                    c,
                    "INSERT INTO game_stats(game_id, total_ms, sessions, first_played_at, \
                       last_played_at, longest_ms, last_launch_at) VALUES (?, ?, 1, ?, ?, ?, ?) \
                     ON CONFLICT(game_id) DO UPDATE SET \
                       total_ms = total_ms + excluded.total_ms, \
                       sessions = sessions + 1, \
                       first_played_at = coalesce(first_played_at, excluded.first_played_at), \
                       last_played_at = max(coalesce(last_played_at, 0), excluded.last_played_at), \
                       longest_ms = max(longest_ms, excluded.longest_ms), \
                       last_launch_at = excluded.last_launch_at",
                    &[
                        Arg::text(game.clone()),
                        Arg::int(int(close.duration_ms)),
                        Arg::int(started),
                        Arg::int(started),
                        Arg::int(int(close.duration_ms)),
                        Arg::int(started),
                    ],
                )
                .await?;
                let key = GameKey::Game(game_id);
                for p in &close.pieces {
                    super::stats::add_playtime(c, &key, p.day, p.ms, u32::from(p.starts_here))
                        .await?;
                }
            } else if row.kind == SessionKind::Game.as_str() {
                exec_args(
                    c,
                    "INSERT INTO game_stats(game_id, failed_launches, last_launch_at) \
                     VALUES (?, 1, ?) ON CONFLICT(game_id) DO UPDATE SET \
                     failed_launches = failed_launches + 1, last_launch_at = excluded.last_launch_at",
                    &[Arg::text(game.clone()), Arg::int(started)],
                )
                .await?;
            }
            exec_args(
                c,
                "UPDATE games SET last_launched_at = max(coalesce(last_launched_at, 0), ?) \
                 WHERE id = ?",
                &[Arg::int(started), Arg::text(game)],
            )
            .await?;
            Ok(CloseOutcome::Closed {
                game_id: Some(game_id),
                counted: close.counted,
            })
        })
    }

    fn open_sessions(&mut self) -> StorageResult<Vec<Session>> {
        conn(self).run(async |c| {
            rows_to_sessions(
                SessionRow::query(
                    "SELECT * FROM sessions WHERE state != 'ended' ORDER BY started_at",
                )
                .fetch_all(&mut *c)
                .await
                .map_err(map_sqlx)?,
            )
        })
    }

    fn session_history(
        &mut self,
        game: Option<GameId>,
        kinds: &[SessionKind],
        offset: u64,
        limit: u64,
    ) -> StorageResult<Vec<Session>> {
        let kinds: Vec<&'static str> = if kinds.is_empty() {
            [
                SessionKind::Game,
                SessionKind::Installer,
                SessionKind::Tool,
                SessionKind::Prepare,
                SessionKind::Provision,
            ]
            .iter()
            .map(|k| k.as_str())
            .collect()
        } else {
            kinds.iter().map(|k| k.as_str()).collect()
        };
        // The kinds are fixed identifiers, so they can be inlined safely.
        let kind_list = kinds
            .iter()
            .map(|k| format!("'{k}'"))
            .collect::<Vec<_>>()
            .join(",");
        let (filter, game) = match game {
            Some(g) => ("game_id = ? AND ", Some(g.to_string())),
            None => ("", None),
        };
        let sql = format!(
            "SELECT * FROM sessions WHERE {filter}kind IN ({kind_list}) \
             ORDER BY started_at DESC, id DESC LIMIT ? OFFSET ?"
        );
        conn(self).run(async move |c| {
            let mut q = SessionRow::query(ormlite::AssertSqlSafe(sql));
            if let Some(g) = game {
                q = q.bind(g);
            }
            rows_to_sessions(
                q.bind(int(limit))
                    .bind(int(offset))
                    .fetch_all(&mut *c)
                    .await
                    .map_err(map_sqlx)?,
            )
        })
    }

    fn delete_session(&mut self, id: SessionId, pieces: &[PlaytimePiece]) -> StorageResult<bool> {
        let pieces = pieces.to_vec();
        conn(self).atomic(async move |c| {
            let Some(row) = fetch_row(c, id).await? else {
                return Ok(false);
            };
            exec_args(
                c,
                "DELETE FROM sessions WHERE id = ?",
                &[Arg::text(row.id.clone())],
            )
            .await?;
            if let (true, Some(game)) = (row.counted, row.game_id.as_deref()) {
                let d = row.duration_ms.unwrap_or(0);
                exec_args(
                    c,
                    "UPDATE game_stats SET total_ms = max(total_ms - ?, 0), \
                     sessions = max(sessions - 1, 0) WHERE game_id = ?",
                    &[Arg::int(d), Arg::text(game)],
                )
                .await?;
                recompute_extremes(c, game).await?;
                for p in pieces {
                    exec_args(
                        c,
                        "UPDATE playtime_daily SET ms = max(ms - ?, 0), \
                         sessions = max(sessions - ?, 0) WHERE game_key = ? AND day = ?",
                        &[
                            Arg::int(int(p.ms)),
                            Arg::int(i64::from(p.starts_here)),
                            Arg::text(game),
                            Arg::int(i64::from(p.day)),
                        ],
                    )
                    .await?;
                }
                exec_args(
                    c,
                    "DELETE FROM playtime_daily WHERE game_key = ? AND ms = 0 AND sessions = 0",
                    &[Arg::text(game)],
                )
                .await?;
            }
            Ok(true)
        })
    }

    fn add_session_event(
        &mut self,
        session: SessionId,
        at: Timestamp,
        kind: &str,
        data_json: &str,
    ) -> StorageResult<i64> {
        let args = [
            Arg::text(session.to_string()),
            Arg::int(at.as_millis()),
            Arg::text(kind),
            Arg::text(data_json),
        ];
        conn(self).atomic(async move |c| {
            exec_args(
                c,
                "INSERT INTO session_events(session_id, at, kind, data_json) VALUES (?, ?, ?, ?)",
                &args,
            )
            .await?;
            let (id,): (i64,) = ormlite::query_as("SELECT last_insert_rowid()")
                .fetch_one(&mut *c)
                .await
                .map_err(map_sqlx)?;
            Ok(id)
        })
    }

    fn session_events(&mut self, session: SessionId) -> StorageResult<Vec<SessionEvent>> {
        conn(self).run(async move |c| {
            let rows: Vec<(i64, String, i64, String, String)> = ormlite::query_as(
                "SELECT id, session_id, at, kind, data_json FROM session_events \
                 WHERE session_id = ? ORDER BY id",
            )
            .bind(session.to_string())
            .fetch_all(&mut *c)
            .await
            .map_err(map_sqlx)?;
            rows.into_iter()
                .map(|(id, sid, at, kind, data_json)| {
                    Ok(SessionEvent {
                        id,
                        session_id: id_parse("session_events.session_id", &sid)?,
                        at: ts(at),
                        kind,
                        data_json,
                    })
                })
                .collect()
        })
    }

    fn prune_session_events(&mut self, before: Timestamp) -> StorageResult<u64> {
        conn(self).atomic(async move |c| {
            exec_args(
                c,
                "DELETE FROM session_events WHERE at < ?",
                &[Arg::int(before.as_millis())],
            )
            .await
        })
    }
}
