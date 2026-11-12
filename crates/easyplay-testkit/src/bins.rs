//! Locate (and if needed build) the workspace's binaries for tests in other packages:
//! `fake-bwrap`, `fake-umu-run`, `probe-payload`, `easyplay-init` and `easyplay`.
//!
//! Cargo provides `CARGO_BIN_EXE_*` only to a package's own tests, so the lifecycle tests in
//! `easyplay-core` find the binaries in the target directory of the running test and run
//! `cargo build -p <package> --bin <name>` once per process (a no-op when up to date).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Mutex;

static BUILT: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());

fn package_of(name: &str) -> &'static str {
    match name {
        "fake-bwrap" | "fake-umu-run" | "probe-payload" => "easyplay-testkit",
        "easyplay-init" => "easyplay-init",
        "easyplay" => "easyplay",
        other => panic!("unknown workspace binary {other}"),
    }
}

/// The directory holding the binaries of the current build (`target/<profile>`).
pub fn target_profile_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let mut dir = exe.parent().expect("test binary directory").to_path_buf();
    if dir.ends_with("deps") {
        dir.pop();
    }
    dir
}

/// The path of workspace binary `name`, built first if this process has not done so yet.
#[allow(clippy::disallowed_methods)] // Test support: running cargo.
pub fn workspace_bin(name: &'static str) -> PathBuf {
    let path = target_profile_dir().join(name);
    let mut built = BUILT.lock().unwrap();
    if !built.contains(name) {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let status = std::process::Command::new(cargo)
            .args(["build", "--quiet", "-p", package_of(name), "--bin", name])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .status();
        match status {
            Ok(s) if s.success() => {}
            other if path.is_file() => {
                eprintln!("building {name} failed ({other:?}); using the existing binary");
            }
            other => panic!("cannot build {name}: {other:?}"),
        }
        built.insert(name);
    }
    assert!(path.is_file(), "{} missing", path.display());
    path
}
