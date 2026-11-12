//! Turn finished sessions into aggregates and answer statistics queries.
//!
//! Layer 5. See MODULES §6 and STATISTICS.
//!
//! | File | Content |
//! |---|---|
//! | [`recorder`] | the counted rule and the guarded close (§2, §4.2) |
//! | [`rollup`] | splitting sessions at local midnights; day keys (§4.3) |
//! | [`queries`] | [`StatsRequest`] → [`StatsResult`] (§3, §6) |
//! | [`rebuild`] | recomputing aggregates, consistency check, recount (§7) |
//!
//! History editing (§9) is here: [`delete_session`], [`reset_game`], [`clear_all`]. Each
//! takes a store that should be a transaction (`Db::transaction`), and leaves the
//! aggregates equal to what [`rebuild::rebuild`] would compute (all-zero `game_stats` rows
//! count as absent).

pub mod queries;
pub mod rebuild;
pub mod recorder;
pub mod rollup;

pub use queries::{
    Bucket, DateRange, GameOverview, HistoryPage, HistoryRow, LibraryOverview, QueryContext,
    Ranking, Series, StatsRequest, StatsResult, Totals,
};
pub use rebuild::{RebuildOptions, RebuildReport};
pub use recorder::{CountRule, SessionEnd, apply, is_counted};

use crate::ids::{GameId, SessionId};
use crate::model::{GameStats, SessionKind, SessionState};
use crate::storage::{SessionRepo, StatsRepo, StorageResult};

/// Recompute one game's `game_stats` row from its sessions (after an edit).
fn refresh_game_stats<S: SessionRepo + StatsRepo + ?Sized>(
    store: &mut S,
    game: GameId,
) -> StorageResult<()> {
    let mut sessions = Vec::new();
    let mut offset = 0;
    loop {
        let page = store.session_history(Some(game), &[SessionKind::Game], offset, 1000)?;
        let n = page.len() as u64;
        sessions.extend(page);
        if n < 1000 {
            break;
        }
        offset += n;
    }
    let agg = rebuild::aggregate(&sessions, false);
    let stats = agg
        .games
        .get(&game)
        .cloned()
        .unwrap_or_else(|| GameStats::empty(game));
    store.put_game_stats(&stats)
}

/// "Remove from history": delete one ended session and adjust the aggregates.
/// Returns `false` for unknown or still-open sessions.
pub fn delete_session<S: SessionRepo + StatsRepo + ?Sized>(
    store: &mut S,
    id: SessionId,
) -> StorageResult<bool> {
    let Some(s) = store.session(id)? else {
        return Ok(false);
    };
    if s.state != SessionState::Ended {
        return Ok(false);
    }
    let pieces = if s.counted {
        rollup::split(s.started_at, s.duration_ms.unwrap_or(0), s.tz_offset_min)
    } else {
        Vec::new()
    };
    if !store.delete_session(id, &pieces)? {
        return Ok(false);
    }
    if let Some(g) = s.game_id {
        refresh_game_stats(store, g)?;
    }
    Ok(true)
}

/// "Reset statistics for this game": delete its ended sessions and aggregates. Open
/// sessions (a running game) are kept. Returns the number of sessions deleted.
pub fn reset_game<S: SessionRepo + StatsRepo + ?Sized>(
    store: &mut S,
    game: GameId,
) -> StorageResult<u64> {
    let mut ids = Vec::new();
    let mut offset = 0;
    loop {
        let page = store.session_history(Some(game), &[], offset, 1000)?;
        let n = page.len() as u64;
        ids.extend(
            page.into_iter()
                .filter(|s| s.state == SessionState::Ended)
                .map(|s| s.id),
        );
        if n < 1000 {
            break;
        }
        offset += n;
    }
    let mut deleted = 0;
    for id in ids {
        if delete_session(store, id)? {
            deleted += 1;
        }
    }
    store.put_game_stats(&GameStats::empty(game))?;
    Ok(deleted)
}

/// "Clear all statistics": delete every ended session (with its events) and every
/// aggregate, including kept history of removed games. Returns the sessions deleted.
pub fn clear_all<S: SessionRepo + StatsRepo + ?Sized>(store: &mut S) -> StorageResult<u64> {
    let sessions = rebuild::all_sessions(store)?;
    let mut deleted = 0;
    for s in sessions.iter().filter(|s| s.state == SessionState::Ended) {
        if store.delete_session(s.id, &[])? {
            deleted += 1;
        }
    }
    store.clear_aggregates()?;
    Ok(deleted)
}
