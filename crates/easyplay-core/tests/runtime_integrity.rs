//! Runtime integrity (RT-006 … RT-008, RT-013, RT-014): manifests, audits of every kind of
//! difference, tamper handling with quarantine, event row and suspected games, garbage
//! collection, and the umu database reader.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use easyplay_core::clock::{Clock, SystemClock, Timestamp};
use easyplay_core::fsx::{Fs, RealFs};
use easyplay_core::ids::RuntimeName;
use easyplay_core::jobs::{CancelToken, JobPool, JobPoolConfig};
use easyplay_core::launch::trusted::DbRecords;
use easyplay_core::model::{
    GameFlags, RuntimeIsolationMode, RuntimeRef, SessionContext, SlrVariant,
};
use easyplay_core::runtime::audit::{self, AuditReport};
use easyplay_core::runtime::integrity::{
    AUDIT_MAX_AGE, AuditAction, AuditDue, audit_due, gate_applies, handle_audit,
};
use easyplay_core::runtime::{
    LocalRuntimeService, MemCatalogStore, ProvisionNeed, Resolution, RuntimeService,
    RuntimeSettings, RuntimeStore, gc, manifest,
};
use easyplay_core::storage::{
    AuditState, Db, GameRepo, MigrateOptions, RuntimeRepo, SessionClose, SessionRepo,
};
use easyplay_testkit::fixtures::games;

fn now() -> Timestamp {
    Timestamp::from_millis(1_794_480_000_000)
}

struct Tree {
    _tmp: tempfile::TempDir,
    data: PathBuf,
    store: RuntimeStore,
    dir: PathBuf,
}

/// A complete store: `steamrt3` (with platform, `.ref`, a link, `var/`) and one build.
fn tree() -> Tree {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let store = RuntimeStore::at(&data, &tmp.path().join("cache"));
    let v = store.slr_dir(SlrVariant::Steamrt3);
    let lib = v.join("sniper_platform_3.0.1/files/lib");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::create_dir_all(v.join("var/tmp-1")).unwrap();
    std::fs::write(v.join("var/tmp-1/junk"), "pv").unwrap();
    std::fs::write(lib.join("libx.so"), vec![7u8; 50_000]).unwrap();
    std::fs::write(lib.join("liby.so"), "y").unwrap();
    std::fs::write(v.join("sniper_platform_3.0.1/files/.ref"), "").unwrap();
    std::fs::write(v.join("_v2-entry-point"), "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(
        v.join("_v2-entry-point"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    symlink("_v2-entry-point", v.join("umu")).unwrap();
    std::fs::write(v.join("VERSIONS.txt"), "depot\t3.0.1\n").unwrap();
    std::fs::write(
        v.join("toolmanifest.vdf"),
        "\"manifest\"\n{\n \"commandline\" \"/_v2-entry-point --verb=%verb% --\"\n}\n",
    )
    .unwrap();
    std::fs::write(v.join(".installed.ok"), "ok\n").unwrap();
    std::fs::write(store.umu_shim(), "#!/bin/sh\n").unwrap();
    for b in ["GE-Proton10-9", "GE-Proton10-25"] {
        let d = store.proton_dir(b);
        std::fs::create_dir_all(d.join("files")).unwrap();
        std::fs::write(
            d.join("toolmanifest.vdf"),
            "\"manifest\"\n{\n \"version\" \"2\"\n \"commandline\" \"/proton %verb%\"\n \"require_tool_appid\" \"1628350\"\n \"compatmanager_layer_name\" \"proton\"\n}\n",
        )
        .unwrap();
        std::fs::write(d.join("proton"), "#!/usr/bin/env python3\n").unwrap();
    }
    Tree {
        _tmp: tmp,
        data,
        store,
        dir: v,
    }
}

fn build(t: &Tree) -> manifest::Manifest {
    let fs = RealFs::new();
    let m = manifest::build(
        &fs,
        &SystemClock::new(),
        &t.dir,
        SlrVariant::Steamrt3,
        Some("3.0.1".into()),
        Some(&t.store.umu_shim()),
        now(),
        &CancelToken::new(),
    )
    .unwrap();
    manifest::write(&fs, &t.store.manifests, &m).unwrap();
    m
}

fn run_audit(t: &Tree, m: &manifest::Manifest) -> AuditReport {
    audit::run(
        &RealFs::new(),
        &SystemClock::new(),
        &t.dir,
        m,
        Some(&t.store.umu_shim()),
        &CancelToken::new(),
    )
    .unwrap()
}

fn tampered(r: AuditReport) -> audit::TamperSummary {
    match r {
        AuditReport::Tampered(s) => s,
        other => panic!("expected tampering, got {other:?}"),
    }
}

#[test]
fn manifest_is_deterministic_and_excludes_volatile_entries() {
    let t = tree();
    let a = build(&t);
    let b = build(&t);
    assert_eq!(a.files, b.files);
    assert_eq!(a.tree_sha256, b.tree_sha256);
    let paths: Vec<&str> = a.files.iter().map(|e| e.p.as_str()).collect();
    assert!(paths.windows(2).all(|w| w[0].as_bytes() < w[1].as_bytes()));
    for excluded in [
        "var",
        "var/tmp-1/junk",
        ".installed.ok",
        "sniper_platform_3.0.1/files/.ref",
    ] {
        assert!(!paths.contains(&excluded), "{excluded}");
    }
    assert!(paths.contains(&"umu") && paths.contains(&"sniper_platform_3.0.1/files/lib/libx.so"));
    assert!(a.umu_shim.is_some());
    // Stored, mode 0600, and read back with its own tree hash and the stored one.
    let file = t.store.manifest_file(SlrVariant::Steamrt3);
    let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let tree = easyplay_core::model::from_hex32(&a.tree_sha256).unwrap();
    assert!(manifest::load(&RealFs::new(), &file, SlrVariant::Steamrt3, Some(&tree)).is_ok());
    assert_eq!(
        manifest::load(&RealFs::new(), &file, SlrVariant::Steamrt3, Some(&[0; 32])).unwrap_err(),
        manifest::ManifestProblem::Replaced
    );
}

#[test]
fn audit_classifies_every_kind_of_difference() {
    // Each case starts from a fresh tree.
    type Case = (&'static str, fn(&Path), &'static str);
    let cases: [Case; 7] = [
        (
            "content",
            |v| std::fs::write(v.join("sniper_platform_3.0.1/files/lib/liby.so"), "z").unwrap(),
            "changed",
        ),
        (
            "mode",
            |v| {
                std::fs::set_permissions(
                    v.join("sniper_platform_3.0.1/files/lib/liby.so"),
                    std::fs::Permissions::from_mode(0o755),
                )
                .unwrap();
            },
            "changed",
        ),
        (
            "symlink target",
            |v| {
                std::fs::remove_file(v.join("umu")).unwrap();
                symlink("run", v.join("umu")).unwrap();
            },
            "changed",
        ),
        (
            "added",
            |v| std::fs::write(v.join("sniper_platform_3.0.1/files/lib/evil.so"), "x").unwrap(),
            "added",
        ),
        (
            "removed",
            |v| std::fs::remove_file(v.join("VERSIONS.txt")).unwrap(),
            "removed",
        ),
        (
            "kind change",
            |v| {
                std::fs::remove_file(v.join("VERSIONS.txt")).unwrap();
                std::fs::create_dir(v.join("VERSIONS.txt")).unwrap();
            },
            "changed",
        ),
        (
            "same size, other bytes",
            |v| {
                let p = v.join("sniper_platform_3.0.1/files/lib/libx.so");
                let mut d = std::fs::read(&p).unwrap();
                d[100] = 8;
                std::fs::write(&p, d).unwrap();
            },
            "changed",
        ),
    ];
    for (name, change, kind) in cases {
        let t = tree();
        let m = build(&t);
        assert!(
            matches!(run_audit(&t, &m), AuditReport::Clean { .. }),
            "{name}"
        );
        change(&t.dir);
        let s = tampered(run_audit(&t, &m));
        let (list, count) = match kind {
            "changed" => (&s.changed, s.changed_count),
            "added" => (&s.added, s.added_count),
            _ => (&s.removed, s.removed_count),
        };
        assert_eq!(count, 1, "{name}: {s:?}");
        assert_eq!(list.len(), 1, "{name}");
    }
    // var/ and the marker are pressure-vessel's and umu's: never a difference.
    let t = tree();
    let m = build(&t);
    std::fs::write(t.dir.join("var/tmp-1/more"), "x").unwrap();
    std::fs::write(t.dir.join(".installed.ok"), "changed\n").unwrap();
    assert!(matches!(run_audit(&t, &m), AuditReport::Clean { .. }));
    // umu-shim is covered too.
    std::fs::write(t.store.umu_shim(), "#!/bin/sh\nevil\n").unwrap();
    assert_eq!(tampered(run_audit(&t, &m)).changed, vec!["../umu-shim"]);
}

#[test]
fn ref_file_rules_and_missing_runtime() {
    let t = tree();
    let m = build(&t);
    let r = t.dir.join("sniper_platform_3.0.1/files/.ref");
    std::fs::write(&r, vec![b'x'; (2 << 20) as usize]).unwrap();
    match run_audit(&t, &m) {
        AuditReport::Clean { notes, .. } => assert_eq!(notes.len(), 1),
        other => panic!("{other:?}"),
    }
    assert_eq!(std::fs::metadata(&r).unwrap().len(), 0, "truncated");
    std::fs::remove_file(&r).unwrap();
    std::fs::create_dir(&r).unwrap();
    assert_eq!(tampered(run_audit(&t, &m)).changed_count, 1);
    // Manifest file replaced (stored tree hash differs) or missing.
    let (rep, _) = audit::audit_variant(
        &RealFs::new(),
        &SystemClock::new(),
        &t.store,
        SlrVariant::Steamrt3,
        Some(&[1; 32]),
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(tampered(rep).manifest_problem.as_deref(), Some("replaced"));
    std::fs::remove_file(t.store.manifest_file(SlrVariant::Steamrt3)).unwrap();
    let (rep, _) = audit::audit_variant(
        &RealFs::new(),
        &SystemClock::new(),
        &t.store,
        SlrVariant::Steamrt3,
        None,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(tampered(rep).manifest_problem.as_deref(), Some("missing"));
    let (rep, _) = audit::audit_variant(
        &RealFs::new(),
        &SystemClock::new(),
        &t.store,
        SlrVariant::Steamrt4,
        None,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(rep, AuditReport::Missing);
}

#[test]
fn audit_schedule_table() {
    let st = |pending, clean_ago: Option<Duration>| AuditState {
        variant: SlrVariant::Steamrt3,
        pending,
        last_run_at: None,
        last_clean_at: clean_ago.map(|d| now().saturating_sub(d)),
        last_result: None,
    };
    let day = Duration::from_secs(86_400);
    assert_eq!(audit_due(None, now()), AuditDue::Stale);
    assert_eq!(
        audit_due(Some(&st(true, Some(day))), now()),
        AuditDue::Pending
    );
    assert_eq!(audit_due(Some(&st(false, Some(day))), now()), AuditDue::No);
    assert_eq!(
        audit_due(Some(&st(false, Some(AUDIT_MAX_AGE + day))), now()),
        AuditDue::Stale
    );
    assert_eq!(audit_due(Some(&st(false, None)), now()), AuditDue::Stale);
    use RuntimeIsolationMode::{Isolated, Shared};
    assert!(gate_applies(AuditDue::Pending, Shared));
    assert!(!gate_applies(AuditDue::Pending, Isolated));
    assert!(gate_applies(AuditDue::Stale, Isolated));
    assert!(!gate_applies(AuditDue::No, Shared));
}

fn library(t: &Tree) -> Db {
    std::fs::create_dir_all(&t.data).unwrap();
    let mut db = Db::open_rw(&t.data.join("library.sqlite")).unwrap();
    db.migrate(&MigrateOptions::default()).unwrap();
    db
}

#[test]
fn modified_runtime_is_quarantined_and_needs_provision() {
    // RT-008 acceptance.
    let t = tree();
    let m = build(&t);
    let mut db = library(&t);
    // Two games ran in Shared mode since the last clean audit; one in Isolated mode, one
    // before it.
    let (a, b, c, old) = (
        games::game(1, 1),
        games::game(2, 1),
        games::game(3, 1),
        games::game(4, 1),
    );
    for g in [&a, &b, &c, &old] {
        db.insert_game(g).unwrap();
    }
    let clean_at = now().saturating_sub(Duration::from_secs(3600));
    db.record_audit_result(SlrVariant::Steamrt3, clean_at, "clean")
        .unwrap();
    let shared = SessionContext {
        slr_variant: Some(SlrVariant::Steamrt3),
        isolation: Some(RuntimeIsolationMode::Shared),
        ..SessionContext::default()
    };
    let mut k = 0;
    for (g, ctx, at) in [
        (&a, shared.clone(), now()),
        (&b, shared.clone(), now()),
        (
            &c,
            SessionContext {
                isolation: Some(RuntimeIsolationMode::Isolated),
                ..shared.clone()
            },
            now(),
        ),
        (
            &old,
            shared.clone(),
            clean_at.saturating_sub(Duration::from_secs(60)),
        ),
    ] {
        k += 1;
        let mut s = games::session(g, 1, k, at.saturating_sub(Duration::from_secs(10)));
        s.context = ctx;
        db.insert_session(&s).unwrap();
        db.close_session(&SessionClose {
            id: s.id,
            ended_at: at,
            duration_ms: 10_000,
            end_reason: easyplay_core::model::EndReason::Exited { code: 0 },
            exit_status: Some(0),
            counted: false,
            pieces: Vec::new(),
        })
        .unwrap();
    }
    db.set_audit_pending(SlrVariant::Steamrt3).unwrap();
    // A game changes a library inside the runtime.
    std::fs::write(
        t.dir.join("sniper_platform_3.0.1/files/lib/libx.so"),
        "evil",
    )
    .unwrap();
    let report = run_audit(&t, &m);
    let fs = RealFs::new();
    let action = handle_audit(
        &fs,
        &mut DbRecords(&mut db),
        &t.store,
        SlrVariant::Steamrt3,
        &report,
        now(),
    )
    .unwrap();
    let AuditAction::Quarantined {
        quarantine,
        suspects,
        ..
    } = &action
    else {
        panic!("{action:?}");
    };
    assert!(
        quarantine
            .join("sniper_platform_3.0.1/files/lib/libx.so")
            .exists()
    );
    assert!(!t.dir.exists());
    assert!(!t.store.manifest_file(SlrVariant::Steamrt3).exists());
    let names: Vec<&str> = suspects.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.contains(&a.name.as_str()) && names.contains(&b.name.as_str()));
    assert_eq!(
        action.notice().unwrap().code,
        easyplay_core::error::ErrorCode::RtSharedTampered
    );
    for (g, flagged) in [(&a, true), (&b, true), (&c, false), (&old, false)] {
        let row = db.game(g.id).unwrap().unwrap();
        assert_eq!(
            row.flags.contains(GameFlags::SUSPECTED_TAMPER),
            flagged,
            "{}",
            g.name
        );
    }
    let ev = db.runtime_events(10).unwrap();
    assert_eq!(ev[0].kind, "quarantined");
    assert!(ev[0].summary_json.contains("libx.so"));
    let st = db.audit_state(SlrVariant::Steamrt3).unwrap().unwrap();
    assert_eq!(st.last_result.as_deref(), Some("tampered"));
    assert!(!st.pending);
    // The next resolve needs a Provision of the variant.
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let svc = LocalRuntimeService::new(
        Arc::new(RealFs::new()),
        Arc::clone(&clock),
        Arc::new(MemCatalogStore::new()),
        t.store.clone(),
        t.data.join("home"),
        RuntimeSettings::default(),
        Arc::new(JobPool::new(JobPoolConfig::default(), clock).unwrap()),
    );
    svc.scan_now();
    assert!(matches!(
        svc.resolve(
            &RuntimeRef::Pinned(RuntimeName::new("GE-Proton10-25").unwrap()),
            None
        ),
        Resolution::NeedsProvision(ProvisionNeed::Slr {
            variant: SlrVariant::Steamrt3,
            ..
        })
    ));
    // Without a suspected session the message is "may be damaged".
    let t2 = tree();
    let m2 = build(&t2);
    let mut db2 = library(&t2);
    std::fs::remove_file(t2.dir.join("VERSIONS.txt")).unwrap();
    let a2 = handle_audit(
        &fs,
        &mut DbRecords(&mut db2),
        &t2.store,
        SlrVariant::Steamrt3,
        &run_audit(&t2, &m2),
        now(),
    )
    .unwrap();
    assert_eq!(
        a2.notice().unwrap().code,
        easyplay_core::error::ErrorCode::RtDamaged
    );
}

// ---- garbage collection (RT-013) ---------------------------------------------------------

fn catalog_of(
    t: &Tree,
    extra_external: Option<&Path>,
) -> Arc<easyplay_core::runtime::RuntimeCatalog> {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let svc = LocalRuntimeService::new(
        Arc::new(RealFs::new()),
        Arc::clone(&clock),
        Arc::new(MemCatalogStore::new()),
        t.store.clone(),
        t.data.join("home"),
        RuntimeSettings {
            extra_search_dirs: extra_external
                .map(|p| vec![p.to_path_buf()])
                .unwrap_or_default(),
            ..RuntimeSettings::default()
        },
        Arc::new(JobPool::new(JobPoolConfig::default(), clock).unwrap()),
    );
    svc.scan_now()
}

#[test]
fn gc_never_removes_external_newest_or_used_builds() {
    let t = tree();
    let ext = t.data.join("ext");
    let e = ext.join("proton-x");
    std::fs::create_dir_all(&e).unwrap();
    std::fs::copy(
        t.store.proton_dir("GE-Proton10-9").join("toolmanifest.vdf"),
        e.join("toolmanifest.vdf"),
    )
    .unwrap();
    std::fs::write(e.join("proton"), "x").unwrap();
    let catalog = catalog_of(&t, Some(&ext));
    assert_eq!(catalog.builds.len(), 3, "{:?}", catalog.rejected);
    let later = SystemClock::new()
        .now()
        .saturating_add(Duration::from_secs(30 * 86_400));
    let none = |_: &RuntimeName| Vec::new();
    let c = gc::candidates(&catalog, &none, later);
    let keep = |n: &str| {
        c.iter()
            .find(|c| c.name.as_str() == n)
            .unwrap()
            .keep
            .clone()
    };
    assert_eq!(keep("proton-x"), Some(gc::KeepReason::External));
    assert_eq!(keep("GE-Proton10-25"), Some(gc::KeepReason::Newest));
    assert_eq!(keep("GE-Proton10-9"), None);
    // Recently discovered: not removable automatically, but by hand.
    let c = gc::candidates(&catalog, &none, SystemClock::new().now());
    let old = c
        .iter()
        .find(|c| c.name.as_str() == "GE-Proton10-9")
        .unwrap();
    assert_eq!(old.keep, Some(gc::KeepReason::RecentlyUsed));
    assert!(old.manually_removable());
    let fs = RealFs::new();
    let name = |n: &str| RuntimeName::new(n).unwrap();
    for (n, why) in [("proton-x", "never removed"), ("GE-Proton10-25", "newest")] {
        let e = gc::remove_build(&fs, &t.store, &catalog, &none, &name(n), later).unwrap_err();
        assert!(format!("{e:?}").contains(why), "{n}: {e:?}");
    }
    let used = |n: &RuntimeName| {
        if n.as_str() == "GE-Proton10-9" {
            vec!["Hollow Knight".to_owned()]
        } else {
            Vec::new()
        }
    };
    let e = gc::remove_build(
        &fs,
        &t.store,
        &catalog,
        &used,
        &name("GE-Proton10-9"),
        later,
    )
    .unwrap_err();
    assert!(format!("{e:?}").contains("Hollow Knight"));
    assert!(t.store.proton_dir("GE-Proton10-9").exists());
    let removed = gc::remove_build(
        &fs,
        &t.store,
        &catalog,
        &none,
        &name("GE-Proton10-9"),
        later,
    )
    .unwrap();
    assert!(!removed.exists());
    assert!(ext.join("proton-x").exists());
}

#[test]
fn gc_expires_quarantine_and_stale_session_dirs() {
    let t = tree();
    let fs = RealFs::new();
    let q_old = t.store.quarantine_dir(
        SlrVariant::Steamrt3,
        now().saturating_sub(Duration::from_secs(8 * 86_400)),
    );
    let q_new = t.store.quarantine_dir(
        SlrVariant::Steamrt4,
        now().saturating_sub(Duration::from_secs(86_400)),
    );
    for q in [&q_old, &q_new] {
        std::fs::create_dir_all(q.join("files")).unwrap();
        std::fs::write(q.join("files/x"), "x").unwrap();
    }
    assert_eq!(
        gc::expire_quarantine(&fs, &t.store, now()),
        vec![q_old.clone()]
    );
    assert!(q_new.exists());
    let active = games::session_id(1, 1);
    let stale = games::session_id(1, 2);
    for s in [active, stale] {
        std::fs::create_dir_all(t.store.session_var_dir(s).join("tmp")).unwrap();
    }
    assert_eq!(
        gc::sweep_session_var(&fs, &t.store, &[active]),
        vec![t.store.session_var_dir(stale)]
    );
    assert!(t.store.session_var_dir(active).exists());
}

#[test]
fn partial_installs_are_swept() {
    let t = tree();
    let fs = RealFs::new();
    let half = t.store.proton_dir("GE-Proton10-26");
    std::fs::create_dir_all(half.join("files")).unwrap();
    let v4 = t.store.slr_dir(SlrVariant::Steamrt4);
    std::fs::create_dir_all(v4.join("pressure-vessel")).unwrap();
    let staging = t.store.root.join(".adopt-steamrt3-1");
    std::fs::create_dir_all(&staging).unwrap();
    let mut swept = easyplay_core::runtime::provision::sweep_partial(&fs, &t.store);
    swept.sort();
    let mut want = vec![half, v4, staging];
    want.sort();
    assert_eq!(swept, want);
    assert!(fs.exists(&t.dir) && t.store.proton_dir("GE-Proton10-9").exists());
}

// ---- umu database (RT-014) ----------------------------------------------------------------

#[test]
fn umu_database_corpus_parses_quickly() {
    use easyplay_core::runtime::UmuDb;
    let mut csv = String::from(
        "TITLE,STORE,CODENAME,UMU_ID,COMMON ACRONYM (Optional),NOTE (Optional),EXE_STRINGS (Optional)\n",
    );
    let stores = ["steam", "gog", "egs", "humble", "ubisoft", "none"];
    for i in 0..1200u32 {
        let store = stores[(i % 6) as usize];
        let title = if i % 7 == 0 {
            format!("\"Game {i}: The \"\"Sequel\"\", Part {}\"", i % 3)
        } else {
            format!("Game {i}")
        };
        csv.push_str(&format!(
            "{title},{store},{},umu-{},g{i},,\n",
            100_000 + i,
            500 + i / 2
        ));
    }
    csv.push_str("broken row without enough fields\n");
    let t0 = std::time::Instant::now();
    let db = UmuDb::parse(&csv).unwrap();
    let took = t0.elapsed();
    assert!(took < Duration::from_millis(50), "{took:?}");
    assert_eq!(db.entries.len(), 1200);
    assert_eq!(db.skipped, 1);
    assert_eq!(
        db.exact_title("game 14 the sequel part 2").as_deref(),
        Some("umu-507")
    );
    assert_eq!(db.steam_app_id(Some("umu-500"), "x"), Some(100_000));
}

#[test]
fn umu_database_cache_reloads_on_change() {
    use easyplay_core::runtime::UmuDbCache;
    let t = tree();
    let fs = RealFs::new();
    let b = t.store.proton_dir("GE-Proton10-25");
    std::fs::create_dir_all(b.join("protonfixes")).unwrap();
    let f = b.join("protonfixes/umu-database.csv");
    std::fs::write(&f, "TITLE,STORE,CODENAME,UMU_ID\nCelia,steam,1,umu-1\n").unwrap();
    let cache = UmuDbCache::new();
    assert_eq!(
        cache.get(&fs, &b).unwrap().exact_title("celia").as_deref(),
        Some("umu-1")
    );
    std::fs::write(
        &f,
        "TITLE,STORE,CODENAME,UMU_ID\nCelia,steam,1,umu-2\nX,gog,2,umu-3\n",
    )
    .unwrap();
    assert_eq!(
        cache.get(&fs, &b).unwrap().exact_title("celia").as_deref(),
        Some("umu-2")
    );
    assert!(
        cache
            .get(&fs, &t.store.proton_dir("GE-Proton10-9"))
            .is_none()
    );
}

#[test]
fn interrupted_parts_are_kept_resumable() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("umu");
    std::fs::create_dir_all(cache.join("tmpab12cd34")).unwrap();
    std::fs::create_dir_all(cache.join("tmpempty")).unwrap();
    std::fs::write(
        cache.join("tmpab12cd34/UMU-Proton-10.0-4.tar.gz.parts"),
        "123",
    )
    .unwrap();
    std::fs::write(cache.join("tmpab12cd34/other"), "x").unwrap();
    std::fs::write(cache.join("keep.tar.gz"), "k").unwrap();
    let moved = easyplay_core::runtime::provision::rescue_parts(&RealFs::new(), &cache);
    assert_eq!(moved, vec![cache.join("UMU-Proton-10.0-4.tar.gz.parts")]);
    assert_eq!(std::fs::read(&moved[0]).unwrap(), b"123");
    assert!(!cache.join("tmpab12cd34").exists() && !cache.join("tmpempty").exists());
    assert!(cache.join("keep.tar.gz").exists());
}
