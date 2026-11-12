//! Library budgets of PERFORMANCE §1 (PERF-001): snapshot rebuild from the database,
//! incremental update, search, sort and filter over 5 000 games.
//!
//! `cargo bench -p easyplay-core --bench library` (optimised); `cargo test --benches` runs
//! it as a smoke test.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use easyplay_core::clock::Timestamp;
use easyplay_core::library::index::{self, DefaultRuntimeInfo, GameRows, Overlays};
use easyplay_core::library::query::{Collection, Filters, LibraryQuery, SortKey, run_query};
use easyplay_core::storage::{ArtworkRepo, Db, GameRepo, MigrateOptions, StatsRepo, TagRepo};
use easyplay_testkit::bench::{Bench, finish};
use easyplay_testkit::fixtures::games::populate;

const GAMES: u32 = 5_000;

fn now() -> Timestamp {
    Timestamp::from_millis(1_794_441_600_000)
}

fn main() {
    let dir = tempfile::Builder::new()
        .prefix("bench-library-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let mut db = Db::open_rw(&dir.path().join("library.db")).unwrap();
    db.migrate(&MigrateOptions::default()).unwrap();
    db.transaction(|tx| Ok::<_, easyplay_core::storage::StorageError>(populate(tx, GAMES, 8, 7)))
        .unwrap();
    let runtime = DefaultRuntimeInfo;
    let overlays = Overlays::default();
    let mut reports = Vec::new();

    reports.push(
        Bench::new("snapshot_rebuild_5000", Duration::from_millis(30))
            .iterations(20)
            .run(|| {
                let rows = db.load_library().unwrap();
                index::build(&rows, &runtime, &overlays, 1, now())
            }),
    );

    // The two halves of the rebuild, to see where time goes.
    reports.push(
        Bench::new("snapshot_load_rows_5000", Duration::from_millis(30))
            .iterations(20)
            .run(|| db.load_library().unwrap()),
    );
    let rows = db.load_library().unwrap();
    reports.push(
        Bench::new("snapshot_index_5000", Duration::from_millis(30))
            .iterations(20)
            .run(|| index::build(&rows, &runtime, &overlays, 1, now())),
    );
    let snapshot = index::build(&rows, &runtime, &overlays, 1, now());
    let id = snapshot.games[snapshot.games.len() / 2].id;
    reports.push(
        Bench::new("snapshot_update_one", Duration::from_micros(500)).run(|| {
            let rec = db.game_record(id).unwrap().unwrap();
            let tags = db.tags_of(id).unwrap();
            let art = db.artwork_of(id).unwrap();
            let stats = db.game_stats(id).unwrap();
            let summary = index::summarize(
                GameRows {
                    record: &rec,
                    tags: &tags,
                    artwork: &art,
                    stats: stats.as_ref(),
                },
                &runtime,
                &overlays,
                snapshot.get(id),
            );
            snapshot.with_game(summary, now())
        }),
    );

    // The in-memory half of the incremental update.
    let one = snapshot.get(id).unwrap().clone();
    reports.push(
        Bench::new("snapshot_with_game_5000", Duration::from_micros(500))
            .run(|| snapshot.with_game(one.clone(), now())),
    );

    // One keystroke: the query text grows by one character (no cache between iterations).
    let words = [
        "h", "ho", "hol", "holl", "hollo", "hollow", "the w", "2", "is:fav",
    ];
    let mut k = 0usize;
    reports.push(
        Bench::new("search_5000", Duration::from_millis(2))
            .p99()
            .iterations(200)
            .run(|| {
                k += 1;
                let q = LibraryQuery {
                    collection: Collection::All,
                    text: words[k % words.len()].to_owned(),
                    ..LibraryQuery::default()
                };
                run_query(&snapshot, &q)
            }),
    );

    let sorts = [
        SortKey::Name,
        SortKey::LastPlayed,
        SortKey::Playtime,
        SortKey::DateAdded,
        SortKey::Launches,
        SortKey::Protection,
        SortKey::Layer,
    ];
    reports.push(
        Bench::new("sort_5000", Duration::from_millis(3))
            .p99()
            .iterations(140)
            .run(|| {
                k += 1;
                let sort = sorts[k % sorts.len()];
                let q = LibraryQuery {
                    collection: Collection::All,
                    sort,
                    descending: sort.default_descending(),
                    ..LibraryQuery::default()
                };
                run_query(&snapshot, &q)
            }),
    );

    let tags: Vec<_> = snapshot.tags.iter().map(|t| t.id).collect();
    reports.push(
        Bench::new("filter_5000", Duration::from_millis(3))
            .p99()
            .iterations(100)
            .run(|| {
                k += 1;
                let q = LibraryQuery {
                    collection: Collection::All,
                    filters: Filters {
                        tags: vec![tags[k % tags.len()]],
                        ..Filters::default()
                    },
                    ..LibraryQuery::default()
                };
                run_query(&snapshot, &q)
            }),
    );

    finish(&reports);
}
