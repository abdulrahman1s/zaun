//! `CatalogStore` on the SQLite `runtimes` table (RT-004 with STORE-012).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use zaun_core::clock::Timestamp;
use zaun_core::fsx::Fs;
use zaun_core::ids::RuntimeName;
use zaun_core::runtime::catalog::{CatalogStore, diff, sync};
use zaun_core::runtime::discovery::DiscoveryInput;
use zaun_core::runtime::{RuntimeStore, SqliteCatalogStore, discover};
use zaun_core::storage::{Db, MigrateOptions, RuntimeRepo};
use zaun_testkit::MemFs;
use zaun_testkit::fixtures::runtime_trees::{RuntimeTree, standard_tree};

const NOW: Timestamp = Timestamp::from_millis(1_794_441_600_000);

fn scan(fs: &MemFs, t: &RuntimeTree) -> zaun_core::runtime::Discovery {
    let store = RuntimeStore::at(&t.data, &t.cache);
    discover(
        fs,
        &DiscoveryInput {
            store: &store,
            home: Some(&t.home),
            extra_dirs: &[],
            now: NOW,
        },
    )
}

#[test]
fn sqlite_catalog_store_syncs_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let mut db = Db::open_rw(&path).unwrap();
    db.migrate(&MigrateOptions::default()).unwrap();
    let rows = SqliteCatalogStore::new(db);

    let fs = MemFs::new();
    let t = standard_tree(&fs);
    let (c1, d1) = sync(&rows, scan(&fs, &t), NOW);
    assert_eq!(d1.added.len(), 5);
    let (_, d2) = sync(
        &rows,
        scan(&fs, &t),
        Timestamp::from_millis(NOW.as_millis() + 1000),
    );
    assert!(d2.is_empty());

    // Remove one build, rename another.
    fs.remove_file(
        &t.data
            .join("runtimes/Steam/compatibilitytools.d/GE-Proton10-9/proton"),
    )
    .unwrap();
    fs.file(
        t.home
            .join(".local/share/Steam/compatibilitytools.d/UMU-Proton-9.0-1/compatibilitytool.vdf"),
        "\"compatibilitytools\" { \"compat_tools\" { \"x\" { \"display_name\" \"Renamed\" } } }",
    );
    let later = Timestamp::from_millis(NOW.as_millis() + 5000);
    let (c3, d3) = sync(&rows, scan(&fs, &t), later);
    assert_eq!(d3.removed, vec![RuntimeName::new("GE-Proton10-9").unwrap()]);
    assert_eq!(d3.updated.len(), 1);
    let renamed = c3
        .by_name(&RuntimeName::new("UMU-Proton-9.0-1").unwrap())
        .unwrap();
    assert_eq!(renamed.discovered_at, NOW, "first-seen time is kept");
    assert_eq!(rows.load_builds().len(), 4);
    assert!(diff(&rows.load_builds(), &c3.builds).is_empty());
    assert_eq!(c1.builds.len(), 5);

    // The rows survive closing and reopening the database.
    rows.into_inner().close().unwrap();
    let mut db = Db::open_rw(&path).unwrap();
    assert_eq!(db.runtimes().unwrap().len(), 4);
}
