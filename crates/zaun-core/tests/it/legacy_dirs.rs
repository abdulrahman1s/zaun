//! The one-time move of the old `easyplay` directories to `zaun` (ADR-0039).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use zaun_core::fsx::{Fs, FsErrorKind, FsOp, LegacyMove, adopt_legacy_dirs};
use zaun_core::paths::{AppPaths, PathEnv};
use zaun_testkit::MemFs;

fn env() -> PathEnv {
    PathEnv::new(1000).with("HOME", "/home/sam")
}

fn old_install() -> MemFs {
    let fs = MemFs::new();
    fs.file("/home/sam/.config/easyplay/config.toml", b"c")
        .file("/home/sam/.local/share/easyplay/library.db", b"db")
        .file("/home/sam/.local/share/easyplay/games/a/prefix/x", b"x")
        .file("/home/sam/.local/state/easyplay/window.json", b"w")
        .file("/home/sam/.cache/easyplay/CACHEDIR.TAG", b"t");
    fs
}

#[test]
fn old_dirs_move_to_the_new_names_once_and_leave_a_link() {
    let fs = old_install();
    let env = env();
    let paths = AppPaths::resolve(&env).unwrap();

    let moved = adopt_legacy_dirs(&fs, &env, &paths);
    assert_eq!(moved.len(), 4);
    assert!(moved.iter().all(|m| matches!(m, LegacyMove::Moved { .. })));
    assert_eq!(
        fs.contents("/home/sam/.local/share/zaun/library.db")
            .unwrap(),
        b"db"
    );
    let old = Path::new("/home/sam/.local/share/easyplay");
    assert!(fs.metadata(old).unwrap().is_symlink());
    assert_eq!(fs.read_link(old).unwrap(), paths.data_dir);
    // An absolute path stored by the old version still resolves.
    let stored = old.join("games/a/prefix/x");
    assert_eq!(fs.read_follow(&stored, 10).unwrap(), b"x");

    assert!(adopt_legacy_dirs(&fs, &env, &paths).is_empty());
    assert_eq!(
        fs.contents("/home/sam/.config/zaun/config.toml").unwrap(),
        b"c"
    );
}

#[test]
fn nothing_moves_when_the_new_dir_exists() {
    let fs = old_install();
    fs.file("/home/sam/.local/share/zaun/library.db", b"new");
    let env = env();
    let paths = AppPaths::resolve(&env).unwrap();

    let out = adopt_legacy_dirs(&fs, &env, &paths);
    assert!(
        out.iter()
            .any(|m| matches!(m, LegacyMove::BothExist { to, .. } if *to == paths.data_dir))
    );
    assert_eq!(
        fs.contents("/home/sam/.local/share/zaun/library.db")
            .unwrap(),
        b"new"
    );
    let old = Path::new("/home/sam/.local/share/easyplay");
    assert!(fs.metadata(old).unwrap().is_dir());
    assert_eq!(
        fs.contents("/home/sam/.local/share/easyplay/library.db")
            .unwrap(),
        b"db"
    );
}

#[test]
fn a_fresh_machine_without_old_dirs_is_left_alone() {
    let fs = MemFs::new();
    let env = env();
    let paths = AppPaths::resolve(&env).unwrap();
    assert!(adopt_legacy_dirs(&fs, &env, &paths).is_empty());
    assert!(!fs.exists(&paths.data_dir));
}

#[test]
fn a_directory_with_an_override_is_never_moved() {
    let fs = MemFs::new();
    fs.file("/srv/easyplay/library.db", b"db");
    let env = env().with("ZAUN_DATA_DIR", "/srv/zaun");
    let paths = AppPaths::resolve(&env).unwrap();
    assert!(adopt_legacy_dirs(&fs, &env, &paths).is_empty());
    assert!(fs.metadata(Path::new("/srv/easyplay")).unwrap().is_dir());
}

#[test]
fn a_failed_rename_is_reported_and_nothing_changes() {
    let fs = old_install();
    fs.fail(
        FsOp::Rename,
        "/home/sam/.local/share/easyplay",
        FsErrorKind::PermissionDenied,
    );
    let env = env();
    let paths = AppPaths::resolve(&env).unwrap();

    let out = adopt_legacy_dirs(&fs, &env, &paths);
    assert!(out.iter().any(|m| matches!(m, LegacyMove::Failed { .. })));
    assert_eq!(
        fs.contents("/home/sam/.local/share/easyplay/library.db")
            .unwrap(),
        b"db"
    );
    assert!(!fs.exists(&paths.data_dir));
}

#[test]
fn a_missing_link_is_reported_after_the_move() {
    let fs = old_install();
    fs.fail(
        FsOp::Symlink,
        "/home/sam/.cache/easyplay",
        FsErrorKind::PermissionDenied,
    );
    let env = env();
    let paths = AppPaths::resolve(&env).unwrap();

    let out = adopt_legacy_dirs(&fs, &env, &paths);
    assert!(
        out.iter()
            .any(|m| matches!(m, LegacyMove::LinkFailed { .. }))
    );
    assert_eq!(
        fs.contents("/home/sam/.cache/zaun/CACHEDIR.TAG").unwrap(),
        b"t"
    );
}
