//! Sessions closed out of start order: the incremental close (`SessionRepo::close_session`)
//! and `stats::rebuild` agree, and first/last played follow STATISTICS §2 (earliest counted
//! start, latest counted end). Also the grouped `StatsRepo` queries used by `stats::queries`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::time::Duration;

use proptest::prelude::*;
use zaun_core::clock::Timestamp;
use zaun_core::ids::{GameId, SessionId};
use zaun_core::model::{EndReason, Game, GameKey, GameStats, Session, SessionKind};
use zaun_core::stats::rebuild::{self, RebuildOptions, normalized_stats};
use zaun_core::stats::{self, CountRule, SessionEnd};
use zaun_core::storage::{Db, GameRepo, SessionRepo, StatsRepo};
use zaun_testkit::FakeClock;
use zaun_testkit::fixtures::games::{self, BASE_MS};

const HOUR: i64 = 3_600_000;
const OK: EndReason = EndReason::Exited { code: 0 };

type Snapshot = (BTreeMap<GameId, GameStats>, Vec<(String, u32, u64, u32)>);

fn add_games(db: &mut Db, n: u32) -> Vec<Game> {
    (0..n)
        .map(|i| {
            let g = games::game(i, 1);
            db.insert_game(&g).unwrap();
            g
        })
        .collect()
}

fn start(db: &mut Db, g: &Game, k: u32, at: i64, kind: SessionKind) -> Session {
    let mut s = games::session(g, 0, k, Timestamp::from_millis(at));
    s.id = SessionId::generate(&FakeClock::at_millis(at));
    s.kind = kind;
    db.insert_session(&s).unwrap();
    s
}

fn end(s: &Session, d: u64, reason: EndReason) -> SessionEnd {
    SessionEnd {
        id: s.id,
        ended_at: s.started_at.saturating_add(Duration::from_millis(d)),
        duration_ms: d,
        end_reason: reason,
        exit_status: Some(0),
    }
}

fn snapshot(db: &mut Db) -> Snapshot {
    let stats = normalized_stats(db.all_game_stats().unwrap());
    let days = db
        .playtime_range(None, 0, u32::MAX)
        .unwrap()
        .into_iter()
        .filter(|d| d.ms > 0 || d.sessions > 0)
        .map(|d| (d.game_key.as_column(), d.day, d.ms, d.sessions))
        .collect();
    (stats, days)
}

fn rebuilt(db: &mut Db) -> Snapshot {
    db.transaction(|tx| rebuild::rebuild(tx, RebuildOptions::default()))
        .unwrap();
    snapshot(db)
}

#[test]
fn out_of_order_closes_follow_the_definitions() {
    let mut db = Db::open_in_memory().unwrap();
    let g = add_games(&mut db, 1).remove(0);
    // A starts first and runs five hours; B starts an hour later and runs ten minutes.
    let a = start(&mut db, &g, 1, BASE_MS, SessionKind::Game);
    let b = start(&mut db, &g, 2, BASE_MS + HOUR, SessionKind::Game);
    // B is closed first, A last (the order a supervisor and recovery can produce).
    stats::apply(&mut db, &end(&b, 600_000, OK), CountRule::default()).unwrap();
    stats::apply(&mut db, &end(&a, 5 * HOUR as u64, OK), CountRule::default()).unwrap();

    let st = db.game_stats(g.id).unwrap().unwrap();
    assert_eq!(
        st.first_played_at,
        Some(a.started_at),
        "earliest counted start"
    );
    assert_eq!(
        st.last_played_at,
        Some(Timestamp::from_millis(BASE_MS + 5 * HOUR)),
        "latest counted end, not the latest start"
    );
    assert_eq!(st.last_launch_at, Some(b.started_at), "latest launch start");
    assert_eq!(st.sessions, 2);

    let incremental = snapshot(&mut db);
    assert_eq!(rebuilt(&mut db), incremental);

    // Deleting the long session recomputes last played from the remaining end.
    assert!(
        db.transaction(|tx| stats::delete_session(tx, a.id))
            .unwrap()
    );
    let st = db.game_stats(g.id).unwrap().unwrap();
    assert_eq!(st.first_played_at, Some(b.started_at));
    assert_eq!(
        st.last_played_at,
        Some(Timestamp::from_millis(BASE_MS + HOUR + 600_000))
    );
    let edited = snapshot(&mut db);
    assert_eq!(rebuilt(&mut db), edited);
}

#[test]
fn grouped_queries() {
    let mut db = Db::open_in_memory().unwrap();
    let key = |n| GameKey::Game(games::game_id(n));
    let removed = GameKey::Removed("Old Game".into());
    db.add_playtime(&key(0), 20261101, 1000, 1).unwrap();
    db.add_playtime(&key(0), 20261102, 500, 1).unwrap();
    db.add_playtime(&key(1), 20261102, 0, 0).unwrap();
    db.add_playtime(&removed, 20261105, 700, 1).unwrap();
    let by_key: BTreeMap<_, _> = db
        .playtime_by_key(20261101, 20261131)
        .unwrap()
        .into_iter()
        .collect();
    assert_eq!(by_key.get(&key(0)), Some(&1500));
    assert_eq!(by_key.get(&removed), Some(&700));
    assert!(!by_key.contains_key(&key(1)), "zero sums are left out");
    assert_eq!(
        db.playtime_by_key(20261102, 20261102).unwrap(),
        vec![(key(0), 500)]
    );
    assert_eq!(
        db.played_days(0, u32::MAX).unwrap(),
        vec![20261101, 20261102, 20261105]
    );
    assert_eq!(
        db.played_days(20261103, 20261104).unwrap(),
        Vec::<u32>::new()
    );
}

/// (game, start offset in hours, duration in minutes, kind, failed).
type Gen = (usize, i64, u64, bool, bool);

fn arb() -> impl Strategy<Value = (Vec<Gen>, Vec<usize>)> {
    prop::collection::vec(
        (0usize..3, 0i64..96, 0u64..600, any::<bool>(), any::<bool>()),
        1..20,
    )
    .prop_flat_map(|v| {
        let n = v.len();
        (Just(v), Just((0..n).collect::<Vec<_>>()).prop_shuffle())
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn out_of_order_closes_equal_a_rebuild((stream, order) in arb()) {
        let mut db = Db::open_in_memory().unwrap();
        let gs = add_games(&mut db, 3);
        let mut open = Vec::new();
        for (i, (gi, h, m, tool, failed)) in stream.iter().enumerate() {
            let kind = if *tool { SessionKind::Tool } else { SessionKind::Game };
            let at = BASE_MS + h * HOUR + i as i64;
            let s = start(&mut db, &gs[*gi], i as u32, at, kind);
            let reason = if *failed { EndReason::Crashed { signal: 11 } } else { OK };
            open.push((s, m * 60_000 + u64::from(*failed) * 1000, reason));
        }
        for i in order {
            let (s, d, reason) = &open[i];
            stats::apply(&mut db, &end(s, *d, *reason), CountRule::default()).unwrap();
        }
        let incremental = snapshot(&mut db);
        prop_assert!(rebuild::is_consistent(&mut db).unwrap());
        prop_assert_eq!(rebuilt(&mut db), incremental);
    }
}
