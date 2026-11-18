//! A Steam game's launch options (ADR-0031 §6a), written into a fake Steam root: refused
//! while Steam runs, a backup beside the file, an atomic write with the same permissions,
//! the missing block created, nothing written when nothing changes, and files that cannot
//! be edited safely left alone.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use zaun_core::error::ErrorCode;
use zaun_core::fsx::{Fs, FsOp};
use zaun_core::steam::launch_options::{self as lo, BACKUP_NAME, Written};
use zaun_testkit::MemFs;
use zaun_testkit::fakes::{FakeProc, proc_entry};
use zaun_testkit::fixtures::steam_root::{self, ACCOUNT};

const HOME: &str = "/home/player";

fn setup() -> (MemFs, FakeProc, PathBuf, PathBuf) {
    let fs = MemFs::new();
    let fx = steam_root::steam_root(&fs, Path::new(HOME));
    let file = fx
        .root
        .join("userdata")
        .join(ACCOUNT)
        .join("config/localconfig.vdf");
    (fs, FakeProc::new(), fx.root, file)
}

fn writes(fs: &MemFs) -> usize {
    [FsOp::Write, FsOp::Rename, FsOp::SetMode, FsOp::RemoveFile]
        .iter()
        .map(|op| fs.calls(*op))
        .sum()
}

#[test]
fn reads_what_steam_has() {
    let (fs, _, root, _) = setup();
    assert_eq!(
        lo::read(&fs, &root, 1_174_180).as_deref(),
        Some("PROTON_LOG=1 %command%")
    );
    assert_eq!(lo::read(&fs, &root, 440).as_deref(), Some(""));
    // No Steam account on this root.
    assert_eq!(lo::read(&fs, Path::new("/nowhere"), 440), None);
}

#[test]
fn refused_while_steam_runs() {
    for comm in ["steam", "steamwebhelper", "steam.sh"] {
        let (fs, proc, root, file) = setup();
        proc.insert(proc_entry(321, 1, comm, 1));
        let before = fs.read(&file, u64::MAX).unwrap();
        let w0 = writes(&fs);
        let err = lo::write(&fs, &proc, &root, 440, "-novid").unwrap_err();
        assert_eq!(err.code, ErrorCode::StmSteamOpen, "{comm}");
        assert_eq!(writes(&fs), w0, "nothing written while {comm} runs");
        assert_eq!(fs.read(&file, u64::MAX).unwrap(), before);
        assert!(!fs.exists(&file.with_file_name(BACKUP_NAME)));
    }
    // A game's own process is not Steam.
    let (fs, proc, root, _) = setup();
    proc.insert(proc_entry(322, 1, "reaper", 1));
    proc.insert(proc_entry(323, 1, "steam_game.exe", 1));
    assert!(lo::write(&fs, &proc, &root, 440, "-novid").is_ok());
}

#[test]
fn writes_with_a_backup_and_the_same_permissions() {
    let (fs, proc, root, file) = setup();
    fs.set_mode(&file, 0o600).unwrap();
    let before = fs.read(&file, u64::MAX).unwrap();
    let r = lo::write(&fs, &proc, &root, 1_174_180, "mangohud %command%").unwrap();
    let backup = file.with_file_name(BACKUP_NAME);
    assert_eq!(
        r,
        Written::Changed {
            path: file.clone(),
            backup: backup.clone()
        }
    );
    assert_eq!(fs.read(&backup, u64::MAX).unwrap(), before);
    let after = fs.read(&file, u64::MAX).unwrap();
    let want = String::from_utf8(before)
        .unwrap()
        .replace("\"PROTON_LOG=1 %command%\"", "\"mangohud %command%\"");
    assert_eq!(String::from_utf8(after).unwrap(), want);
    assert_eq!(fs.metadata(&file).unwrap().mode & 0o7777, 0o600);
    // One backup, replaced by the next change.
    let second = fs.read(&file, u64::MAX).unwrap();
    lo::write(&fs, &proc, &root, 1_174_180, "").unwrap();
    assert_eq!(fs.read(&backup, u64::MAX).unwrap(), second);
    assert_eq!(lo::read(&fs, &root, 1_174_180).as_deref(), Some(""));
}

#[test]
fn creates_the_missing_block() {
    let (fs, proc, root, file) = setup();
    // An app Steam has no block for yet.
    lo::write(&fs, &proc, &root, 70, "%command% -console").unwrap();
    assert_eq!(
        lo::read(&fs, &root, 70).as_deref(),
        Some("%command% -console")
    );
    let text = String::from_utf8(fs.read(&file, u64::MAX).unwrap()).unwrap();
    assert!(text.contains(
        "\t\t\t\t\t\"70\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"%command% -console\"\n\t\t\t\t\t}\n"
    ));
    // Play times and the other values read as before.
    assert_eq!(
        lo::read(&fs, &root, 1_174_180).as_deref(),
        Some("PROTON_LOG=1 %command%")
    );
}

#[test]
fn nothing_is_written_when_nothing_changes() {
    let (fs, proc, root, file) = setup();
    let w0 = writes(&fs);
    assert_eq!(
        lo::write(&fs, &proc, &root, 1_174_180, "PROTON_LOG=1 %command%").unwrap(),
        Written::Unchanged
    );
    assert_eq!(
        lo::write(&fs, &proc, &root, 440, "").unwrap(),
        Written::Unchanged
    );
    assert_eq!(writes(&fs), w0);
    assert!(!fs.exists(&file.with_file_name(BACKUP_NAME)));
}

#[test]
fn files_that_cannot_be_edited_safely_are_left_alone() {
    // Cut off mid-write by Steam.
    let (fs, proc, root, file) = setup();
    let text = fs.read(&file, u64::MAX).unwrap();
    fs.file(&file, &text[..text.len() - 20]);
    let w0 = writes(&fs);
    let err = lo::write(&fs, &proc, &root, 440, "-novid").unwrap_err();
    assert_eq!(err.code, ErrorCode::StmSettingsUnwritable);
    assert_eq!(writes(&fs), w0);
    // A line break in the value.
    let (fs, proc, root, _) = setup();
    let err = lo::write(&fs, &proc, &root, 440, "a\nb").unwrap_err();
    assert_eq!(err.code, ErrorCode::StmSettingsUnwritable);
    // No account at all.
    let fs = MemFs::new();
    fs.dir("/home/p/.local/share/Steam/steamapps");
    let err = lo::write(
        &fs,
        &FakeProc::new(),
        Path::new("/home/p/.local/share/Steam"),
        440,
        "-novid",
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::StmSettingsUnwritable);
    // A symbolic link in place of the file is not followed.
    let (fs, proc, root, file) = setup();
    let elsewhere = Path::new("/tmp/other.vdf");
    fs.file(elsewhere, fs.read(&file, u64::MAX).unwrap());
    fs.remove_file(&file).unwrap();
    fs.symlink(elsewhere, &file).unwrap();
    assert!(lo::write(&fs, &proc, &root, 440, "-novid").is_err());
}

#[test]
fn a_failed_write_says_so_without_the_account_s_path() {
    use zaun_core::fsx::FsErrorKind;
    let (fs, proc, root, file) = setup();
    let before = fs.read(&file, u64::MAX).unwrap();
    fs.fail(FsOp::Write, &file, FsErrorKind::NoSpace);
    let err = lo::write(&fs, &proc, &root, 440, "-novid").unwrap_err();
    assert_eq!(err.code, ErrorCode::StmSettingsUnwritable);
    assert!(
        !format!("{err:?}").contains(ACCOUNT),
        "the account's folder is not in the error: {err:?}"
    );
    assert_eq!(fs.read(&file, u64::MAX).unwrap(), before);
}
