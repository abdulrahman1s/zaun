//! Adopting umu's own Steam Linux Runtime into the store (runtime::adopt) and its manifest.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use easyplay_core::clock::{Clock, SystemClock};
use easyplay_core::fsx::RealFs;
use easyplay_core::jobs::CancelToken;
use easyplay_core::model::SlrVariant;
use easyplay_core::runtime::discovery::detect_slr;
use easyplay_core::runtime::manifest::Manifest;
use easyplay_core::runtime::{RuntimeStore, adopt_slr};

/// A minimal complete `steamrt3` install as umu leaves it.
fn fake_umu(root: &Path) -> PathBuf {
    let umu = root.join("umu");
    let v = umu.join("steamrt3");
    let files = v.join("sniper_platform_3.0.1/files/lib");
    std::fs::create_dir_all(&files).unwrap();
    std::fs::create_dir_all(v.join("var/tmp-x")).unwrap();
    std::fs::write(v.join("var/tmp-x/junk"), "x").unwrap();
    std::fs::write(files.join("libx.so"), vec![7u8; 100_000]).unwrap();
    std::fs::write(v.join("sniper_platform_3.0.1/files/.ref"), "").unwrap();
    std::fs::write(v.join("_v2-entry-point"), "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(
        v.join("_v2-entry-point"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    symlink("_v2-entry-point", v.join("umu")).unwrap();
    std::fs::write(v.join("VERSIONS.txt"), "depot\t3.0.1\n").unwrap();
    std::fs::write(v.join(".installed.ok"), "ok\n").unwrap();
    std::fs::write(umu.join("umu-shim"), "#!/bin/sh\n").unwrap();
    v
}

fn listing(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let e = e.unwrap();
            let md = std::fs::symlink_metadata(e.path()).unwrap();
            if md.is_dir() {
                stack.push(e.path());
                out.push((e.path(), Vec::new()));
            } else if md.is_file() {
                out.push((e.path(), std::fs::read(e.path()).unwrap()));
            } else {
                out.push((
                    e.path(),
                    std::fs::read_link(e.path())
                        .unwrap()
                        .into_os_string()
                        .into_encoded_bytes(),
                ));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn adopt_copies_into_the_store_and_never_touches_the_source() {
    let tmp = tempfile::tempdir().unwrap();
    let src = fake_umu(&tmp.path().join("home/.local/share"));
    let before = listing(src.parent().unwrap());
    let store = RuntimeStore::at(&tmp.path().join("data"), &tmp.path().join("cache"));
    let fs = RealFs::new();
    let clock = SystemClock::new();
    let r = adopt_slr(
        &fs,
        &clock,
        &store,
        &src,
        SlrVariant::Steamrt3,
        clock.now(),
        &CancelToken::new(),
    )
    .unwrap();
    assert!(!r.already_installed);
    assert_eq!(r.files, 4); // libx.so, .ref, _v2-entry-point, VERSIONS.txt
    let dest = store.slr_dir(SlrVariant::Steamrt3);
    let found = detect_slr(&fs, &dest, SlrVariant::Steamrt3).unwrap();
    assert!(found.is_complete());
    assert_eq!(found.version.as_deref(), Some("3.0.1"));
    assert!(!dest.join("var").exists());
    assert_eq!(
        std::fs::metadata(dest.join("_v2-entry-point"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert!(store.umu_shim().is_file());
    // The manifest excludes var/, the marker and .ref; its tree hash is in the row.
    let m: Manifest =
        serde_json::from_slice(&std::fs::read(store.manifest_file(SlrVariant::Steamrt3)).unwrap())
            .unwrap();
    let paths: Vec<&str> = m.files.iter().map(|f| f.p.as_str()).collect();
    assert!(paths.contains(&"umu"));
    assert!(paths.contains(&"sniper_platform_3.0.1/files/lib/libx.so"));
    assert!(
        !paths
            .iter()
            .any(|p| p.starts_with("var") || p.ends_with(".ref"))
    );
    assert!(!paths.contains(&".installed.ok"));
    let mut sorted = paths.clone();
    sorted.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    assert_eq!(paths, sorted);
    let install = r.install.unwrap();
    assert_eq!(
        install
            .manifest_tree_sha256
            .map(|h| easyplay_core::model::to_hex(&h)),
        Some(m.tree_sha256.clone())
    );
    assert!(m.umu_shim.is_some());
    // The source is unchanged, and no staging directory is left.
    assert_eq!(listing(src.parent().unwrap()), before);
    assert!(std::fs::read_dir(&store.root).unwrap().all(|e| {
        !e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".adopt")
    }));
    // Idempotent.
    let again = adopt_slr(
        &fs,
        &clock,
        &store,
        &src,
        SlrVariant::Steamrt3,
        clock.now(),
        &CancelToken::new(),
    )
    .unwrap();
    assert!(again.already_installed);
}

#[test]
fn adopt_refuses_incomplete_sources_and_cleans_up() {
    let tmp = tempfile::tempdir().unwrap();
    let src = fake_umu(&tmp.path().join("home/.local/share"));
    std::fs::remove_file(src.join(".installed.ok")).unwrap();
    let store = RuntimeStore::at(&tmp.path().join("data"), &tmp.path().join("cache"));
    let fs = RealFs::new();
    let clock = SystemClock::new();
    let err = adopt_slr(
        &fs,
        &clock,
        &store,
        &src,
        SlrVariant::Steamrt3,
        clock.now(),
        &CancelToken::new(),
    )
    .unwrap_err();
    assert_eq!(err.code, easyplay_core::error::ErrorCode::RtInvalidFolder);
    assert!(!store.slr_dir(SlrVariant::Steamrt3).exists());
}
