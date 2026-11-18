//! Statistics: recorder, rebuild, history editing and queries against SQLite in memory
//! (STATISTICS §10, STATS-001 … STATS-005).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::time::Duration;

use jiff::civil::Weekday;
use proptest::prelude::*;
use zaun_core::clock::{Clock, Timestamp};
use zaun_core::error::ErrorCode;
use zaun_core::ids::GameId;
use zaun_core::model::{EndReason, Game, GameKey, GameStats, Session, SessionKind};
use zaun_core::stats::rebuild::{self, RebuildOptions, normalized_stats};
use zaun_core::stats::rollup::{DAY_MS, day_key, key_date, key_days};
use zaun_core::stats::{
    self, Bucket, CountRule, DateRange, QueryContext, SessionEnd, StatsRequest, StatsResult,
    queries,
};
use zaun_core::storage::{CloseOutcome, Db, GameRepo, SessionRepo, StatsRepo};
use zaun_testkit::FakeClock;
use zaun_testkit::fixtures::games::{self, BASE_MS, Lcg};

fn mem() -> Db {
    Db::open_in_memory().unwrap()
}

fn add_games(db: &mut Db, n: u32) -> Vec<Game> {
    (0..n)
        .map(|i| {
            let g = games::game(i, 1);
            db.insert_game(&g).unwrap();
            g
        })
        .collect()
}

fn start(db: &mut Db, g: &Game, k: u32, at: i64, kind: SessionKind, tz: i16) -> Session {
    let mut s = games::session(g, 0, k, Timestamp::from_millis(at));
    s.id = zaun_core::ids::SessionId::generate(&FakeClock::at_millis(at));
    s.kind = kind;
    s.tz_offset_min = tz;
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

const OK: EndReason = EndReason::Exited { code: 0 };

type Snapshot = (BTreeMap<GameId, GameStats>, Vec<(String, u32, u64, u32)>);

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
fn apply_is_idempotent_and_counts() {
    let mut db = mem();
    let g = add_games(&mut db, 1).remove(0);
    let s = start(&mut db, &g, 0, BASE_MS, SessionKind::Game, 0);
    let e = end(&s, 3_600_000, OK);
    let rule = CountRule::default();
    let first = db.transaction(|tx| stats::apply(tx, &e, rule)).unwrap();
    assert_eq!(
        first,
        CloseOutcome::Closed {
            game_id: Some(g.id),
            counted: true
        }
    );
    let after_one = snapshot(&mut db);
    assert_eq!(
        stats::apply(&mut db, &e, rule).unwrap(),
        CloseOutcome::AlreadyClosed
    );
    // A racing closer that skips the read still hits the storage guard.
    let close = stats::recorder::close_for(&s, &e, rule);
    assert_eq!(
        db.close_session(&close).unwrap(),
        CloseOutcome::AlreadyClosed
    );
    assert_eq!(snapshot(&mut db), after_one);
    let st = db.game_stats(g.id).unwrap().unwrap();
    assert_eq!(
        (st.total_ms, st.sessions, st.longest_ms),
        (3_600_000, 1, 3_600_000)
    );
    assert_eq!(
        stats::apply(
            &mut db,
            &SessionEnd {
                id: games::session_id(99, 99),
                ..e
            },
            rule
        )
        .unwrap(),
        CloseOutcome::NotFound
    );

    // A short session and a failed start are recorded but not counted.
    let short = start(&mut db, &g, 1, BASE_MS + DAY_MS, SessionKind::Game, 0);
    stats::apply(&mut db, &end(&short, 1000, OK), rule).unwrap();
    let failed = start(&mut db, &g, 2, BASE_MS + 2 * DAY_MS, SessionKind::Game, 0);
    let ftf = EndReason::FailedToStart {
        code: ErrorCode::IntTaskFailed,
    };
    stats::apply(&mut db, &end(&failed, 60_000, ftf), rule).unwrap();
    let tool = start(&mut db, &g, 3, BASE_MS + 3 * DAY_MS, SessionKind::Tool, 0);
    stats::apply(&mut db, &end(&tool, 600_000, OK), rule).unwrap();
    let st = db.game_stats(g.id).unwrap().unwrap();
    assert_eq!(
        (st.total_ms, st.sessions, st.failed_launches),
        (3_600_000, 1, 2)
    );
    assert!(!db.session(tool.id).unwrap().unwrap().counted);
}

#[test]
fn recovery_uses_the_last_heartbeat_and_suspend_is_not_counted() {
    let mut db = mem();
    let g = add_games(&mut db, 1).remove(0);
    let clock = FakeClock::at_millis(BASE_MS);
    let s = start(&mut db, &g, 0, BASE_MS, SessionKind::Game, 0);
    let mono0 = clock.mono();
    // Ten minutes of play, then eight hours suspended (wall clock only).
    clock.advance(Duration::from_secs(600));
    clock.advance_wall(Duration::from_secs(8 * 3600));
    let d = clock.mono().saturating_duration_since(mono0).as_millis() as u64;
    db.heartbeat(s.id, d, clock.now()).unwrap();
    let rec = SessionEnd::recovered(&db.session(s.id).unwrap().unwrap());
    assert_eq!(rec.duration_ms, 600_000);
    assert_eq!(rec.ended_at, clock.now());
    assert_eq!(rec.end_reason, EndReason::Interrupted);
    stats::apply(&mut db, &rec, CountRule::default()).unwrap();
    assert_eq!(db.game_stats(g.id).unwrap().unwrap().total_ms, 600_000);
}

#[test]
fn sessions_split_at_local_midnight() {
    let mut db = mem();
    let g = add_games(&mut db, 1).remove(0);
    // 2026-11-12 23:00 local at UTC+2 is 21:00 UTC; play 2 h.
    let day0 = key_days(20_261_112) * DAY_MS;
    let at = day0 + 21 * 3_600_000;
    let s = start(&mut db, &g, 0, at, SessionKind::Game, 120);
    stats::apply(&mut db, &end(&s, 7_200_000, OK), CountRule::default()).unwrap();
    let rows = db
        .playtime_range(Some(&GameKey::Game(g.id)), 0, u32::MAX)
        .unwrap();
    let got: Vec<_> = rows.iter().map(|r| (r.day, r.ms, r.sessions)).collect();
    assert_eq!(
        got,
        vec![(20_261_112, 3_600_000, 1), (20_261_113, 3_600_000, 0)]
    );
}

/// One generated session: (game index, gap after previous start, duration, kind, reason, tz).
type Gen = (usize, i64, u64, u8, u8, i16);

fn arb_sessions() -> impl Strategy<Value = Vec<Gen>> {
    prop::collection::vec(
        (
            0usize..3,
            1i64..(3 * DAY_MS),
            0u64..(2 * DAY_MS as u64),
            0u8..4,
            0u8..5,
            -600i16..600,
        ),
        0..25,
    )
}

fn play(db: &mut Db, gs: &[Game], stream: &[Gen], rule: CountRule) {
    let mut t = BASE_MS;
    for (i, (gi, gap, d, kind, reason, tz)) in stream.iter().enumerate() {
        t += gap;
        let kind = [
            SessionKind::Game,
            SessionKind::Game,
            SessionKind::Tool,
            SessionKind::Installer,
        ][*kind as usize];
        let reason = [
            OK,
            EndReason::Crashed { signal: 11 },
            EndReason::Killed,
            EndReason::Interrupted,
            EndReason::FailedToStart {
                code: ErrorCode::IntTaskFailed,
            },
        ][*reason as usize];
        let s = start(db, &gs[*gi], i as u32, t, kind, *tz);
        stats::apply(db, &end(&s, *d, reason), rule).unwrap();
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn rebuild_equals_incremental(stream in arb_sessions()) {
        let mut db = mem();
        let gs = add_games(&mut db, 3);
        play(&mut db, &gs, &stream, CountRule::default());
        let incremental = snapshot(&mut db);
        prop_assert!(rebuild::is_consistent(&mut db).unwrap());
        prop_assert_eq!(rebuilt(&mut db), incremental);
    }

    #[test]
    fn edits_equal_a_rebuild(stream in arb_sessions(), pick in 0usize..25, which in 0u8..3) {
        let mut db = mem();
        let gs = add_games(&mut db, 3);
        play(&mut db, &gs, &stream, CountRule::default());
        let all = rebuild::all_sessions(&mut db).unwrap();
        match which {
            0 => {
                if let Some(s) = all.get(pick % all.len().max(1)) {
                    let id = s.id;
                    prop_assert!(db.transaction(|tx| stats::delete_session(tx, id)).unwrap());
                }
            }
            1 => {
                let g = gs[pick % 3].id;
                db.transaction(|tx| stats::reset_game(tx, g)).unwrap();
                prop_assert!(db.session_history(Some(g), &[], 0, 100).unwrap().is_empty());
            }
            _ => {
                db.transaction(|tx| stats::clear_all(tx)).unwrap();
                prop_assert!(snapshot(&mut db).0.is_empty());
            }
        }
        let edited = snapshot(&mut db);
        prop_assert_eq!(rebuilt(&mut db), edited);
    }
}

#[test]
fn consistency_check_repairs_drift() {
    let mut db = mem();
    let gs = add_games(&mut db, 2);
    play(
        &mut db,
        &gs,
        &[(0, 1000, 600_000, 0, 0, 0), (1, DAY_MS, 900_000, 0, 1, 60)],
        CountRule::default(),
    );
    let good = snapshot(&mut db);
    let mut bad = db.game_stats(gs[0].id).unwrap().unwrap();
    bad.total_ms += 12345;
    db.put_game_stats(&bad).unwrap();
    assert!(!rebuild::is_consistent(&mut db).unwrap());
    assert!(db.transaction(|tx| rebuild::check_and_repair(tx)).unwrap());
    assert_eq!(snapshot(&mut db), good);
    assert!(!rebuild::check_and_repair(&mut db).unwrap());
}

#[test]
fn removed_history_is_preserved_or_regenerated() {
    let mut db = mem();
    let gs = add_games(&mut db, 2);
    play(
        &mut db,
        &gs,
        &[(0, 1000, 600_000, 0, 0, 0), (1, DAY_MS, 900_000, 0, 0, 0)],
        CountRule::default(),
    );
    db.purge_game(gs[1].id, true).unwrap();
    let kept = snapshot(&mut db);
    assert!(kept.1.iter().any(|d| d.0.starts_with("removed:")));
    assert_eq!(rebuilt(&mut db), kept);
    db.transaction(|tx| {
        rebuild::rebuild(
            tx,
            RebuildOptions {
                include_removed: true,
            },
        )
    })
    .unwrap();
    assert_eq!(snapshot(&mut db), kept);
}

#[test]
fn recount_after_minimum_changes() {
    let mut db = mem();
    let gs = add_games(&mut db, 1);
    // 3 s (not counted at 5 s), 8 s and 20 min (counted).
    play(
        &mut db,
        &gs,
        &[
            (0, 1000, 3_000, 0, 0, 0),
            (0, DAY_MS, 8_000, 0, 0, 0),
            (0, DAY_MS, 1_200_000, 0, 0, 0),
        ],
        CountRule::default(),
    );
    let s0 = rebuild::all_sessions(&mut db).unwrap()[0].id;
    db.add_session_event(s0, Timestamp::from_millis(BASE_MS), "note", "{}")
        .unwrap();
    let old = CountRule::new(true, 5);
    let changed = db
        .transaction(|tx| rebuild::recount(tx, old, CountRule::new(true, 10)))
        .unwrap();
    assert_eq!(changed, 1);
    let st = db.game_stats(gs[0].id).unwrap().unwrap();
    assert_eq!((st.sessions, st.total_ms), (1, 1_200_000));
    let changed = db
        .transaction(|tx| rebuild::recount(tx, CountRule::new(true, 10), CountRule::new(true, 1)))
        .unwrap();
    assert_eq!(changed, 2);
    let st = db.game_stats(gs[0].id).unwrap().unwrap();
    assert_eq!(st.sessions, 3);
    assert_eq!(
        db.session_events(s0).unwrap().len(),
        1,
        "events survive a recount"
    );
}

fn ctx(today: u32) -> QueryContext {
    QueryContext {
        today: key_date(today).unwrap(),
        week_start: Weekday::Monday,
    }
}

#[test]
fn queries_on_a_small_library() {
    let mut db = mem();
    let gs = add_games(&mut db, 3);
    let day = |k: u32| key_days(k) * DAY_MS + 12 * 3_600_000;
    let mut k = 0;
    for (g, d, ms) in [
        (0, 20_261_110, 3_600_000u64),
        (0, 20_261_111, 1_800_000),
        (1, 20_261_111, 600_000),
        (1, 20_261_112, 7_200_000),
        (2, 20_261_001, 60_000),
    ] {
        let s = start(&mut db, &gs[g], k, day(d), SessionKind::Game, 0);
        stats::apply(&mut db, &end(&s, ms, OK), CountRule::default()).unwrap();
        k += 1;
    }
    let failed = start(
        &mut db,
        &gs[0],
        k,
        day(20_261_112) + 1000,
        SessionKind::Game,
        0,
    );
    stats::apply(&mut db, &end(&failed, 1000, OK), CountRule::default()).unwrap();

    let c = ctx(20_261_112);
    let week = DateRange::last_days(c.today, 7);
    let StatsResult::Library(lib) =
        queries::run(&mut db, &c, &StatsRequest::LibraryOverview { range: week }).unwrap()
    else {
        panic!()
    };
    assert_eq!(lib.totals.ms, 3_600_000 + 1_800_000 + 600_000 + 7_200_000);
    assert_eq!(lib.totals.sessions, 4);
    assert_eq!(lib.totals.games_played, 2);
    assert_eq!(lib.series.bucket, Bucket::Day);
    assert_eq!(lib.series.points.len(), 7);
    assert_eq!(lib.series.points[6], (20_261_112, 7_200_000));
    assert_eq!(lib.top[0].key, GameKey::Game(gs[1].id));
    assert_eq!((lib.current_streak, lib.longest_streak), (3, 3));

    let all = DateRange::all_time(c.today);
    let StatsResult::Library(lib_all) =
        queries::run(&mut db, &c, &StatsRequest::LibraryOverview { range: all }).unwrap()
    else {
        panic!()
    };
    assert_eq!(lib_all.totals.games_played, 3);
    assert_eq!(lib_all.series.bucket, Bucket::Week);
    assert_eq!(lib_all.series.points.first().map(|p| p.0), Some(20_260_928));

    let StatsResult::Game(go) = queries::run(
        &mut db,
        &c,
        &StatsRequest::GameOverview {
            game: gs[0].id,
            range: week,
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(go.totals.ms, 5_400_000);
    assert_eq!(go.average_ms, Some(2_700_000));
    assert_eq!(go.last_session_ms, Some(1_800_000));
    assert_eq!(go.recent.len(), 2);
    assert_eq!(go.stats.unwrap().failed_launches, 1);

    let StatsResult::Top(top) = queries::run(
        &mut db,
        &c,
        &StatsRequest::TopGames {
            range: all,
            limit: 2,
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(
        top.iter().map(|r| r.ms).collect::<Vec<_>>(),
        vec![7_800_000, 5_400_000]
    );

    let StatsResult::Weekdays(w) = queries::run(
        &mut db,
        &c,
        &StatsRequest::WeekdayDistribution { range: week },
    )
    .unwrap() else {
        panic!()
    };
    // 2026-11-10 Tuesday, 11 Wednesday, 12 Thursday.
    assert_eq!(w, [0, 3_600_000, 2_400_000, 7_200_000, 0, 0, 0]);

    let mut page = |p: u32, uncounted: bool| {
        let StatsResult::History(h) = queries::run(
            &mut db,
            &c,
            &StatsRequest::SessionHistory {
                game: None,
                page: p,
                page_size: 2,
                include_uncounted: uncounted,
                include_tools: false,
            },
        )
        .unwrap() else {
            panic!()
        };
        h
    };
    let p0 = page(0, false);
    assert_eq!(p0.rows.len(), 2);
    assert!(p0.has_more);
    assert_eq!(p0.rows[0].duration_ms, 7_200_000);
    let p2 = page(2, false);
    assert_eq!((p2.rows.len(), p2.has_more), (1, false));
    assert_eq!(page(0, true).rows[0].duration_ms, 1000);
}

/// STATISTICS §6 budget: 500 games × 300 days ≈ 150 000 daily rows; any request < 20 ms in a
/// release build. Run with `cargo test --release -- --ignored query_budget`.
#[test]
#[ignore = "benchmark; run in release"]
fn query_budget() {
    let mut db = mem();
    let gs = add_games(&mut db, 500);
    let mut rng = Lcg::new(1);
    db.transaction(|tx| -> zaun_core::storage::StorageResult<()> {
        for g in &gs {
            tx.put_game_stats(&GameStats {
                total_ms: 1,
                sessions: 1,
                ..GameStats::empty(g.id)
            })?;
            for d in 0..300 {
                let key = day_key(20_000 + d);
                tx.add_playtime(&GameKey::Game(g.id), key, 1000 + rng.below(100_000), 1)?;
            }
        }
        Ok(())
    })
    .unwrap();
    let c = ctx(day_key(20_300));
    let clock = zaun_core::clock::SystemClock::new();
    for req in [
        StatsRequest::LibraryOverview {
            range: DateRange::last_days(c.today, 30),
        },
        StatsRequest::LibraryOverview {
            range: DateRange::all_time(c.today),
        },
        StatsRequest::GameOverview {
            game: gs[7].id,
            range: DateRange::all_time(c.today),
        },
        StatsRequest::TopGames {
            range: DateRange::last_days(c.today, 365),
            limit: 10,
        },
        StatsRequest::WeekdayDistribution {
            range: DateRange::all_time(c.today),
        },
    ] {
        let t0 = clock.mono();
        queries::run(&mut db, &c, &req).unwrap();
        let took = clock.mono().saturating_duration_since(t0);
        eprintln!("{req:?}: {took:?}");
        if !cfg!(debug_assertions) {
            assert!(took < Duration::from_millis(20), "{req:?} took {took:?}");
        }
    }
}
