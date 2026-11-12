//! Repository round trips, constraints and concurrency (TESTING_STRATEGY §5).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use easyplay_core::clock::Timestamp;
use easyplay_core::ids::{RuntimeName, TagName};
use easyplay_core::model::{
    ArtOrigin, ArtSlot, ArtworkRef, EndReason, ExeRole, GameKey, Preset, RuntimeBuild,
    RuntimeFamily, RuntimeOrigin, SessionKind, SessionState, SlrVariant, TagColor,
};
use easyplay_core::storage::{
    ArtworkRepo, AuditState, CloseOutcome, Db, ExecutableRepo, GamePatch, GameRepo, KvRepo,
    MetaKey, MigrateOptions, RuntimeEvent, RuntimeRepo, SessionClose, SessionRepo, SlrInstall,
    StatsRepo, StorageError, Store, TagRepo,
};
use easyplay_testkit::fixtures::games::{self, BASE_MS};

fn t(ms: i64) -> Timestamp {
    Timestamp::from_millis(BASE_MS + ms)
}

fn mem() -> Db {
    Db::open_in_memory().unwrap()
}

fn file_db(dir: &Path) -> (Db, PathBuf) {
    let path = dir.join("library.db");
    let mut db = Db::open_rw(&path).unwrap();
    db.migrate(&MigrateOptions::default()).unwrap();
    (db, path)
}

#[test]
fn games_round_trip_with_executables() {
    let mut db = mem();
    let mut g = games::game(1, 3);
    g.store = Some(easyplay_core::model::Store::Gog);
    g.runtime =
        easyplay_core::model::RuntimeRef::Pinned(RuntimeName::new("GE-Proton10-25").unwrap());
    g.runtime_last_resolved = Some(RuntimeName::new("GE-Proton10-25").unwrap());
    g.flags = easyplay_core::model::GameFlags::X11_NOTICE_ACK;
    db.insert_game(&g).unwrap();
    let back = db.game(g.id).unwrap().unwrap();
    assert_eq!(back, g);
    assert_eq!(db.game_count().unwrap(), 1);
    assert_eq!(db.executables_of(g.id).unwrap(), g.executables);
    let (gid, exe) = db
        .executable_by_canonical_path(&g.executables[1].canonical_path)
        .unwrap()
        .unwrap();
    assert_eq!((gid, &exe), (g.id, &g.executables[1]));
    let by_hash = db
        .executables_by_quick_hash(&g.executables[0].fingerprint.quick_hash)
        .unwrap();
    assert_eq!(by_hash, vec![(g.id, g.executables[0].clone())]);
}

#[test]
fn non_utf8_paths_survive() {
    use std::os::unix::ffi::OsStringExt;
    let mut db = mem();
    let mut g = games::game(2, 1);
    let weird = PathBuf::from(std::ffi::OsString::from_vec(
        b"/games/\xff\xfe/x.exe".to_vec(),
    ));
    g.executables[0].path = weird.clone();
    g.executables[0].canonical_path = weird.clone();
    g.root = Some(PathBuf::from(std::ffi::OsString::from_vec(
        b"/games/\xff\xfe".to_vec(),
    )));
    db.insert_game(&g).unwrap();
    assert_eq!(db.game(g.id).unwrap().unwrap(), g);
    assert!(db.executable_by_canonical_path(&weird).unwrap().is_some());
}

#[test]
fn constraint_violations_are_typed() {
    let mut db = mem();
    let a = games::game(1, 1);
    db.insert_game(&a).unwrap();

    // Duplicate canonical path in another game.
    let mut b = games::game(2, 1);
    b.executables[0].canonical_path = a.executables[0].canonical_path.clone();
    assert!(matches!(
        db.insert_game(&b),
        Err(StorageError::DuplicateExecutablePath)
    ));
    // Nothing of `b` was written.
    assert!(db.game_record(b.id).unwrap().is_none());

    // A second primary.
    let mut second = games::executable(1, 5, "/games/0001");
    second.role = ExeRole::Primary;
    assert!(matches!(
        db.add_executable(a.id, &second),
        Err(StorageError::MultiplePrimaries)
    ));
    // Validation rejects a game without a primary before touching the database.
    let mut c = games::game(3, 2);
    c.executables[0].role = ExeRole::Alternate;
    assert!(matches!(db.insert_game(&c), Err(StorageError::Invalid(_))));
    // The primary cannot be removed; a tool can.
    assert!(db.remove_executable(a.executables[0].id).is_err());
    let tool = games::executable(1, 1, "/games/0001");
    db.add_executable(a.id, &tool).unwrap();
    assert!(db.remove_executable(tool.id).unwrap());
}

#[test]
fn executable_limit_and_primary_switch() {
    let mut db = mem();
    let g = games::game(1, 1);
    db.insert_game(&g).unwrap();
    for k in 1..32 {
        db.add_executable(g.id, &games::executable(1, k, "/games/0001"))
            .unwrap();
    }
    assert!(matches!(
        db.add_executable(g.id, &games::executable(1, 40, "/games/0001")),
        Err(StorageError::Limit { max: 32, .. })
    ));
    let new_primary = games::exe_id(1, 3);
    db.set_primary_executable(g.id, new_primary).unwrap();
    let exes = db.executables_of(g.id).unwrap();
    assert_eq!(exes[0].id, new_primary);
    assert_eq!(
        exes.iter().filter(|e| e.role == ExeRole::Primary).count(),
        1
    );
    assert_eq!(
        db.executable(g.executables[0].id).unwrap().unwrap().1.role,
        ExeRole::Alternate
    );
    let mut changed = exes[1].clone();
    changed.label = Some("Renamed".into());
    db.update_executable(&changed).unwrap();
    assert_eq!(
        db.executable(changed.id)
            .unwrap()
            .unwrap()
            .1
            .label
            .as_deref(),
        Some("Renamed")
    );
}

#[test]
fn patch_and_supervisor_columns() {
    let mut db = mem();
    let g = games::game(1, 1);
    db.insert_game(&g).unwrap();
    let patch = GamePatch {
        name: Some("The Hollow Knight".into()),
        favorite: Some(true),
        notes: Some("great".into()),
        umu_id: Some(Some("umu-367520".into())),
        ..GamePatch::default()
    };
    db.update_game(g.id, &patch, t(1000)).unwrap();
    db.set_prefix_tainted_at(g.id, Some(t(5))).unwrap();
    db.set_last_launched_at(g.id, t(6)).unwrap();
    let back = db.game(g.id).unwrap().unwrap();
    assert_eq!(back.name, "The Hollow Knight");
    assert_eq!(
        back.sort_name,
        easyplay_core::model::sort_name_of("The Hollow Knight")
    );
    assert!(back.favorite);
    assert_eq!(back.updated_at, t(1000));
    assert_eq!(back.policy_updated_at, g.policy_updated_at);
    assert_eq!(back.prefix_tainted_at, Some(t(5)));
    assert_eq!(back.last_launched_at, Some(t(6)));
    let bad = GamePatch {
        name: Some(String::new()),
        ..GamePatch::default()
    };
    assert!(matches!(
        db.update_game(g.id, &bad, t(2000)),
        Err(StorageError::Invalid(_))
    ));
    assert!(matches!(
        db.update_game(games::game_id(99), &patch, t(1)),
        Err(StorageError::NotFound { .. })
    ));
}

#[test]
fn soft_delete_hides_and_purge_keeps_history() {
    let mut db = mem();
    let g = games::game(1, 1);
    let other = games::game(2, 1);
    db.insert_game(&g).unwrap();
    db.insert_game(&other).unwrap();
    let s = games::session(&g, 1, 0, t(0));
    db.insert_session(&s).unwrap();
    db.close_session(&SessionClose {
        id: s.id,
        ended_at: t(60_000),
        duration_ms: 60_000,
        end_reason: EndReason::Exited { code: 0 },
        exit_status: Some(0),
        counted: true,
        pieces: games::pieces(t(0), 60_000),
    })
    .unwrap();

    assert!(db.set_game_deleted(g.id, Some(t(10))).unwrap());
    assert!(db.game(g.id).unwrap().is_none());
    assert_eq!(
        db.game_record(g.id).unwrap().unwrap().deleted_at,
        Some(t(10))
    );
    let lib = db.load_library().unwrap();
    assert_eq!(lib.games.len(), 1);
    assert!(lib.stats.iter().all(|s| s.game_id != g.id));
    assert_eq!(db.deleted_games().unwrap(), vec![g.id]);
    assert!(
        db.executables_by_quick_hash(&g.executables[0].fingerprint.quick_hash)
            .unwrap()
            .is_empty()
    );

    assert!(db.purge_game(g.id, true).unwrap());
    assert!(db.game_record(g.id).unwrap().is_none());
    let kept = db.session(s.id).unwrap().unwrap();
    assert_eq!(kept.game_id, None);
    assert_eq!(kept.game_name, g.name);
    let removed = GameKey::Removed(g.name.clone());
    assert_eq!(
        db.playtime_sum(Some(&removed), 0, 99_999_999).unwrap(),
        60_000
    );
    assert!(db.game_stats(g.id).unwrap().is_none());

    // Without history.
    let s2 = games::session(&other, 2, 0, t(0));
    db.insert_session(&s2).unwrap();
    db.purge_game(other.id, false).unwrap();
    assert!(db.session(s2.id).unwrap().is_none());
}

#[test]
fn tags_are_case_insensitive_limited_and_mergeable() {
    let mut db = mem();
    let a = games::game(1, 1);
    let b = games::game(2, 1);
    db.insert_game(&a).unwrap();
    db.insert_game(&b).unwrap();
    let name = |s: &str| TagName::new(s).unwrap();
    let c0 = TagColor::new(0).unwrap();
    let rpg = db.create_tag(&name("RPG"), c0, t(0)).unwrap();
    assert!(matches!(
        db.create_tag(&name("rpg"), c0, t(0)),
        Err(StorageError::DuplicateTag)
    ));
    // Unicode case folding, which COLLATE NOCASE would miss.
    db.create_tag(&name("Ärger"), c0, t(0)).unwrap();
    assert!(matches!(
        db.create_tag(&name("ärger"), c0, t(0)),
        Err(StorageError::DuplicateTag)
    ));
    let coop = db.create_tag(&name("Co-op"), c0, t(0)).unwrap();
    db.set_game_tags(a.id, &[rpg.id, coop.id]).unwrap();
    db.set_game_tags(b.id, &[coop.id]).unwrap();
    assert!(matches!(
        db.rename_tag(coop.id, &name("rpg")),
        Err(StorageError::DuplicateTag)
    ));
    db.rename_tag(coop.id, &name("Coop")).unwrap();
    db.set_tag_color(coop.id, TagColor::new(3).unwrap())
        .unwrap();
    let counts = db.tags_with_counts().unwrap();
    let coop_row = counts.iter().find(|(t, _)| t.id == coop.id).unwrap();
    assert_eq!(coop_row.1, 2);
    assert_eq!(coop_row.0.name.as_str(), "Coop");
    assert_eq!(coop_row.0.color.index(), 3);

    // Merge `Coop` into `RPG`: a keeps one link, b gains RPG.
    assert_eq!(db.merge_tags(coop.id, rpg.id).unwrap(), 2);
    assert_eq!(db.tags_of(a.id).unwrap(), vec![rpg.id]);
    assert_eq!(db.tags_of(b.id).unwrap(), vec![rpg.id]);
    assert!(db.tag_by_name(&name("coop")).unwrap().is_none());

    // 16 per game.
    let many: Vec<_> = (0..17)
        .map(|i| db.create_tag(&name(&format!("t{i}")), c0, t(0)).unwrap().id)
        .collect();
    assert!(matches!(
        db.set_game_tags(a.id, &many),
        Err(StorageError::Limit { max: 16, .. })
    ));
    db.set_game_tags(a.id, &many[..16]).unwrap();
    let skipped = db.add_tag_to_games(many[16], &[a.id, b.id]).unwrap();
    assert_eq!(skipped, vec![a.id]);

    // 200 in total.
    let existing = db.tags_with_counts().unwrap().len();
    for i in existing..200 {
        db.create_tag(&name(&format!("x{i}")), c0, t(0)).unwrap();
    }
    assert!(matches!(
        db.create_tag(&name("one too many"), c0, t(0)),
        Err(StorageError::Limit { max: 200, .. })
    ));

    // Cascade on game delete; delete_tag reports the count.
    db.purge_game(a.id, true).unwrap();
    assert_eq!(db.delete_tag(rpg.id).unwrap(), 1);
}

#[test]
fn artwork_upsert_delete_and_unreferenced() {
    let mut db = mem();
    let g = games::game(1, 1);
    db.insert_game(&g).unwrap();
    let art = |slot, h: u8| ArtworkRef {
        game_id: g.id,
        slot,
        file: format!(
            "{}-{h:02x}.png",
            match slot {
                ArtSlot::Cover => "cover",
                ArtSlot::Icon => "icon",
                ArtSlot::Hero => "hero",
            }
        ),
        hash: [h; 32],
        width: 600,
        height: 900,
        source: ArtOrigin::File,
        updated_at: t(0),
    };
    db.upsert_artwork(&art(ArtSlot::Cover, 1)).unwrap();
    db.upsert_artwork(&art(ArtSlot::Icon, 2)).unwrap();
    db.upsert_artwork(&art(ArtSlot::Cover, 3)).unwrap();
    let rows = db.artwork_of(g.id).unwrap();
    assert_eq!(rows, vec![art(ArtSlot::Cover, 3), art(ArtSlot::Icon, 2)]);
    assert_eq!(
        db.unreferenced_artwork(&[[1; 32], [2; 32], [3; 32]])
            .unwrap(),
        vec![[1; 32]]
    );
    assert!(db.delete_artwork(g.id, ArtSlot::Icon).unwrap());
    assert!(!db.delete_artwork(g.id, ArtSlot::Icon).unwrap());
    let mut bad = art(ArtSlot::Hero, 4);
    bad.file = "../escape.png".into();
    assert!(db.upsert_artwork(&bad).is_err());
    db.purge_game(g.id, false).unwrap();
    assert!(db.referenced_artwork_hashes().unwrap().is_empty());
}

fn close(
    id: easyplay_core::ids::SessionId,
    start: Timestamp,
    d: u64,
    counted: bool,
) -> SessionClose {
    SessionClose {
        id,
        ended_at: start.saturating_add(Duration::from_millis(d)),
        duration_ms: d,
        end_reason: if counted {
            EndReason::Exited { code: 0 }
        } else {
            EndReason::FailedToStart {
                code: easyplay_core::ErrorCode::SesInterrupted,
            }
        },
        exit_status: None,
        counted,
        pieces: games::pieces(start, d),
    }
}

#[test]
fn sessions_lifecycle_aggregates_and_paging() {
    let mut db = mem();
    let g = games::game(1, 1);
    db.insert_game(&g).unwrap();
    // Starts 1 h before midnight UTC and runs 2 h: split over two days.
    let day0 = (BASE_MS / 86_400_000 + 1) * 86_400_000 - 3_600_000;
    let start = Timestamp::from_millis(day0);
    let mut s = games::session(&g, 1, 0, start);
    s.supervisor = Some(easyplay_core::model::SupervisorIdent {
        pid: 4242,
        start_ticks: 99,
        boot_id: "boot".into(),
    });
    s.context.runtime_name = Some(RuntimeName::new("UMU-Proton-9.0").unwrap());
    s.context.slr_variant = Some(SlrVariant::Steamrt3);
    s.context.preset = Some(Preset::Custom {
        base: easyplay_core::model::BasePreset::Online,
    });
    db.insert_session(&s).unwrap();
    assert_eq!(db.session(s.id).unwrap().unwrap(), s);
    assert!(db.set_session_state(s.id, SessionState::Running).unwrap());
    assert!(
        db.set_session_sandbox(s.id, Some(77), Some(4_026_531_836))
            .unwrap()
    );
    assert!(
        db.heartbeat(s.id, 30_000, start.saturating_add(Duration::from_secs(30)))
            .unwrap()
    );
    assert_eq!(db.open_sessions().unwrap().len(), 1);
    let ev = db
        .add_session_event(s.id, start, "phase", "{\"p\":1}")
        .unwrap();
    assert!(ev > 0);
    assert_eq!(db.session_events(s.id).unwrap()[0].kind, "phase");

    let out = db
        .close_session(&close(s.id, start, 7_200_000, true))
        .unwrap();
    assert_eq!(
        out,
        CloseOutcome::Closed {
            game_id: Some(g.id),
            counted: true
        }
    );
    assert_eq!(
        db.close_session(&close(s.id, start, 7_200_000, true))
            .unwrap(),
        CloseOutcome::AlreadyClosed
    );
    assert!(!db.heartbeat(s.id, 1, start).unwrap());
    let ended = db.session(s.id).unwrap().unwrap();
    assert_eq!(ended.state, SessionState::Ended);
    assert_eq!(ended.end_reason, Some(EndReason::Exited { code: 0 }));
    assert!(db.open_sessions().unwrap().is_empty());

    let stats = db.game_stats(g.id).unwrap().unwrap();
    assert_eq!(
        (stats.total_ms, stats.sessions, stats.longest_ms),
        (7_200_000, 1, 7_200_000)
    );
    assert_eq!(stats.first_played_at, Some(start));
    let d0 = games::utc_day(start);
    let d1 = games::utc_day(start.saturating_add(Duration::from_secs(7200)));
    let days = db
        .playtime_range(Some(&GameKey::Game(g.id)), d0, d1)
        .unwrap();
    assert_eq!(
        days.iter().map(|d| d.ms).collect::<Vec<_>>(),
        vec![3_600_000, 3_600_000]
    );
    assert_eq!(days.iter().map(|d| d.sessions).sum::<u32>(), 1);
    assert_eq!(db.playtime_sum(None, d0, d1).unwrap(), 7_200_000);
    assert_eq!(db.daily_totals(d0, d0).unwrap()[0].ms, 3_600_000);
    assert_eq!(
        db.game(g.id).unwrap().unwrap().last_launched_at,
        Some(start)
    );

    // A failed launch counts only as a failure.
    let f = games::session(&g, 1, 1, start.saturating_add(Duration::from_secs(9000)));
    db.insert_session(&f).unwrap();
    db.close_session(&close(f.id, f.started_at, 1000, false))
        .unwrap();
    let stats = db.game_stats(g.id).unwrap().unwrap();
    assert_eq!((stats.sessions, stats.failed_launches), (1, 1));

    // Paging, newest first.
    for k in 2..12 {
        let x = games::session(
            &g,
            1,
            k,
            start.saturating_add(Duration::from_secs(10_000 + u64::from(k))),
        );
        db.insert_session(&x).unwrap();
    }
    let page1 = db.session_history(Some(g.id), &[], 0, 5).unwrap();
    let page2 = db.session_history(Some(g.id), &[], 5, 5).unwrap();
    let page3 = db
        .session_history(Some(g.id), &[SessionKind::Game], 10, 5)
        .unwrap();
    assert_eq!((page1.len(), page2.len(), page3.len()), (5, 5, 2));
    assert!(page1[0].started_at > page2[0].started_at);
    assert_eq!(page3.last().unwrap().id, s.id);
    assert!(
        db.session_history(None, &[SessionKind::Tool], 0, 50)
            .unwrap()
            .is_empty()
    );

    // Deleting the counted session removes its aggregates.
    assert!(
        db.delete_session(s.id, &games::pieces(start, 7_200_000))
            .unwrap()
    );
    let stats = db.game_stats(g.id).unwrap().unwrap();
    assert_eq!(
        (stats.total_ms, stats.sessions, stats.longest_ms),
        (0, 0, 0)
    );
    assert_eq!(stats.first_played_at, None);
    assert!(db.playtime_range(None, 0, 99_999_999).unwrap().is_empty());
    assert_eq!(db.prune_session_events(t(i64::MAX / 4)).unwrap(), 0);
}

#[test]
fn two_concurrent_closers_apply_aggregates_once() {
    let dir = tempfile::tempdir().unwrap();
    let (mut db, path) = file_db(dir.path());
    let g = games::game(1, 1);
    db.insert_game(&g).unwrap();
    let sessions: Vec<_> = (0..20)
        .map(|k| games::session(&g, 1, k, t(i64::from(k) * 10_000_000)))
        .collect();
    for s in &sessions {
        db.insert_session(s).unwrap();
    }
    db.close().unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let sessions = sessions.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut db = Db::open_rw(&path).unwrap();
                barrier.wait();
                let mut closed = 0;
                for s in &sessions {
                    if let CloseOutcome::Closed { .. } = db
                        .close_session(&close(s.id, s.started_at, 60_000, true))
                        .unwrap()
                    {
                        closed += 1;
                    }
                    // Interleave heartbeats on other rows.
                    let _ = db.heartbeat(s.id, 5, s.started_at).unwrap();
                }
                closed
            })
        })
        .collect();
    let total: u32 = handles.into_iter().map(|h| h.join().unwrap()).sum();
    assert_eq!(total, 20);
    let mut db = Db::open_rw(&path).unwrap();
    let stats = db.game_stats(g.id).unwrap().unwrap();
    assert_eq!(stats.sessions, 20);
    assert_eq!(stats.total_ms, 20 * 60_000);
    assert_eq!(db.playtime_sum(None, 0, 99_999_999).unwrap(), 20 * 60_000);
}

#[test]
fn runtimes_and_kv_round_trip() {
    let mut db = mem();
    let build = |name: &str, key: &str| RuntimeBuild {
        name: RuntimeName::new(name).unwrap(),
        family: RuntimeFamily::GeProton,
        version_key: key.into(),
        origin: RuntimeOrigin::Store,
        origin_label: "EasyPlay".into(),
        path: PathBuf::from(format!("/data/runtimes/{name}")),
        required_slr: SlrVariant::Steamrt3,
        display_name: name.into(),
        has_fixes: true,
        size_bytes: Some(1 << 30),
        discovered_at: t(0),
        last_used_at: None,
    };
    let a = build("GE-Proton10-25", "0010.0025");
    let b = build("GE-Proton9-27", "0009.0027");
    db.replace_runtime_catalog(&[b.clone(), a.clone()]).unwrap();
    assert_eq!(db.runtimes().unwrap(), vec![a.clone(), b.clone()]);
    db.replace_runtime_catalog(std::slice::from_ref(&a))
        .unwrap();
    assert_eq!(db.runtimes().unwrap().len(), 1);
    assert!(db.set_runtime_last_used(&a.name, t(5)).unwrap());
    assert_eq!(
        db.runtime(&a.name).unwrap().unwrap().last_used_at,
        Some(t(5))
    );
    let mut dup = build("Other", "0001");
    dup.path = a.path.clone();
    assert!(matches!(
        db.replace_runtime_catalog(&[a.clone(), dup]),
        Err(StorageError::DuplicateRuntimePath)
    ));
    assert_eq!(
        db.runtimes().unwrap().len(),
        1,
        "failed replace rolled back"
    );

    let slr = SlrInstall {
        variant: SlrVariant::Steamrt3,
        version: "3.0.20261101".into(),
        path: PathBuf::from("/data/runtimes/umu/steamrt3"),
        platform_dir: PathBuf::from("sniper_platform_3.0"),
        manifest_tree_sha256: Some([9; 32]),
        manifest_built_at: Some(t(1)),
        size_bytes: None,
    };
    db.put_slr_install(&slr).unwrap();
    db.put_slr_install(&slr).unwrap();
    assert_eq!(db.slr_installs().unwrap(), vec![slr]);
    db.set_audit_pending(SlrVariant::Steamrt3).unwrap();
    assert!(
        db.audit_state(SlrVariant::Steamrt3)
            .unwrap()
            .unwrap()
            .pending
    );
    db.record_audit_result(SlrVariant::Steamrt3, t(7), "clean")
        .unwrap();
    db.record_audit_result(SlrVariant::Steamrt3, t(8), "tampered")
        .unwrap();
    assert_eq!(
        db.audit_state(SlrVariant::Steamrt3).unwrap().unwrap(),
        AuditState {
            variant: SlrVariant::Steamrt3,
            pending: false,
            last_run_at: Some(t(8)),
            last_clean_at: Some(t(7)),
            last_result: Some("tampered".into()),
        }
    );
    assert!(
        db.record_audit_result(SlrVariant::Steamrt3, t(9), "bogus")
            .is_err()
    );
    let id = db
        .add_runtime_event(&RuntimeEvent {
            id: 0,
            at: t(3),
            variant: Some(SlrVariant::Steamrt3),
            kind: "provisioned".into(),
            summary_json: "{}".into(),
        })
        .unwrap();
    assert_eq!(db.runtime_events(10).unwrap()[0].id, id);
    assert!(db.delete_slr_install(SlrVariant::Steamrt3).unwrap());

    db.kv_set("library.sort.all", "name", t(0)).unwrap();
    db.kv_set("library.sort.recent", "played", t(0)).unwrap();
    db.kv_set("library.view", "grid", t(0)).unwrap();
    db.kv_set("library.view", "list", t(1)).unwrap();
    db.kv_set("LIBRARY.SORT.x", "case", t(1)).unwrap();
    assert_eq!(db.kv_get("library.view").unwrap().as_deref(), Some("list"));
    assert_eq!(db.kv_with_prefix("library.sort.").unwrap().len(), 2);
    assert!(db.kv_delete("library.view").unwrap());
    assert!(db.kv_get("library.view").unwrap().is_none());
    db.meta_set(MetaKey::LastBackupAt, "123").unwrap();
    assert_eq!(
        db.meta_get(MetaKey::LastBackupAt).unwrap().as_deref(),
        Some("123")
    );
}

#[test]
fn read_only_handle_reads_but_never_writes() {
    let dir = tempfile::tempdir().unwrap();
    let (mut db, path) = file_db(dir.path());
    let g = games::game(1, 1);
    db.insert_game(&g).unwrap();
    let mut ro = Db::open_ro(&path).unwrap();
    assert!(ro.is_read_only());
    assert_eq!(ro.load_library().unwrap().games.len(), 1);
    assert!(matches!(
        ro.kv_set("a", "b", t(0)),
        Err(StorageError::ReadOnly)
    ));
    assert!(matches!(
        ro.insert_game(&games::game(2, 1)),
        Err(StorageError::ReadOnly)
    ));
    // data_version changes when the other connection commits.
    let before = ro.data_version().unwrap();
    db.kv_set("x", "y", t(0)).unwrap();
    assert_ne!(ro.data_version().unwrap(), before);
}

#[test]
fn transactions_group_repository_calls() {
    let mut db = mem();
    let g = games::game(1, 1);
    let r: Result<(), StorageError> = db.transaction(|tx| {
        tx.insert_game(&g)?;
        tx.kv_set("k", "v", t(0))?;
        Err(StorageError::Busy)
    });
    assert!(r.is_err());
    assert!(db.game_record(g.id).unwrap().is_none());
    assert!(db.kv_get("k").unwrap().is_none());
    db.transaction(|tx| {
        tx.insert_game(&g)?;
        tx.kv_set("k", "v", t(0))
    })
    .unwrap();
    assert!(db.game(g.id).unwrap().is_some());
    assert!(!db.in_transaction());
}

#[test]
fn populate_generates_a_consistent_library() {
    let mut db = mem();
    let p = games::populate(&mut db, 40, 6, 7);
    let lib = db.load_library().unwrap();
    assert_eq!(lib.games.len(), 40);
    assert_eq!(lib.tags.len(), 5);
    let total: u64 = lib.stats.iter().map(|s| s.total_ms).sum();
    assert_eq!(total, p.counted_ms);
    assert_eq!(db.playtime_sum(None, 0, 99_999_999).unwrap(), p.counted_ms);
    // Sorted by sort_name.
    let names: Vec<_> = lib.games.iter().map(|g| g.sort_name.clone()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}
