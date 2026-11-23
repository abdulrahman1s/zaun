//! The helper's command line and environment: the sandbox of ADR-0033, without a library.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Read-only binds for the helper: programs, libraries and system configuration. `-try`:
/// whatever does not exist on this system is skipped.
const SYSTEM_BINDS: &[&str] = &[
    "/usr",
    "/lib",
    "/lib64",
    "/lib32",
    "/bin",
    "/sbin",
    "/etc",
    "/nix/store",
    "/run/current-system",
    "/run/systemd/resolve",
    "/run/NetworkManager",
];

/// The command line that starts `helper` for `data_dir`: `(program, args)`. Under `bwrap`
/// the helper sees the system read-only, a private `/tmp`, the network and `data_dir`
/// writable; without it, it runs as it is.
pub(super) fn command(
    helper: &Path,
    bwrap: Option<&Path>,
    data_dir: &Path,
) -> (PathBuf, Vec<OsString>) {
    let Some(bwrap) = bwrap else {
        return (helper.to_owned(), Vec::new());
    };
    let mut a: Vec<OsString> = [
        "--unshare-all",
        "--share-net",
        "--die-with-parent",
        "--new-session",
        "--dev",
        "/dev",
        "--proc",
        "/proc",
        "--tmpfs",
        "/tmp",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    let mut ro = |p: &Path, try_: bool| {
        a.push(if try_ { "--ro-bind-try" } else { "--ro-bind" }.into());
        a.push(p.into());
        a.push(p.into());
    };
    for p in SYSTEM_BINDS {
        ro(Path::new(p), true);
    }
    ro(helper, false);
    a.extend(["--bind".into(), data_dir.into(), data_dir.into()]);
    a.extend(["--chdir".into(), data_dir.into(), "--".into()]);
    a.push(helper.into());
    (bwrap.to_owned(), a)
}

/// The helper's whole environment.
pub(super) fn env(data_dir: &Path) -> Vec<(OsString, OsString)> {
    vec![
        ("HOME".into(), data_dir.into()),
        ("LANG".into(), "C.UTF-8".into()),
        ("PATH".into(), "/usr/bin:/bin".into()),
    ]
}
