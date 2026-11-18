//! Recomputing `game_stats` and `playtime_daily` from `sessions` (STATISTICS §7), the
//! start-up consistency check, and recomputing `counted` after `min_session_secs` changes.
//!
//! The aggregation mirrors the incremental SQL of `SessionRepo::close_session` exactly and
//! is order-independent (first played = earliest counted start, last played = latest counted
//! end, last launch = latest start; STATISTICS §2), so a rebuild reproduces the incremental
//! tables whatever order sessions were closed in (the `rebuild_equals_incremental` and
//! `out_of_order_closes_equal_a_rebuild` tests). An all-zero `game_stats` row and a missing
//! row mean the same ("never played"); [`normalized_stats`] compares them as equal.

use std::collections::BTreeMap;

use crate::clock::Timestamp;
use crate::ids::GameId;
use crate::model::{EndReason, GameKey, GameStats, Session, SessionKind, SessionState};
use crate::storage::{SessionRepo, StatsRepo, StorageResult};

use super::recorder::{CountRule, is_counted};
use super::rollup;

/// Sessions read per page while scanning the table.
const PAGE: u64 = 1000;

/// Options of [`rebuild`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RebuildOptions {
    /// Also regenerate the kept history of removed games (`removed:<name>` rows) from their
    /// sessions' name snapshots. Off: those rows are preserved as they are.
    pub include_removed: bool,
}

/// What a rebuild did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RebuildReport {
    /// Counted sessions applied.
    pub counted_sessions: u64,
    /// `game_stats` rows written.
    pub games: u64,
    /// `playtime_daily` rows written.
    pub days: u64,
}

/// Every session, oldest first.
pub fn all_sessions<S: SessionRepo + ?Sized>(store: &mut S) -> StorageResult<Vec<Session>> {
    let mut out = Vec::new();
    let mut offset = 0;
    loop {
        let page = store.session_history(None, &[], offset, PAGE)?;
        let n = page.len() as u64;
        out.extend(page);
        if n < PAGE {
            break;
        }
        offset += n;
    }
    out.sort_by_key(|s| (s.started_at, s.id));
    Ok(out)
}

/// The aggregates implied by `sessions` (any order; ended rows only).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Aggregates {
    /// `game_stats` rows.
    pub games: BTreeMap<GameId, GameStats>,
    /// `playtime_daily` rows: `(ms, sessions)`.
    pub days: BTreeMap<(GameKey, u32), (u64, u32)>,
}

/// Aggregate ended sessions as the incremental path would (the result does not depend on
/// the order of `sessions`).
pub fn aggregate(sessions: &[Session], include_removed: bool) -> Aggregates {
    let mut ended: Vec<&Session> = sessions
        .iter()
        .filter(|s| s.state == SessionState::Ended)
        .collect();
    ended.sort_by_key(|s| (s.started_at, s.id));
    let mut agg = Aggregates::default();
    for s in ended {
        let d = s.duration_ms.unwrap_or(0);
        let started = s.started_at;
        match s.game_id {
            Some(g) => {
                if s.counted {
                    let st = agg.games.entry(g).or_insert_with(|| GameStats::empty(g));
                    st.total_ms += d;
                    st.sessions += 1;
                    let ended = s.ended_at.unwrap_or(started);
                    st.first_played_at =
                        Some(st.first_played_at.map_or(started, |f| f.min(started)));
                    st.last_played_at =
                        Some(st.last_played_at.unwrap_or(Timestamp::EPOCH).max(ended));
                    st.longest_ms = st.longest_ms.max(d);
                    st.last_launch_at = Some(st.last_launch_at.map_or(started, |l| l.max(started)));
                    add_days(&mut agg, GameKey::Game(g), s, d);
                } else if s.kind == SessionKind::Game {
                    let st = agg.games.entry(g).or_insert_with(|| GameStats::empty(g));
                    st.failed_launches += 1;
                    st.last_launch_at = Some(st.last_launch_at.map_or(started, |l| l.max(started)));
                }
            }
            None if include_removed && s.counted => {
                add_days(&mut agg, GameKey::Removed(s.game_name.clone()), s, d);
            }
            None => {}
        }
    }
    agg
}

fn add_days(agg: &mut Aggregates, key: GameKey, s: &Session, d: u64) {
    for p in rollup::split(s.started_at, d, s.tz_offset_min) {
        let e = agg.days.entry((key.clone(), p.day)).or_insert((0, 0));
        e.0 += p.ms;
        e.1 += u32::from(p.starts_here);
    }
}

/// `game_stats` rows with all-zero rows dropped, keyed by game (for comparisons).
pub fn normalized_stats(rows: Vec<GameStats>) -> BTreeMap<GameId, GameStats> {
    rows.into_iter()
        .filter(|s| *s != GameStats::empty(s.game_id))
        .map(|s| (s.game_id, s))
        .collect()
}

/// Recompute both aggregate tables from `sessions`. Run it inside a transaction
/// (`Db::transaction`) so readers never see the cleared state.
pub fn rebuild<S: SessionRepo + StatsRepo + ?Sized>(
    store: &mut S,
    opts: RebuildOptions,
) -> StorageResult<RebuildReport> {
    let sessions = all_sessions(store)?;
    let kept: Vec<_> = if opts.include_removed {
        Vec::new()
    } else {
        store
            .playtime_range(None, 0, u32::MAX)?
            .into_iter()
            .filter(|d| matches!(d.game_key, GameKey::Removed(_)))
            .collect()
    };
    let agg = aggregate(&sessions, opts.include_removed);
    store.clear_aggregates()?;
    let mut report = RebuildReport {
        counted_sessions: sessions
            .iter()
            .filter(|s| s.state == SessionState::Ended && s.counted)
            .count() as u64,
        ..RebuildReport::default()
    };
    for st in agg.games.values() {
        store.put_game_stats(st)?;
        report.games += 1;
    }
    for ((key, day), (ms, n)) in &agg.days {
        store.add_playtime(key, *day, *ms, *n)?;
        report.days += 1;
    }
    for d in kept {
        store.add_playtime(&d.game_key, d.day, d.ms, d.sessions)?;
        report.days += 1;
    }
    Ok(report)
}

/// The start-up consistency check: `Σ game_stats.total_ms` equals the sum of counted
/// durations of sessions that still belong to a game.
pub fn is_consistent<S: SessionRepo + StatsRepo + ?Sized>(store: &mut S) -> StorageResult<bool> {
    let stats: u64 = store.all_game_stats()?.iter().map(|s| s.total_ms).sum();
    let sessions: u64 = all_sessions(store)?
        .iter()
        .filter(|s| s.state == SessionState::Ended && s.counted && s.game_id.is_some())
        .map(|s| s.duration_ms.unwrap_or(0))
        .sum();
    Ok(stats == sessions)
}

/// Run the consistency check and rebuild on mismatch; returns whether a rebuild ran.
pub fn check_and_repair<S: SessionRepo + StatsRepo + ?Sized>(store: &mut S) -> StorageResult<bool> {
    if is_consistent(store)? {
        return Ok(false);
    }
    tracing::warn!("statistics aggregates inconsistent with sessions; rebuilding");
    rebuild(store, RebuildOptions::default())?;
    Ok(true)
}

/// Sessions whose `counted` flag changes when the minimum goes from `old` to `new`
/// (STATISTICS §7). Only the duration criterion is re-evaluated: a session that was not
/// counted for another reason (statistics disabled at the time) and already met the old
/// minimum stays uncounted.
pub fn recount_changes(sessions: &[Session], old: CountRule, new: CountRule) -> Vec<(usize, bool)> {
    let mut out = Vec::new();
    for (i, s) in sessions.iter().enumerate() {
        if s.state != SessionState::Ended {
            continue;
        }
        let (Some(d), Some(reason)) = (s.duration_ms, s.end_reason) else {
            continue;
        };
        let enabled = CountRule {
            enabled: true,
            ..new
        };
        let now = is_counted(s.kind, d, reason, enabled);
        let was_by_duration = is_counted(
            s.kind,
            d,
            reason,
            CountRule {
                enabled: true,
                ..old
            },
        );
        if s.counted && !now {
            out.push((i, false));
        } else if !s.counted
            && now
            && !was_by_duration
            && !matches!(reason, EndReason::FailedToStart { .. })
        {
            out.push((i, true));
        }
    }
    out
}

/// Apply [`recount_changes`] and rebuild. The `sessions` table has no column update for
/// `counted`, so a changed row is deleted and re-inserted with its events. Run inside a
/// transaction. Returns the number of sessions whose flag changed.
pub fn recount<S: SessionRepo + StatsRepo + ?Sized>(
    store: &mut S,
    old: CountRule,
    new: CountRule,
) -> StorageResult<usize> {
    let sessions = all_sessions(store)?;
    let changes = recount_changes(&sessions, old, new);
    for (i, counted) in &changes {
        let mut s = sessions[*i].clone();
        let events = store.session_events(s.id)?;
        store.delete_session(s.id, &[])?;
        s.counted = *counted;
        store.insert_session(&s)?;
        for e in events {
            store.add_session_event(s.id, e.at, &e.kind, &e.data_json)?;
        }
    }
    rebuild(store, RebuildOptions::default())?;
    Ok(changes.len())
}
