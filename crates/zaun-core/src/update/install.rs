//! How this copy was installed (ADR-0040 §3, ADR-0041 §1), and whether it updates itself
//! through the bundle store.

use std::path::Path;

use super::store::Store;
use crate::fsx::Fs;
use crate::model::InstallChannel;

/// The program interpreter the bundle's binaries name (`PT_INTERP`). NixOS has it only with
/// nix-ld, which `programs.zaun.selfUpdate` turns on.
pub const GENERIC_LOADER: &str = "/lib64/ld-linux-x86-64.so.2";

/// The running copy: its channel, and the store it updates through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    pub channel: InstallChannel,
    pub store: Store,
}

impl Install {
    /// The store, when this copy updates itself.
    pub fn store(&self) -> Option<&Store> {
        self.channel.self_updates().then_some(&self.store)
    }
}

/// Detect the channel from the resolved path of the running binary. `install.sh` writes
/// `install-manifest` beside the binary: without it, a writable tree is not an install.
pub fn detect(fs: &dyn Fs, exe: &Path, store: Store) -> Install {
    let channel = channel(fs, exe, &store);
    Install { channel, store }
}

fn channel(fs: &dyn Fs, exe: &Path, store: &Store) -> InstallChannel {
    if store.contains(fs, exe) {
        return InstallChannel::Bundle;
    }
    if exe.starts_with("/nix/store") {
        let bundles = fs
            .metadata_follow(Path::new(GENERIC_LOADER))
            .is_ok_and(|m| m.is_file());
        return InstallChannel::Nix { bundles };
    }
    if fs.exists(Path::new("/.flatpak-info")) {
        return InstallChannel::Flatpak;
    }
    if is_tarball(fs, exe) {
        return InstallChannel::Tarball;
    }
    if exe.starts_with("/usr") && !exe.starts_with("/usr/local") {
        return InstallChannel::Package;
    }
    InstallChannel::Other
}

/// `PREFIX/libexec/zaun/zaun` with the `install-manifest` of `install.sh` beside it.
fn is_tarball(fs: &dyn Fs, exe: &Path) -> bool {
    let Some(dir) = exe.parent() else {
        return false;
    };
    let layout = exe
        .file_name()
        .is_some_and(|n| n == crate::consts::BIN_NAME)
        && dir
            .file_name()
            .is_some_and(|n| n == crate::consts::DIR_NAME)
        && dir
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|n| n == "libexec");
    layout
        && fs
            .metadata(&dir.join("install-manifest"))
            .is_ok_and(|m| m.is_file())
}
