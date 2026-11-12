//! `MemFs` parity with the real filesystem for the behaviours core relies on.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use easyplay_core::clock::Clock;
use easyplay_core::fsx::{
    FileType, Fs, FsErrorKind, RealFs, WalkControl, WalkLimits, safe_remove_tree, safe_walk,
};
use easyplay_testkit::{FakeClock, MemFs, fixtures};

/// Build the same tree in both filesystems at the same absolute root.
fn both() -> (tempfile::TempDir, RealFs, MemFs, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let real = RealFs::new();
    std::fs::write(root.join("a.txt"), b"hello").unwrap();
    std::fs::create_dir_all(root.join("sub/deep")).unwrap();
    std::fs::write(root.join("sub/b.txt"), b"b").unwrap();
    std::fs::write(root.join("sub/deep/c.txt"), b"c").unwrap();
    std::os::unix::fs::symlink("/etc", root.join("link_out")).unwrap();
    std::os::unix::fs::symlink(".", root.join("loop")).unwrap();
    let mem = MemFs::new();
    mem.dir(&root);
    fixtures::basic_tree(&mem, &root);
    (tmp, real, mem, root)
}

fn walk(fs: &dyn Fs, root: &Path, clock: &dyn Clock) -> Vec<(String, FileType, usize)> {
    let mut out = Vec::new();
    safe_walk(fs, root, WalkLimits::default(), clock, |e| {
        out.push((
            e.path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            e.metadata.file_type,
            e.depth,
        ));
        WalkControl::Continue
    })
    .unwrap();
    out
}

#[test]
fn walks_agree() {
    let (_tmp, real, mem, root) = both();
    let clock = FakeClock::baseline();
    assert_eq!(walk(&real, &root, &clock), walk(&mem, &root, &clock));
}

#[test]
fn nofollow_reads_agree() {
    let (_tmp, real, mem, root) = both();
    for fs in [&real as &dyn Fs, &mem] {
        assert_eq!(fs.read(&root.join("a.txt"), 100).unwrap(), b"hello");
        assert_eq!(
            fs.read(&root.join("a.txt"), 2).unwrap_err().kind,
            FsErrorKind::TooLarge { limit: 2 }
        );
        assert!(fs.metadata(&root.join("loop")).unwrap().is_symlink());
        assert_eq!(
            fs.read_link(&root.join("link_out")).unwrap(),
            Path::new("/etc")
        );
        assert_eq!(
            fs.read(&root.join("sub"), 10).unwrap_err().kind,
            FsErrorKind::IsADirectory
        );
        assert_eq!(
            fs.metadata(&root.join("missing")).unwrap_err().kind,
            FsErrorKind::NotFound
        );
        assert_eq!(
            fs.create_dir(&root.join("sub"), 0o700).unwrap_err().kind,
            FsErrorKind::AlreadyExists
        );
        assert_eq!(
            fs.remove_dir(&root.join("sub")).unwrap_err().kind,
            FsErrorKind::DirectoryNotEmpty
        );
        assert_eq!(
            fs.canonicalize(&root.join("loop/sub/../a.txt")).unwrap(),
            fs.canonicalize(&root.join("a.txt")).unwrap()
        );
        let names: Vec<_> = fs
            .read_dir(&root)
            .unwrap()
            .into_iter()
            .map(|e| e.name.into_string().unwrap())
            .collect();
        assert_eq!(names, ["a.txt", "link_out", "loop", "sub"]);
    }
}

#[test]
fn symlink_targets_survive_removal() {
    let (_tmp, real, mem, root) = both();
    for fs in [&real as &dyn Fs, &mem] {
        let link = root.join("sub/a_link");
        fs.symlink(&root.join("a.txt"), &link).unwrap();
        let r = safe_remove_tree(fs, &root, &root.join("sub")).unwrap();
        assert!(r.is_complete(), "{r:?}");
        assert!(!fs.exists(&root.join("sub")));
        assert_eq!(fs.read(&root.join("a.txt"), 10).unwrap(), b"hello");
    }
}

#[test]
fn atomic_write_agrees() {
    let (_tmp, real, mem, root) = both();
    for fs in [&real as &dyn Fs, &mem] {
        let p = root.join("state.json");
        fs.atomic_write(&p, b"1", 0o600).unwrap();
        fs.atomic_write(&p, b"2", 0o600).unwrap();
        assert_eq!(fs.read(&p, 10).unwrap(), b"2");
        assert_eq!(fs.metadata(&p).unwrap().mode, 0o600);
        assert_eq!(
            fs.atomic_write(&root.join("nope/x"), b"", 0o600)
                .unwrap_err()
                .kind,
            FsErrorKind::NotFound
        );
    }
}
