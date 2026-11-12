//! Layer-1/2 behaviour through the testkit fakes.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use easyplay_core::clock::{Clock, Timestamp};
use easyplay_core::fsx::{
    FsErrorKind, FsOp, MountTable, WalkControl, WalkLimits, WalkStop, ensure_private_dir, safe_walk,
};
use easyplay_core::ids::{GameId, SessionId};
use easyplay_core::jobs::{JobLabel, JobPool, JobPoolConfig, JobPriority};
use easyplay_core::paths::AppPaths;
use easyplay_core::procfs::{ProcSource, RealProc};
use easyplay_testkit::{FakeClock, MemFs, TestEnv};

fn unix_millis(secs: u64) -> i64 {
    i64::try_from(secs).unwrap() * 1000
}

#[test]
fn ids_take_their_time_from_the_clock() {
    let clock = FakeClock::baseline();
    let a = GameId::generate(&clock);
    clock.advance(Duration::from_secs(1));
    let b = GameId::generate(&clock);
    assert!(a < b);
    let (secs, _) = b.as_uuid().get_timestamp().unwrap().to_unix();
    assert_eq!(unix_millis(secs), clock.now().as_millis());
}

#[test]
fn walk_deadline_uses_the_monotonic_clock() {
    let fs = MemFs::new();
    for i in 0..10 {
        fs.file(format!("/g/f{i}"), b"x");
    }
    let clock = FakeClock::baseline();
    let limits = WalkLimits::with_budget(&clock, Duration::from_secs(5));
    // Wall-clock jumps do not matter; the monotonic clock passing the deadline does.
    clock.set_wall(Timestamp::EPOCH);
    let mut n = 0;
    let s = safe_walk(&fs, Path::new("/g"), limits, &clock, |_| {
        n += 1;
        if n == 3 {
            clock.advance_mono(Duration::from_secs(6));
        }
        WalkControl::Continue
    })
    .unwrap();
    assert_eq!(s.stopped, Some(WalkStop::Deadline));
    assert_eq!(s.entries, 3);
}

#[test]
fn runtime_fallback_directory_is_verified() {
    let fs = MemFs::new();
    fs.dir("/tmp");
    ensure_private_dir(&fs, Path::new("/tmp/easyplay-1000"), 1000).unwrap();
    fs.fail(FsOp::CreateDir, "/tmp/other", FsErrorKind::PermissionDenied);
    assert_eq!(
        ensure_private_dir(&fs, Path::new("/tmp/other"), 1000)
            .unwrap_err()
            .kind,
        FsErrorKind::PermissionDenied
    );
    // Someone pre-created a symlink at the fallback path.
    fs.link("/tmp/easyplay-1001", "/home/victim");
    assert_eq!(
        ensure_private_dir(&fs, Path::new("/tmp/easyplay-1001"), 1001)
            .unwrap_err()
            .kind,
        FsErrorKind::Insecure
    );
}

#[test]
fn mount_table_from_memfs() {
    let fs = MemFs::new();
    fs.mount("/mnt/games", 9, "xfs")
        .mount("/mnt/nas", 10, "nfs4");
    fs.file("/proc/self/mountinfo", fs.mountinfo());
    let t = MountTable::read(&fs).unwrap();
    assert_eq!(t.fs_type_of(Path::new("/mnt/games/x")), Some("xfs"));
    assert!(
        t.mount_point_of(Path::new("/mnt/nas/y"))
            .unwrap()
            .is_network_fs()
    );
    assert!(!t.is_same_fs(Path::new("/mnt/games"), Path::new("/home")));
}

#[test]
fn procfs_through_memfs() {
    let fs = Arc::new(MemFs::new());
    fs.file(
        "/proc/42/stat",
        "42 (Hollow Knight.e) S 7 42 42 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 5555 0 0",
    )
    .link("/proc/42/ns/pid", "pid:[4026533244]")
    .file("/proc/42/cmdline", b"Z:\\game.exe\0-windowed\0")
    .file(
        "/proc/sys/kernel/random/boot_id",
        "8a6b2c4e-0000-4000-8000-000000000000\n",
    );
    let p = RealProc::new(fs);
    let e = p.entry(42).unwrap().unwrap();
    assert_eq!(e.comm, "Hollow Knight.e");
    assert_eq!(e.pid_ns, Some(4_026_533_244));
    assert_eq!(e.start_time_ticks, 5555);
    assert_eq!(p.cmdline(42).unwrap().unwrap().len(), 2);
    assert_eq!(p.pids().unwrap(), vec![42]);
    assert_eq!(p.boot_id().unwrap(), "8a6b2c4e-0000-4000-8000-000000000000");
    assert_eq!(p.entry(43).unwrap(), None);
}

#[test]
fn test_env_paths() {
    let env = TestEnv::new().unwrap();
    let paths: AppPaths = env.paths();
    let clock = FakeClock::baseline();
    let sid = SessionId::generate(&clock);
    assert!(paths.plan_file(sid).starts_with(env.run()));
    assert!(paths.app_log_file.starts_with(env.state()));
}

#[test]
fn job_ids_come_from_the_injected_clock() {
    let clock = Arc::new(FakeClock::baseline());
    let pool = JobPool::new(
        JobPoolConfig {
            workers: 2,
            ..JobPoolConfig::default()
        },
        clock.clone(),
    )
    .unwrap();
    let h = pool.submit(JobPriority::Normal, JobLabel("t"), |_| 1);
    let (secs, _) = h.id().as_uuid().get_timestamp().unwrap().to_unix();
    assert_eq!(unix_millis(secs), clock.now().as_millis());
    assert_eq!(h.wait(), Ok(1));
}
