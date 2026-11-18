//! Runtime discovery, catalogue synchronisation and resolution (RT-001 … RT-005).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use zaun_core::clock::Timestamp;
use zaun_core::fsx::Fs;
use zaun_core::ids::RuntimeName;
use zaun_core::jobs::{JobPool, JobPoolConfig};
use zaun_core::model::{
    Channel, RuntimeFamily, RuntimeIsolation, RuntimeIsolationMode, RuntimeOrigin, RuntimeRef,
    SlrVariant,
};
use zaun_core::runtime::catalog::{CatalogStore, diff, sync};
use zaun_core::runtime::discovery::{DiscoveryInput, RejectReason};
use zaun_core::runtime::{
    IsolationInputs, LocalRuntimeService, MemCatalogStore, ProvisionNeed, Resolution, ResolveInput,
    RuntimeCatalog, RuntimeService, RuntimeSettings, RuntimeStore, UnavailableReason,
    catalog_check, discover, resolve, select_isolation,
};
use zaun_testkit::fixtures::runtime_trees::{self, RuntimeTree, SNIPER, standard_tree};
use zaun_testkit::{FakeClock, MemFs};

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

fn name(s: &str) -> RuntimeName {
    RuntimeName::new(s).unwrap()
}

#[test]
fn discovery_yields_the_expected_catalogue() {
    let fs = MemFs::new();
    let t = standard_tree(&fs);
    let d = scan(&fs, &t);
    let names: Vec<(&str, RuntimeOrigin)> = d
        .builds
        .iter()
        .map(|b| (b.name.as_str(), b.origin))
        .collect();
    assert_eq!(
        names,
        vec![
            ("GE-Proton10-25", RuntimeOrigin::Store),
            ("GE-Proton10-9", RuntimeOrigin::Store),
            ("UMU-Proton-10.0-4", RuntimeOrigin::Store),
            ("UMU-Proton-9.0-1", RuntimeOrigin::External),
            ("proton-cachyos-11.0", RuntimeOrigin::External),
        ]
    );
    let ge = &d.builds[0];
    assert_eq!(ge.family, RuntimeFamily::GeProton);
    assert_eq!(ge.required_slr, SlrVariant::Steamrt3);
    assert_eq!(ge.origin_label, "Managed by Zaun");
    assert!(ge.has_fixes);
    let umu9 = &d.builds[3];
    assert_eq!(
        umu9.display_name, "UMU-Proton 9.0-1",
        "fallback display name"
    );
    assert_eq!(umu9.origin_label, "From Steam");
    assert!(!umu9.has_fixes);
    assert_eq!(d.builds[4].required_slr, SlrVariant::Steamrt4);

    let reason = |n: &str| {
        d.rejected
            .iter()
            .find(|r| r.path.file_name().unwrap() == n)
            .map(|r| r.reason.clone())
    };
    assert_eq!(reason("broken"), Some(RejectReason::NoProtonScript));
    assert_eq!(reason("host-runtime"), Some(RejectReason::HostRuntime));
    assert_eq!(
        reason("soldier"),
        Some(RejectReason::UnsupportedRuntime("1391110".into()))
    );
    assert_eq!(reason("escape"), Some(RejectReason::SymlinkOutside));
    // The Flatpak-data build (reached through the Heroic link) is never scanned.
    assert!(
        d.builds
            .iter()
            .all(|b| !b.path.starts_with(t.home.join(".var")))
    );
    assert!(
        d.rejected
            .iter()
            .all(|r| !r.path.starts_with(t.home.join(".var")))
    );
    // `alias` and `~/.steam/root` reach the same directories: no duplicates.
    assert_eq!(
        d.builds
            .iter()
            .filter(|b| b.name.as_str() == "UMU-Proton-9.0-1")
            .count(),
        1
    );

    assert_eq!(d.slr.len(), 1);
    let slr = &d.slr[0];
    assert!(slr.is_complete());
    assert_eq!(slr.version.as_deref(), Some("3.0.20261110.268881"));
    assert!(
        slr.platform_dir
            .as_ref()
            .unwrap()
            .ends_with("sniper_platform_3.0.20261110.268881")
    );

    assert_eq!(d.foreign.slr.len(), 1);
    assert_eq!(d.foreign.partial_downloads.len(), 1);
}

#[test]
fn incomplete_slr_is_detected() {
    let fs = MemFs::new();
    runtime_trees::slr_variant(&fs, "/s/umu/steamrt4", "steamrt4", "4.0.1", false);
    let s = zaun_core::runtime::discovery::detect_slr(
        &fs,
        Path::new("/s/umu/steamrt4"),
        SlrVariant::Steamrt4,
    )
    .unwrap();
    assert!(!s.is_complete() && !s.marker && !s.umu_link && s.platform_dir.is_some());
}

#[test]
fn sync_is_idempotent_and_keeps_timestamps() {
    let fs = MemFs::new();
    let t = standard_tree(&fs);
    let rows = MemCatalogStore::new();
    let (c1, d1) = sync(&rows, scan(&fs, &t), NOW);
    assert_eq!(d1.added.len(), 5);
    assert_eq!(rows.applies(), 1);
    let (c2, d2) = sync(
        &rows,
        scan(&fs, &t),
        Timestamp::from_millis(NOW.as_millis() + 1000),
    );
    assert!(d2.is_empty());
    assert_eq!(rows.applies(), 1, "nothing written the second time");
    assert_eq!(c1.builds, c2.builds);

    // Removing a build and changing another.
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
    assert_eq!(d3.removed, vec![name("GE-Proton10-9")]);
    assert_eq!(d3.updated.len(), 1);
    let renamed = c3.by_name(&name("UMU-Proton-9.0-1")).unwrap();
    assert_eq!(renamed.display_name, "Renamed");
    assert_eq!(renamed.discovered_at, NOW, "first-seen time is kept");
    assert_eq!(rows.load_builds().len(), 4);
    assert!(diff(&rows.load_builds(), &c3.builds).is_empty());
}

fn catalog(fs: &MemFs, t: &RuntimeTree) -> RuntimeCatalog {
    RuntimeCatalog::from_discovery(scan(fs, t), NOW)
}

struct Case {
    reference: RuntimeRef,
    follow: bool,
    last: Option<&'static str>,
    expect: Expect,
}

#[derive(Debug, PartialEq)]
enum Expect {
    Ready(String),
    Needs(ProvisionNeed),
    Unavailable(UnavailableReason),
}

fn run_case(c: &Case, cat: &RuntimeCatalog, t: &RuntimeTree, default: &RuntimeRef) -> Expect {
    let store = RuntimeStore::at(&t.data, &t.cache);
    let last = c.last.map(name);
    let input = ResolveInput {
        reference: &c.reference,
        default,
        follow_updates: c.follow,
        last_resolved: last.as_ref(),
        store: &store,
        home: &t.home,
        isolation: IsolationInputs::default(),
    };
    match resolve(&input, cat) {
        Resolution::Ready(r) => {
            // Sandbox-side paths.
            match r.origin {
                RuntimeOrigin::Store => assert_eq!(
                    r.proton_path_sandbox,
                    t.home
                        .join(".local/share/Steam/compatibilitytools.d")
                        .join(r.name.as_str())
                ),
                RuntimeOrigin::External => assert_eq!(r.proton_path_sandbox, r.proton_path_host),
            }
            assert_eq!(r.slr_dir, t.data.join("runtimes/umu/steamrt3"));
            Expect::Ready(r.name.to_string())
        }
        Resolution::NeedsProvision(p) => Expect::Needs(p),
        Resolution::Unavailable(u) => Expect::Unavailable(u),
    }
}

#[test]
fn resolver_table() {
    let fs = MemFs::new();
    let t = standard_tree(&fs);
    let cat = catalog(&fs, &t);
    let ext = t
        .home
        .join(".local/share/Steam/compatibilitytools.d/UMU-Proton-9.0-1");
    let cases = vec![
        // Default → the preference (Channel GE here).
        Case {
            reference: RuntimeRef::Default,
            follow: true,
            last: None,
            expect: Expect::Ready("GE-Proton10-25".into()),
        },
        // Channel: newest by natural order.
        Case {
            reference: RuntimeRef::Channel(Channel::GeProton),
            follow: true,
            last: None,
            expect: Expect::Ready("GE-Proton10-25".into()),
        },
        Case {
            reference: RuntimeRef::Channel(Channel::UmuProton),
            follow: true,
            last: None,
            expect: Expect::Ready("UMU-Proton-10.0-4".into()),
        },
        // follow_updates = false keeps the last resolved build while installed …
        Case {
            reference: RuntimeRef::Channel(Channel::GeProton),
            follow: false,
            last: Some("GE-Proton10-9"),
            expect: Expect::Ready("GE-Proton10-9".into()),
        },
        // … and falls back to the newest when it is gone.
        Case {
            reference: RuntimeRef::Channel(Channel::GeProton),
            follow: false,
            last: Some("GE-Proton9-1"),
            expect: Expect::Ready("GE-Proton10-25".into()),
        },
        // Pinned installed / downloadable / unknown.
        Case {
            reference: RuntimeRef::Pinned(name("GE-Proton10-9")),
            follow: true,
            last: None,
            expect: Expect::Ready("GE-Proton10-9".into()),
        },
        Case {
            reference: RuntimeRef::Pinned(name("GE-Proton10-30")),
            follow: true,
            last: None,
            expect: Expect::Needs(ProvisionNeed::Exact(name("GE-Proton10-30"))),
        },
        Case {
            reference: RuntimeRef::Pinned(name("my-build")),
            follow: true,
            last: None,
            expect: Expect::Unavailable(UnavailableReason::UnknownPinned(name("my-build"))),
        },
        // External valid / invalid.
        Case {
            reference: RuntimeRef::External(ext.clone()),
            follow: true,
            last: None,
            expect: Expect::Ready("UMU-Proton-9.0-1".into()),
        },
        Case {
            reference: RuntimeRef::External("/gone".into()),
            follow: true,
            last: None,
            expect: Expect::Unavailable(UnavailableReason::ExternalInvalid("/gone".into())),
        },
        // SLR requirement from the manifest: steamrt4 is not in the store.
        Case {
            reference: RuntimeRef::External(
                t.home
                    .join(".local/share/Steam/compatibilitytools.d/proton-cachyos-11.0"),
            ),
            follow: true,
            last: None,
            expect: Expect::Needs(ProvisionNeed::Slr {
                variant: SlrVariant::Steamrt4,
                build: name("proton-cachyos-11.0"),
            }),
        },
    ];
    let default = RuntimeRef::Channel(Channel::GeProton);
    for (i, c) in cases.iter().enumerate() {
        assert_eq!(
            run_case(c, &cat, &t, &default),
            c.expect,
            "case {i}: {:?}",
            c.reference
        );
    }

    // Empty store: a channel needs its codename.
    let empty = RuntimeCatalog::default();
    let c = Case {
        reference: RuntimeRef::Default,
        follow: true,
        last: None,
        expect: Expect::Needs(ProvisionNeed::Codename(Channel::UmuProton)),
    };
    assert_eq!(run_case(&c, &empty, &t, &RuntimeRef::Default), c.expect);
    assert_eq!(
        ProvisionNeed::Codename(Channel::UmuProton).protonpath(),
        "UMU-Proton"
    );
}

#[test]
fn isolation_table() {
    use RuntimeIsolation::*;
    use RuntimeIsolationMode as M;
    let cases = [
        (Isolated, false, Some(false), M::Isolated),
        (Shared, true, Some(true), M::Shared),
        (Auto, true, Some(false), M::Isolated),
        (Auto, false, Some(true), M::Isolated),
        (Auto, false, Some(false), M::Shared),
        (Auto, false, None, M::Shared),
    ];
    for (policy, always, reflink, want) in cases {
        let got = select_isolation(IsolationInputs {
            policy,
            always_isolate: always,
            reflink_ok: reflink,
        });
        assert_eq!(got, want, "{policy:?} {always} {reflink:?}");
    }
}

#[test]
fn service_resolves_custom_external_dirs_and_refreshes() {
    let fs = Arc::new(MemFs::new());
    let t = standard_tree(&fs);
    runtime_trees::proton_build(&fs, "/games/tools/MyProton", Some(SNIPER), None);
    runtime_trees::proton_build(&fs, "/games/tools/NoRuntime", None, None);
    let clock = Arc::new(FakeClock::baseline());
    let pool = Arc::new(JobPool::new(JobPoolConfig::default(), clock.clone()).unwrap());
    let svc = LocalRuntimeService::new(
        fs.clone(),
        clock,
        Arc::new(MemCatalogStore::new()),
        RuntimeStore::at(&t.data, &t.cache),
        t.home.clone(),
        RuntimeSettings::default(),
        pool,
    );
    assert!(svc.catalog().builds.is_empty());
    let c = svc.refresh().wait().unwrap();
    assert_eq!(c.builds.len(), 5);
    assert_eq!(svc.catalog().builds.len(), 5);
    assert!(
        matches!(svc.resolve(&RuntimeRef::Default, None), Resolution::Ready(r) if r.name.as_str() == "UMU-Proton-10.0-4")
    );
    assert!(matches!(
        svc.resolve(&RuntimeRef::External(PathBuf::from("/games/tools/MyProton")), None),
        Resolution::Ready(r) if r.origin == RuntimeOrigin::External
    ));
    assert_eq!(
        svc.resolve(
            &RuntimeRef::External(PathBuf::from("/games/tools/NoRuntime")),
            None
        ),
        Resolution::Unavailable(UnavailableReason::UnsupportedRuntime(name("NoRuntime")))
    );
    let check = catalog_check(&c);
    assert_eq!(check.id, "D-15");
    assert!(check.summary.contains("steamrt3"), "{}", check.summary);
    assert!(fs.exists(&t.data.join("runtimes/umu/steamrt3")));
}
