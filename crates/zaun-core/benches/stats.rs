//! Statistics budgets of PERFORMANCE §1 (PERF-001, STATS-003): the overview queries on the
//! fixture of 500 games with sessions spread over the preceding 900 days, read through a
//! read-only connection like the job pool does.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use jiff::civil::Weekday;
use zaun_core::stats::queries::{self, DateRange, QueryContext, StatsRequest};
use zaun_core::stats::rollup::key_date;
use zaun_core::storage::{Db, MigrateOptions, StorageError};
use zaun_testkit::bench::{Bench, finish};
use zaun_testkit::fixtures::games::populate;

fn main() {
    let dir = tempfile::Builder::new()
        .prefix("bench-stats-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let file = dir.path().join("library.db");
    let mut db = Db::open_rw(&file).unwrap();
    db.migrate(&MigrateOptions::default()).unwrap();
    let made = db
        .transaction(|tx| Ok::<_, StorageError>(populate(tx, 500, 60, 11)))
        .unwrap();
    db.close().unwrap();
    println!("fixture: 500 games, {} sessions", made.sessions);
    let mut ro = Db::open_ro(&file).unwrap();
    let ctx = QueryContext {
        today: key_date(20_261_112).unwrap(),
        week_start: Weekday::Monday,
    };
    let game = made.games[0];
    let ranges = [
        ("stats_overview_week", DateRange::last_days(ctx.today, 7)),
        ("stats_overview_month", DateRange::last_days(ctx.today, 30)),
        ("stats_overview_year", DateRange::this_year(ctx.today)),
        ("stats_overview_all_time", DateRange::all_time(ctx.today)),
    ];
    let mut reports = Vec::new();
    for (name, range) in ranges {
        reports.push(
            Bench::new(name, Duration::from_millis(20))
                .iterations(30)
                .run(|| {
                    queries::run(&mut ro, &ctx, &StatsRequest::LibraryOverview { range }).unwrap()
                }),
        );
    }
    reports.push(
        Bench::new("stats_game_all_time", Duration::from_millis(20))
            .iterations(30)
            .run(|| {
                queries::run(
                    &mut ro,
                    &ctx,
                    &StatsRequest::GameOverview {
                        game,
                        range: DateRange::all_time(ctx.today),
                    },
                )
                .unwrap()
            }),
    );
    finish(&reports);
}
