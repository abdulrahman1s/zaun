//! How this copy was installed (ADR-0040 §3), and where an owned install keeps its update.

use std::path::{Path, PathBuf};

use crate::fsx::Fs;
use crate::model::InstallChannel;

/// The running copy: its channel, and the prefix of a tarball install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    pub channel: InstallChannel,
    /// `PREFIX` of `PREFIX/libexec/zaun/zaun` (tarball installs only).
    pub prefix: Option<PathBuf>,
}

impl Install {
    /// Where an owned install stages, keeps and replaces files; `None` unless Zaun updates
    /// itself.
    pub fn layout(&self) -> Option<Layout> {
        match (self.channel.self_updates(), &self.prefix) {
            (true, Some(p)) => Some(Layout { prefix: p.clone() }),
            _ => None,
        }
    }
}

/// Detect the channel from the resolved path of the running binary. `install.sh` writes
/// `install-manifest` beside the binary: without it, a writable tree is not an install.
pub fn detect(fs: &dyn Fs, exe: &Path, uid: u32) -> Install {
    let other = |channel| Install {
        channel,
        prefix: None,
    };
    if exe.starts_with("/nix/store") {
        return other(InstallChannel::Nix);
    }
    if fs.exists(Path::new("/.flatpak-info")) {
        return other(InstallChannel::Flatpak);
    }
    if let Some(prefix) = tarball_prefix(exe) {
        let dir = prefix.join("libexec/zaun");
        if fs
            .metadata(&dir.join("install-manifest"))
            .is_ok_and(|m| m.is_file())
        {
            let mine = |p: &Path| fs.metadata(p).is_ok_and(|m| m.uid == uid);
            let owned = mine(&dir) && mine(exe);
            return Install {
                channel: InstallChannel::Tarball { owned },
                prefix: Some(prefix),
            };
        }
    }
    if exe.starts_with("/usr") && !exe.starts_with("/usr/local") {
        return other(InstallChannel::Package);
    }
    other(InstallChannel::Other)
}

fn tarball_prefix(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    let ok = exe.file_name()? == crate::consts::BIN_NAME
        && dir.file_name()? == crate::consts::DIR_NAME
        && dir.parent()?.file_name()? == "libexec";
    ok.then(|| dir.parent()?.parent().map(Path::to_path_buf))
        .flatten()
}

/// The folders of an owned install's update, all inside `PREFIX/libexec/zaun/.update`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub prefix: PathBuf,
}

impl Layout {
    /// The installed file at `rel` (one of `TARBALL_FILES`).
    pub fn target(&self, rel: &str) -> PathBuf {
        self.prefix.join(rel)
    }

    pub fn update_dir(&self) -> PathBuf {
        self.prefix.join("libexec/zaun/.update")
    }

    /// A download in progress.
    pub fn staging(&self) -> PathBuf {
        self.update_dir().join("staging")
    }

    /// A complete, verified download waiting to be applied.
    pub fn staged(&self) -> PathBuf {
        self.update_dir().join("staged")
    }

    /// The files replaced by the last update, until the new version is confirmed.
    pub fn previous(&self) -> PathBuf {
        self.update_dir().join("previous")
    }

    /// The new version's trial (`trial`).
    pub fn trial_file(&self) -> PathBuf {
        self.update_dir().join("trial.json")
    }

    /// "Restart now" was pressed: apply and start again once the window is closed.
    pub fn restart_file(&self) -> PathBuf {
        self.update_dir().join("restart")
    }

    /// The binary that runs after an apply or a rollback.
    pub fn binary(&self) -> PathBuf {
        self.target(super::source::TARBALL_FILES[1])
    }
}

/// The file name of a tarball path (`libexec/zaun/zaun` → `zaun`).
pub(super) fn file_name(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}
