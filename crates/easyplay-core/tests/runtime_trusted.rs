//! `launch::LocalTrustedSteps` against a real library and store with `RowLauncher` playing
//! the supervisor (RT-009, RT-012, SVC-008), and the pristine-prefix invariant of RT-010 as
//! a property test.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use easyplay_core::clock::{Clock, SystemClock};
use easyplay_core::fsx::{Fs, RealFs};
use easyplay_core::ids::{RuntimeName, SessionId};
use easyplay_core::jobs::{CancelToken, JobPool, JobPoolConfig};
use easyplay_core::launch::{LocalTrustedSteps, TrustedProgress, TrustedSteps};
use easyplay_core::model::{
    Channel, EndReason, NetworkAccess, RuntimeIsolationMode, SessionKind, SlrVariant,
};
use easyplay_core::paths::AppPaths;
use easyplay_core::procfs::RealProc;
use easyplay_core::runtime::provision::{
    NextStep, PrefixState, StepInput, check_postconditions, interpret, next_step,
};
use easyplay_core::runtime::{
    LocalRuntimeService, MemCatalogStore, ProvisionNeed, Resolution, RuntimeSettings, RuntimeStore,
    UnavailableReason,
};
use easyplay_core::storage::{Db, GameRepo, MigrateOptions, RuntimeRepo, SessionRepo};
use easyplay_core::umu::UmuPhase;
use easyplay_testkit::TestEnv;
use easyplay_testkit::fakes::{RowLauncher, RowScript};
use easyplay_testkit::fixtures::games;
use proptest::prelude::*;

struct World {
    _env: TestEnv,
    paths: AppPaths,
    store: RuntimeStore,
    launcher: Arc<RowLauncher>,
    steps: LocalTrustedSteps,
}

fn world() -> World {
    let env = TestEnv::new().unwrap();
    let paths = env.paths();
    std::fs::create_dir_all(&paths.data_dir).unwrap();
    let mut db = Db::open_rw(&paths.db_file).unwrap();
    db.migrate(&MigrateOptions::default()).unwrap();
    let store = RuntimeStore::new(&paths);
    let launcher = Arc::new(RowLauncher::new(paths.db_file.clone()));
    let fs: Arc<dyn Fs> = Arc::new(RealFs::new());
    let steps = LocalTrustedSteps {
        launcher: launcher.clone(),
        fs: Arc::clone(&fs),
        clock: Arc::new(SystemClock::new()),
        proc: Arc::new(RealProc::new(fs)),
        paths: paths.clone(),
        store: store.clone(),
        prepare_online: true,
    };
    World {
        _env: env,
        paths,
        store,
        launcher,
        steps,
    }
}

fn plan(w: &World, kind: SessionKind) -> easyplay_core::launch::PlannedLaunch {
    let mut p = plan_for(&w.paths);
    p.plan.session.kind = kind;
    p
}

fn host_planned(
    w: &easyplay_testkit::fixtures::hosts::SandboxWorld,
) -> easyplay_core::launch::PlannedLaunch {
    let env = easyplay_core::launch::HostEnv {
        uid: 1000,
        gid: 1000,
        user_name: "sam",
        group_name: "sam",
        home: Path::new(easyplay_testkit::fixtures::hosts::HOME),
        paths: &w.paths,
        host_env: &w.host_env,
        install_id: easyplay_testkit::fixtures::hosts::INSTALL_ID,
        init_binary: &w.init,
        developer_mode: false,
        seccomp_available: true,
        verbose_log: false,
        global_env: &[],
        user_dirs: &w.user_dirs,
        other_game_roots: &w.other_roots,
        mounts: &w.mounts,
        fs: &w.fs,
    };
    easyplay_core::launch::plan_provision(
        &ProvisionNeed::Codename(Channel::UmuProton),
        &easyplay_core::runtime::RuntimeCatalog::default(),
        &w.host,
        &env,
        SessionId::generate(&SystemClock::new()),
    )
    .unwrap()
}

/// What umu leaves after a successful download: a build and a complete `steamrt3`.
fn install_files(store: &RuntimeStore) {
    let b = store.proton_dir("UMU-Proton-10.0-4");
    std::fs::create_dir_all(b.join("files")).unwrap();
    std::fs::write(
        b.join("toolmanifest.vdf"),
        "\"manifest\"\n{\n \"version\" \"2\"\n \"commandline\" \"/proton %verb%\"\n \"require_tool_appid\" \"1628350\"\n \"compatmanager_layer_name\" \"proton\"\n}\n",
    )
    .unwrap();
    std::fs::write(b.join("proton"), "#!/usr/bin/env python3\n").unwrap();
    let v = store.slr_dir(SlrVariant::Steamrt3);
    std::fs::create_dir_all(v.join("sniper_platform_3.0.1/files")).unwrap();
    std::fs::write(v.join("sniper_platform_3.0.1/files/.ref"), "").unwrap();
    std::fs::write(v.join("_v2-entry-point"), "#!/bin/sh\n").unwrap();
    std::os::unix::fs::symlink("_v2-entry-point", v.join("umu")).unwrap();
    std::fs::write(v.join("VERSIONS.txt"), "depot\t3.0.1\n").unwrap();
    std::fs::write(
        v.join("toolmanifest.vdf"),
        "\"manifest\"\n{\n \"commandline\" \"/_v2-entry-point --verb=%verb% --\"\n}\n",
    )
    .unwrap();
    std::fs::write(v.join(".installed.ok"), "ok\n").unwrap();
}

fn catalog(w: &World) -> Arc<easyplay_core::runtime::RuntimeCatalog> {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    LocalRuntimeService::new(
        Arc::new(RealFs::new()),
        Arc::clone(&clock),
        Arc::new(MemCatalogStore::new()),
        w.store.clone(),
        w.paths.data_dir.join("nohome"),
        RuntimeSettings::default(),
        Arc::new(JobPool::new(JobPoolConfig::default(), clock).unwrap()),
    )
    .scan_now()
}

fn launching() -> UmuPhase {
    UmuPhase::Launching {
        tool: "proton".into(),
        runtime: "sniper".into(),
    }
}

#[test]
fn provision_installs_and_the_catalogue_gains_the_build() {
    // RT-009 acceptance, fast path: stopped once umu reports Launching.
    let w = world();
    let need = ProvisionNeed::Codename(Channel::UmuProton);
    assert!(check_postconditions(&catalog(&w), &need).is_err());
    let store = w.store.clone();
    w.launcher.push(
        RowScript::new(
            vec![
                UmuPhase::DownloadingProton {
                    name: "UMU-Proton-10.0-4.tar.gz".into(),
                },
                launching(),
            ],
            EndReason::Exited { code: 0 },
        )
        .with_hook(move |_| install_files(&store))
        .linger(Duration::from_secs(30)),
    );
    let p = plan(&w, SessionKind::Provision);
    let mut phases = Vec::new();
    let t0 = Instant::now();
    let end = w
        .steps
        .run(
            &p,
            true,
            &mut |pr| {
                if let TrustedProgress::Phase(ph) = pr {
                    phases.push(ph);
                }
            },
            &CancelToken::new(),
        )
        .unwrap();
    assert!(
        t0.elapsed() < Duration::from_secs(10),
        "fast path stopped it"
    );
    assert!(end.launching_seen && end.fast_path_stopped);
    assert_eq!(end.reason, EndReason::StoppedByUser);
    assert_eq!(phases.len(), 2);
    interpret(&end).unwrap();
    let change = w.steps.after_install().unwrap();
    assert_eq!(change.manifests, vec![SlrVariant::Steamrt3]);
    let c = catalog(&w);
    assert_eq!(
        check_postconditions(&c, &need).unwrap(),
        vec![SlrVariant::Steamrt3]
    );
    assert!(
        c.by_name(&RuntimeName::new("UMU-Proton-10.0-4").unwrap())
            .is_some()
    );
    // The manifest and the install row are recorded; the audit is clean.
    let mut db = Db::open_rw(&w.paths.db_file).unwrap();
    let i = db.slr_installs().unwrap();
    assert_eq!(i.len(), 1);
    assert!(i[0].manifest_tree_sha256.is_some());
    assert_eq!(
        db.runtime_events(5).unwrap()[0].kind,
        "provisioned",
        "event row"
    );
    assert_eq!(
        w.steps
            .audit(SlrVariant::Steamrt3, &CancelToken::new())
            .unwrap(),
        easyplay_core::runtime::AuditAction::Clean
    );
    // A second after_install does not rebuild an unchanged manifest.
    assert!(w.steps.after_install().unwrap().manifests.is_empty());
}

#[test]
fn provision_failures() {
    let w = world();
    // Failure before Launching: a download interruption.
    w.launcher
        .push(RowScript::new(Vec::new(), EndReason::Exited { code: 1 }));
    let end = w
        .steps
        .run(
            &plan(&w, SessionKind::Provision),
            true,
            &mut |_| {},
            &CancelToken::new(),
        )
        .unwrap();
    assert_eq!(
        interpret(&end).unwrap_err().code,
        easyplay_core::error::ErrorCode::RtDownloadInterrupted
    );
    // No network: the log shows it (EP-RT-010).
    let p = plan(&w, SessionKind::Provision);
    let log = p.plan.paths.log_dir.clone();
    w.launcher.push(
        RowScript::new(Vec::new(), EndReason::Exited { code: 1 }).with_hook(move |_| {
            std::fs::create_dir_all(&log).unwrap();
            std::fs::write(
                log.join("game.log"),
                "urllib.error.URLError: <urlopen error [Errno -3] Temporary failure in name resolution>\n",
            )
            .unwrap();
        }),
    );
    let end = w
        .steps
        .run(&p, true, &mut |_| {}, &CancelToken::new())
        .unwrap();
    assert!(end.network_failure);
    assert_eq!(
        interpret(&end).unwrap_err().code,
        easyplay_core::error::ErrorCode::RtNotInstalledOffline
    );
    // Cancel: graceful stop, reported as cancelled.
    w.launcher.push(
        RowScript::new(Vec::new(), EndReason::Exited { code: 0 }).linger(Duration::from_secs(30)),
    );
    let cancel = CancelToken::new();
    let c2 = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        c2.cancel();
    });
    let end = w
        .steps
        .run(
            &plan(&w, SessionKind::Provision),
            true,
            &mut |_| {},
            &cancel,
        )
        .unwrap();
    assert!(end.cancelled);
    assert!(interpret(&end).is_err());
}

#[test]
fn download_progress_is_measured_from_the_cache() {
    // RT-012: a growing `*.parts` file in umu's cache becomes byte progress.
    let w = world();
    let cache = w.store.cache.clone();
    w.launcher.push(
        RowScript::new(Vec::new(), EndReason::Exited { code: 0 })
            .with_hook(move |_| {
                std::fs::create_dir_all(cache.join(".tmp-x")).unwrap();
                for i in 1..=4u64 {
                    std::fs::write(
                        cache.join("UMU-Proton.tar.gz.parts"),
                        vec![0u8; (i << 20) as usize],
                    )
                    .unwrap();
                    std::fs::write(cache.join(".tmp-x/runtime"), vec![0u8; 1024]).unwrap();
                    std::thread::sleep(Duration::from_millis(600));
                }
            })
            .linger(Duration::from_millis(600)),
    );
    let mut seen = Vec::new();
    w.steps
        .run(
            &plan(&w, SessionKind::Provision),
            true,
            &mut |p| {
                if let TrustedProgress::Bytes { bytes, .. } = p {
                    seen.push(bytes);
                }
            },
            &CancelToken::new(),
        )
        .unwrap();
    assert!(seen.len() >= 3, "{seen:?}");
    assert!(seen.windows(2).all(|w| w[0] <= w[1]));
    assert_eq!(*seen.last().unwrap(), (4 << 20) + 1024);
}

#[test]
fn provision_waits_for_shared_mode_sessions() {
    // SVC-008 acceptance: never during a Shared-mode game session.
    let w = world();
    let g = games::game(1, 1);
    let mut db = Db::open_rw(&w.paths.db_file).unwrap();
    db.insert_game(&g).unwrap();
    let mut s = games::session(&g, 1, 1, SystemClock::new().now());
    s.context.isolation = Some(RuntimeIsolationMode::Shared);
    s.context.slr_variant = Some(SlrVariant::Steamrt3);
    db.insert_session(&s).unwrap();
    let steps = Arc::new(w.steps);
    let p = plan_for(&w.paths);
    let s2 = Arc::clone(&steps);
    let t = std::thread::spawn(move || {
        let mut waited = false;
        let end = s2
            .run(
                &p,
                true,
                &mut |pr| waited |= pr == TrustedProgress::Waiting,
                &CancelToken::new(),
            )
            .unwrap();
        (end, waited)
    });
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        w.launcher.plans().is_empty(),
        "started during a Shared session"
    );
    db.close_session(&easyplay_core::storage::SessionClose {
        id: s.id,
        ended_at: SystemClock::new().now(),
        duration_ms: 1,
        end_reason: EndReason::Exited { code: 0 },
        exit_status: Some(0),
        counted: false,
        pieces: Vec::new(),
    })
    .unwrap();
    let (end, waited) = t.join().unwrap();
    assert!(waited);
    assert_eq!(w.launcher.plans().len(), 1);
    assert_eq!(end.reason, EndReason::Exited { code: 0 });
}

fn plan_for(paths: &AppPaths) -> easyplay_core::launch::PlannedLaunch {
    let host = easyplay_testkit::fixtures::hosts::SandboxWorld::worked_example();
    let mut p = host_planned(&host);
    let sid = SessionId::generate(&SystemClock::new());
    p.plan.session.id = sid;
    p.plan.paths.log_dir = paths.session_dir(sid);
    p.plan.paths.runtime_dir = paths.run_session_dir(sid);
    p
}

#[test]
fn prepare_records_the_build() {
    let w = world();
    let g = games::game(1, 1);
    Db::open_rw(&w.paths.db_file)
        .unwrap()
        .insert_game(&g)
        .unwrap();
    let b = RuntimeName::new("UMU-Proton-10.0-4").unwrap();
    w.steps.prepared(g.id, &b).unwrap();
    w.steps.launched(g.id, &b).unwrap();
    let row = Db::open_rw(&w.paths.db_file)
        .unwrap()
        .game(g.id)
        .unwrap()
        .unwrap();
    assert_eq!(row.prefix_prepared_with.as_ref(), Some(&b));
    assert_eq!(row.runtime_last_resolved.as_ref(), Some(&b));
}

// ---- RT-010: the pristine-prefix rule --------------------------------------------------

fn resolution(k: u8) -> Resolution {
    match k % 4 {
        0 => Resolution::Ready(
            easyplay_testkit::fixtures::hosts::SandboxWorld::worked_example().runtime,
        ),
        1 => Resolution::NeedsProvision(ProvisionNeed::Codename(Channel::GeProton)),
        2 => Resolution::NeedsProvision(ProvisionNeed::Slr {
            variant: SlrVariant::Steamrt4,
            build: RuntimeName::new("proton-cachyos-11.0").unwrap(),
        }),
        _ => Resolution::Unavailable(UnavailableReason::ExternalInvalid("/x".into())),
    }
}

proptest! {
    #[test]
    fn no_network_step_on_a_tainted_prefix_without_consent(
        k in 0u8..4,
        exists: bool,
        tainted: bool,
        prepare_online: bool,
        consent: bool,
        online_policy: bool,
        external: bool,
    ) {
        let r = resolution(k);
        let game_network = if online_policy { NetworkAccess::Full } else { NetworkAccess::None };
        let step = next_step(&StepInput {
            resolution: &r,
            prefix: PrefixState { exists, tainted },
            prepare_online,
            game_network,
            consent,
            slr_build_external: external,
        });
        if let NextStep::Prepare { network, codename } = &step {
            if *network == NetworkAccess::Full && tainted {
                // Only with a recorded consent, or the game's own policy (Online games).
                prop_assert!(consent || online_policy);
            }
            if !prepare_online && !tainted {
                prop_assert_eq!(*network, game_network);
            }
            // A download inside Prepare always has the network.
            if codename.is_some() {
                prop_assert_eq!(*network, NetworkAccess::Full);
                prop_assert!(!tainted);
            }
        }
        // An Offline game with a tainted prefix and no consent never gets any network step
        // that mounts its prefix.
        if tainted && !consent && !online_policy {
            let networked_prepare =
                matches!(step, NextStep::Prepare { network: NetworkAccess::Full, .. });
            prop_assert!(!networked_prepare);
        }
    }
}
