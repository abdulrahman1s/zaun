//! `RealFs` and helper tests against real temporary trees.

#![allow(clippy::disallowed_methods)]

use std::cell::Cell;
use std::fs;
use std::io::Read;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::time::Duration;

use super::*;
use crate::clock::{Clock, MonoInstant, Timestamp};

/// A clock whose monotonic time advances by one second on every read.
struct Ticking(std::sync::atomic::AtomicU64);
impl Clock for Ticking {
    fn now(&self) -> Timestamp {
        Timestamp::EPOCH
    }
    fn mono(&self) -> MonoInstant {
        let n = self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        MonoInstant::from_duration(Duration::from_secs(n))
    }
}

fn ticking() -> Ticking {
    Ticking(std::sync::atomic::AtomicU64::new(0))
}

/// root/{a.txt, sub/{b.txt, deep/{c.txt}}, link_out -> /etc, loop -> ., dirlink -> sub}
fn tree() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    fs::write(r.join("a.txt"), b"hello").unwrap();
    fs::create_dir_all(r.join("sub/deep")).unwrap();
    fs::write(r.join("sub/b.txt"), b"b").unwrap();
    fs::write(r.join("sub/deep/c.txt"), b"c").unwrap();
    symlink("/etc", r.join("link_out")).unwrap();
    symlink(".", r.join("loop")).unwrap();
    symlink("sub", r.join("dirlink")).unwrap();
    t
}

#[test]
fn metadata_does_not_follow() {
    let t = tree();
    let fs_ = RealFs::new();
    let md = fs_.metadata(&t.path().join("link_out")).unwrap();
    assert!(md.is_symlink());
    let md = fs_.metadata_follow(&t.path().join("dirlink")).unwrap();
    assert!(md.is_dir());
    let md = fs_.metadata(&t.path().join("a.txt")).unwrap();
    assert!(md.is_file());
    assert_eq!(md.size, 5);
    assert!(md.mtime_ns > 0);
    assert_eq!(
        fs_.read_link(&t.path().join("link_out")).unwrap(),
        Path::new("/etc")
    );
    assert_eq!(
        fs_.metadata(&t.path().join("missing")).unwrap_err().kind,
        FsErrorKind::NotFound
    );
}

#[test]
fn read_limits_and_nofollow() {
    let t = tree();
    let fs_ = RealFs::new();
    let a = t.path().join("a.txt");
    assert_eq!(fs_.read(&a, 5).unwrap(), b"hello");
    assert_eq!(
        fs_.read(&a, 4).unwrap_err().kind,
        FsErrorKind::TooLarge { limit: 4 }
    );
    symlink(&a, t.path().join("a_link")).unwrap();
    assert_eq!(
        fs_.read(&t.path().join("a_link"), 100).unwrap_err().kind,
        FsErrorKind::Symlink
    );
    assert_eq!(
        fs_.read_follow(&t.path().join("a_link"), 100).unwrap(),
        b"hello"
    );
    assert_eq!(fs_.read_range(&a, 1, 3).unwrap(), b"ell");
    assert_eq!(fs_.read_range(&a, 3, 100).unwrap(), b"lo");
    let mut s = String::new();
    fs_.open_read(&a).unwrap().read_to_string(&mut s).unwrap();
    assert_eq!(s, "hello");
    assert!(fs_.open_read(&t.path().join("a_link")).is_err());
    assert_eq!(
        fs_.read(&t.path().join("sub"), 10).unwrap_err().kind,
        FsErrorKind::IsADirectory
    );
}

#[test]
fn read_dir_is_sorted_and_reports_symlinks() {
    let t = tree();
    let entries = RealFs::new().read_dir(t.path()).unwrap();
    let names: Vec<_> = entries
        .iter()
        .map(|e| e.name.to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["a.txt", "dirlink", "link_out", "loop", "sub"]);
    assert_eq!(entries[1].file_type, FileType::Symlink);
    assert_eq!(entries[4].file_type, FileType::Dir);
}

/// An entry that vanished between the listing and the look at its type (a process that
/// exited while `/proc` was read) is skipped; any other failure still fails the listing.
#[test]
fn read_dir_skips_entries_that_vanish_while_listing() {
    use std::io::{Error, ErrorKind};
    let dir = fs::metadata(".").unwrap().file_type();
    assert_eq!(real::listed_type(Ok(dir)).unwrap(), Some(dir));
    assert_eq!(
        real::listed_type(Err(Error::from(ErrorKind::NotFound))).unwrap(),
        None
    );
    let denied = real::listed_type(Err(Error::from(ErrorKind::PermissionDenied)));
    assert_eq!(denied.unwrap_err().kind(), ErrorKind::PermissionDenied);
    // The listing every process list starts from.
    let fs = RealFs::new();
    let me = std::process::id().to_string();
    let proc = fs.read_dir(Path::new("/proc")).unwrap();
    assert!(proc.iter().any(|e| e.name.to_str() == Some(me.as_str())));
}

#[test]
fn write_create_rename_remove() {
    let t = tempfile::tempdir().unwrap();
    let fs_ = RealFs::new();
    let d = t.path().join("x/y");
    fs_.create_dir_all(&d, 0o700).unwrap();
    assert_eq!(
        fs::metadata(&d).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs_.create_dir(&d, 0o700).unwrap_err().kind,
        FsErrorKind::AlreadyExists
    );
    let f = d.join("f");
    fs_.write_new(&f, b"1", 0o600).unwrap();
    assert_eq!(
        fs_.write_new(&f, b"2", 0o600).unwrap_err().kind,
        FsErrorKind::AlreadyExists
    );
    fs_.atomic_write(&f, b"3", 0o600).unwrap();
    assert_eq!(fs::read(&f).unwrap(), b"3");
    fs_.rename(&f, &d.join("g")).unwrap();
    fs_.symlink(Path::new("g"), &d.join("l")).unwrap();
    assert_eq!(
        fs_.set_mode(&d.join("l"), 0o644).unwrap_err().kind,
        FsErrorKind::Symlink
    );
    fs_.set_mode(&d.join("g"), 0o644).unwrap();
    assert_eq!(fs_.metadata(&d.join("g")).unwrap().mode, 0o644);
    fs_.remove_file(&d.join("l")).unwrap();
    assert!(d.join("g").exists());
    assert_eq!(
        fs_.remove_dir(&d).unwrap_err().kind,
        FsErrorKind::DirectoryNotEmpty
    );
    fs_.remove_file(&d.join("g")).unwrap();
    fs_.remove_dir(&d).unwrap();
    assert!(fs_.free_space(t.path()).unwrap() > 0);
    assert_eq!(
        fs_.canonicalize(&t.path().join("x/../x")).unwrap(),
        fs::canonicalize(t.path()).unwrap().join("x")
    );
}

#[test]
fn walk_visits_everything_without_following() {
    let t = tree();
    let mut seen = Vec::new();
    let s = safe_walk(
        &RealFs::new(),
        t.path(),
        WalkLimits::default(),
        &ticking(),
        |e| {
            seen.push(
                e.path
                    .strip_prefix(t.path())
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            );
            WalkControl::Continue
        },
    )
    .unwrap();
    assert_eq!(
        seen,
        [
            "a.txt",
            "dirlink",
            "link_out",
            "loop",
            "sub",
            "sub/b.txt",
            "sub/deep",
            "sub/deep/c.txt"
        ]
    );
    assert_eq!(s.entries, 8);
    assert!(s.stopped.is_none());
    assert!(s.errors.is_empty());
}

#[test]
fn walk_limits() {
    let t = tree();
    let fs_ = RealFs::new();
    let clock = ticking();
    let s = safe_walk(
        &fs_,
        t.path(),
        WalkLimits {
            max_depth: 1,
            ..WalkLimits::default()
        },
        &clock,
        |e| {
            assert_eq!(e.depth, 1);
            WalkControl::Continue
        },
    )
    .unwrap();
    assert_eq!(s.depth_limited, 1);

    let s = safe_walk(
        &fs_,
        t.path(),
        WalkLimits {
            max_entries: 3,
            ..WalkLimits::default()
        },
        &clock,
        |_| WalkControl::Continue,
    )
    .unwrap();
    assert_eq!((s.entries, s.stopped), (3, Some(WalkStop::MaxEntries)));

    let clock = ticking();
    let limits = WalkLimits::with_budget(&clock, Duration::from_secs(2));
    let s = safe_walk(&fs_, t.path(), limits, &clock, |_| WalkControl::Continue).unwrap();
    assert_eq!(s.stopped, Some(WalkStop::Deadline));
    assert!(s.entries < 8);

    let count = Cell::new(0);
    let s = safe_walk(&fs_, t.path(), WalkLimits::default(), &ticking(), |e| {
        count.set(count.get() + 1);
        if e.path.ends_with("sub") {
            WalkControl::SkipDir
        } else {
            WalkControl::Continue
        }
    })
    .unwrap();
    assert_eq!(s.entries, 5);
    let s = safe_walk(&fs_, t.path(), WalkLimits::default(), &ticking(), |_| {
        WalkControl::Stop
    })
    .unwrap();
    assert_eq!((s.entries, s.stopped), (1, Some(WalkStop::Visitor)));
}

#[test]
fn walk_refuses_symlink_root() {
    let t = tree();
    let err = safe_walk(
        &RealFs::new(),
        &t.path().join("dirlink"),
        WalkLimits::default(),
        &ticking(),
        |_| WalkControl::Continue,
    )
    .unwrap_err();
    assert_eq!(err.kind, FsErrorKind::Symlink);
}

#[test]
fn remove_tree_never_follows_symlinks() {
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("precious"), b"keep").unwrap();
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let target = root.join("games/g1");
    fs::create_dir_all(target.join("prefix/drive_c")).unwrap();
    fs::write(target.join("prefix/drive_c/f"), b"x").unwrap();
    symlink(outside.path(), target.join("prefix/escape")).unwrap();
    symlink(outside.path().join("precious"), target.join("file_link")).unwrap();

    let r = safe_remove_tree(&RealFs::new(), root, &target).unwrap();
    assert!(r.is_complete(), "{r:?}");
    assert!(!target.exists());
    assert_eq!(r.files_removed, 3);
    assert_eq!(r.dirs_removed, 3);
    assert_eq!(fs::read(outside.path().join("precious")).unwrap(), b"keep");
    // Missing target: nothing to do.
    let r = safe_remove_tree(&RealFs::new(), root, &target).unwrap();
    assert_eq!(r.files_removed, 0);
}

#[test]
fn remove_tree_confinement() {
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(outside.path().join("victim")).unwrap();
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let fs_ = RealFs::new();
    for bad in [
        root.join("../x"),
        root.to_owned(),
        Path::new("/etc").to_owned(),
    ] {
        assert_eq!(
            safe_remove_tree(&fs_, root, &bad).unwrap_err().kind,
            FsErrorKind::OutsideRoot,
            "{bad:?}"
        );
    }
    // An intermediate symlink pointing outside the root is refused.
    symlink(outside.path(), root.join("games")).unwrap();
    assert_eq!(
        safe_remove_tree(&fs_, root, &root.join("games/victim"))
            .unwrap_err()
            .kind,
        FsErrorKind::Symlink
    );
    assert!(outside.path().join("victim").exists());
    // A target that is itself a symlink is unlinked, its target kept.
    symlink(outside.path().join("victim"), root.join("lnk")).unwrap();
    let r = safe_remove_tree(&fs_, root, &root.join("lnk")).unwrap();
    assert_eq!(r.files_removed, 1);
    assert!(outside.path().join("victim").exists());
}

#[test]
fn private_dir_checks() {
    let t = tempfile::tempdir().unwrap();
    let fs_ = RealFs::new();
    let uid = rustix::process::getuid().as_raw();
    let p = t.path().join("zaun-run");
    ensure_private_dir(&fs_, &p, uid).unwrap();
    assert_eq!(fs_.metadata(&p).unwrap().mode, 0o700);
    ensure_private_dir(&fs_, &p, uid).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        ensure_private_dir(&fs_, &p, uid).unwrap_err().kind,
        FsErrorKind::Insecure
    );
    fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        ensure_private_dir(&fs_, &p, uid.wrapping_add(1))
            .unwrap_err()
            .kind,
        FsErrorKind::Insecure
    );
    let l = t.path().join("link");
    symlink(&p, &l).unwrap();
    assert_eq!(
        ensure_private_dir(&fs_, &l, uid).unwrap_err().kind,
        FsErrorKind::Insecure
    );
}

#[test]
fn errors_map_to_codes() {
    use crate::error::{Error, ErrorCode};
    let e: Error = FsError::new(FsOp::Write, "/x", FsErrorKind::NoSpace).into();
    assert_eq!(e.code, ErrorCode::FsDiskFull);
    let e: Error = FsError::new(FsOp::Write, "/x", FsErrorKind::PermissionDenied).into();
    assert_eq!(e.code, ErrorCode::FsWriteDenied);
    let e: Error = FsError::new(FsOp::Read, "/x", FsErrorKind::PermissionDenied).into();
    assert_eq!(e.code, ErrorCode::FsReadDenied);
    assert_eq!(e.context_value("path"), Some("/x"));
    let e: Error = FsError::new(FsOp::RemoveDir, "/x", FsErrorKind::Other).into();
    assert_eq!(e.code, ErrorCode::FsDeleteIncomplete);
    let e: Error = FsError::new(FsOp::ReadDir, "/x", FsErrorKind::TimedOut).into();
    assert_eq!(e.code, ErrorCode::FsTimeout);
    assert!(is_within(Path::new("/a/b"), Path::new("/a/b/c")));
    assert!(!is_within(Path::new("/a/b"), Path::new("/a/bc")));
    assert!(!is_within(Path::new("/a/b"), Path::new("/a/b/../c")));
}
