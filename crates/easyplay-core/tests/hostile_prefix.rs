//! Host-side hostile-input tests (SEC-003, TESTING_STRATEGY §8 "Directory trees" and
//! "Prefix as hostile tree", SECURITY_MODEL §9).
//!
//! A game controls its prefix completely. Installer discovery, size computation and removal
//! run on it outside any sandbox, so they must never read, follow or delete anything outside
//! the game's directory, and must not hang on FIFOs, loops or huge trees. Policy imports with
//! denied paths are covered by `library::hostile_imports_are_rejected`; symlink-free removal
//! on `MemFs` by the `fsx` unit tests.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use easyplay_core::clock::{SystemClock, Timestamp};
use easyplay_core::fsx::{Fs, RealFs, safe_remove_tree};
use easyplay_core::jobs::CancelToken;
use easyplay_core::library::installer;
use easyplay_core::library::prefix_ops::tree_size;
use easyplay_testkit::MemFs;
use easyplay_testkit::fixtures::lnk::{LnkSpec, build as lnk_build};

struct Tree {
    _tmp: tempfile::TempDir,
    /// `<root>/outside`: must stay untouched and unread.
    outside: PathBuf,
    /// `<root>/game`: the game directory (removal root).
    game: PathBuf,
    /// `<game>/prefix`.
    prefix: PathBuf,
    /// Bytes of regular files inside the prefix.
    inside_bytes: u64,
}

fn put(p: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

fn lnk_to(local: Option<&str>, relative: Option<&str>) -> Vec<u8> {
    lnk_build(&LnkSpec {
        local_base_path: local.map(str::to_owned),
        relative_path: relative.map(str::to_owned),
        ..LnkSpec::default()
    })
}

/// A prefix written by a hostile game.
fn hostile_tree() -> Tree {
    let tmp = tempfile::Builder::new()
        .prefix("hostile-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let outside = root.join("outside");
    put(&outside.join("secret.txt"), b"secret");
    put(&outside.join("evil.exe"), &vec![b'M'; 1 << 20]);
    put(&outside.join("sub/deeper.exe"), b"MZ");

    let game = root.join("game");
    let prefix = game.join("prefix");
    let c = prefix.join("drive_c");
    let mut inside = 0u64;
    let mut file = |p: PathBuf, b: &[u8]| {
        inside += b.len() as u64;
        put(&p, b);
    };
    file(c.join("Games/Real/real.exe"), b"MZ real");
    let desk = c.join("users/steamuser/Desktop");
    // Shortcuts pointing out of the prefix: Z: drive, `..` walk, and through a symlink.
    file(
        desk.join("z.lnk"),
        &lnk_to(Some("Z:\\outside\\evil.exe"), None),
    );
    file(
        desk.join("dotdot.lnk"),
        &lnk_to(None, Some("..\\..\\..\\..\\..\\..\\outside\\evil.exe")),
    );
    file(
        desk.join("vialink.lnk"),
        &lnk_to(Some("C:\\escape\\evil.exe"), None),
    );
    file(
        desk.join("real.lnk"),
        &lnk_to(Some("C:\\Games\\Real\\real.exe"), None),
    );
    // A corrupt shortcut.
    file(desk.join("broken.lnk"), b"L\0\0\0garbage");
    // Symlinks out of the prefix and loops.
    symlink(&outside, c.join("escape")).unwrap();
    symlink(&outside, c.join("users/steamuser/Documents")).unwrap();
    symlink(outside.join("evil.exe"), c.join("Games/linked.exe")).unwrap();
    symlink(&c, c.join("Games/loop")).unwrap();
    symlink("..", c.join("Games/Real/up")).unwrap();
    symlink("dangling-target", c.join("Games/dangling.exe")).unwrap();
    // FIFOs that would block a reader, a socket, odd names.
    rustix::fs::mknodat(
        rustix::fs::CWD,
        c.join("Games/trap.exe"),
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::from_raw_mode(0o644),
        0,
    )
    .unwrap();
    rustix::fs::mknodat(
        rustix::fs::CWD,
        desk.join("trap.lnk"),
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::from_raw_mode(0o644),
        0,
    )
    .unwrap();
    let _sock = std::os::unix::net::UnixListener::bind(c.join("Games/sock.exe")).unwrap();
    file(c.join("Games/new\nline.exe"), b"MZ");
    file(c.join("Games/ctrl\u{1b}[31m.exe"), b"MZ");
    file(
        c.join("Games")
            .join(OsStr::from_bytes(b"bad\xff\xfeutf8.exe")),
        b"MZ",
    );
    file(c.join("windows/system32/notepad.exe"), b"MZ");
    Tree {
        _tmp: tmp,
        outside,
        game,
        prefix,
        inside_bytes: inside,
    }
}

fn outside_intact(t: &Tree) {
    assert_eq!(
        std::fs::read(t.outside.join("secret.txt")).unwrap(),
        b"secret"
    );
    assert_eq!(
        std::fs::metadata(t.outside.join("evil.exe")).unwrap().len(),
        1 << 20
    );
    assert!(t.outside.join("sub/deeper.exe").is_file());
}

#[test]
fn installer_discovery_stays_inside_the_prefix() {
    let t = hostile_tree();
    let fs = RealFs::new();
    let clock = SystemClock::new();
    let start = Instant::now();
    let found = installer::discover(&fs, &clock, &t.prefix, Timestamp::EPOCH);
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "discovery blocked"
    );
    let drive_c = t.prefix.join("drive_c");
    for p in &found {
        assert!(p.path.starts_with(&drive_c), "{}", p.path.display());
        let real = std::fs::canonicalize(&p.path).unwrap();
        assert!(
            real.starts_with(&drive_c),
            "{} -> {}",
            p.path.display(),
            real.display()
        );
        assert!(std::fs::symlink_metadata(&p.path).unwrap().is_file());
        assert!(!p.name.chars().any(char::is_control), "{:?}", p.name);
    }
    let names: Vec<String> = found.iter().map(|p| p.name.clone()).collect();
    assert!(
        found
            .iter()
            .any(|p| p.path.ends_with("Games/Real/real.exe")),
        "{names:?}"
    );
    assert!(!found.iter().any(|p| p.path.ends_with("notepad.exe")));
    outside_intact(&t);
}

#[test]
fn size_counts_only_regular_files_inside() {
    let t = hostile_tree();
    let fs = RealFs::new();
    let clock = SystemClock::new();
    let r = tree_size(
        &fs,
        &clock,
        &t.prefix,
        Duration::from_secs(5),
        &CancelToken::new(),
    )
    .unwrap();
    assert!(r.complete);
    assert_eq!(r.bytes, t.inside_bytes, "outside files counted");
}

#[test]
fn removal_deletes_the_prefix_and_nothing_outside() {
    let t = hostile_tree();
    let fs = RealFs::new();
    let report = safe_remove_tree(&fs, &t.game, &t.prefix).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(!t.prefix.exists());
    outside_intact(&t);
}

#[test]
fn removal_refuses_symlinked_components_and_targets_outside() {
    let t = hostile_tree();
    let fs = RealFs::new();
    // `<game>/link -> outside`: removing `<game>/link/sub` must not reach `outside/sub`.
    symlink(&t.outside, t.game.join("link")).unwrap();
    assert!(safe_remove_tree(&fs, &t.game, &t.game.join("link/sub")).is_err());
    assert!(safe_remove_tree(&fs, &t.game, &t.outside).is_err());
    assert!(safe_remove_tree(&fs, &t.game, &t.game.join("../outside")).is_err());
    // A symlink as the target is unlinked, not followed.
    let r = safe_remove_tree(&fs, &t.game, &t.game.join("link")).unwrap();
    assert!(r.errors.is_empty());
    assert!(!t.game.join("link").exists());
    outside_intact(&t);
}

#[test]
fn deep_and_wide_trees_are_bounded() {
    let fs = MemFs::new();
    let clock = SystemClock::new();
    // 200 nested components (well past every walk's depth limit; `MemFs` builds paths
    // in quadratic time, so the 10 000 levels of TESTING_STRATEGY §8 would only test `MemFs`).
    let mut deep = PathBuf::from("/p/drive_c/deep");
    for i in 0..200 {
        deep.push(format!("d{}", i % 10));
    }
    fs.file(deep.join("x.exe"), b"MZ");
    // A wide directory.
    for i in 0..5_000 {
        fs.file(format!("/p/drive_c/wide/f{i}.dat"), b"x");
    }
    let start = Instant::now();
    let r = tree_size(
        &fs,
        &clock,
        Path::new("/p"),
        Duration::from_secs(10),
        &CancelToken::new(),
    )
    .unwrap();
    assert!(start.elapsed() < Duration::from_secs(10));
    // The depth limit stops the descent before the deep file; the wide directory is counted.
    assert_eq!(r.files, 5_000);
    // Discovery stops at its own depth limit (the executable is 200 levels down).
    let found = installer::discover(&fs, &clock, Path::new("/p"), Timestamp::EPOCH);
    assert!(found.is_empty(), "{found:?}");
    assert!(fs.exists(Path::new("/p/drive_c/wide/f0.dat")));
}
