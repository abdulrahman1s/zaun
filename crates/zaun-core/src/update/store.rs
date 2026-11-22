//! The bundle store (ADR-0041 §2): `<data>/versions/` holds one folder per downloaded
//! version and the links that choose between them; `<data>/components/` holds the folders
//! versions share (the Chat engine), named by their hash.
//!
//! ```text
//! versions/0.3.0/{zaun, zaun-init, zaun-overlay, zaun-chat, simplex -> ../../components/simplex-…}
//! versions/current -> 0.3.0     what the installed copy starts
//! versions/previous -> 0.2.9    until 0.3.0 is confirmed
//! versions/staged -> 0.3.1      downloaded and verified, switched to at the next start
//! ```

use std::path::{Path, PathBuf};

use super::UpdateError;
use super::source::BUNDLE_BINARY;
use super::version::Version;
use crate::fsx::{Fs, FsErrorKind};
use crate::paths::AppPaths;

/// Where versions and components live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Store {
    pub versions: PathBuf,
    pub components: PathBuf,
}

impl Store {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            versions: paths.data_dir.join("versions"),
            components: paths.data_dir.join("components"),
        }
    }

    pub fn version_dir(&self, v: &Version) -> PathBuf {
        self.versions.join(v.to_string())
    }

    /// The version the installed copy starts.
    pub fn current(&self) -> PathBuf {
        self.versions.join("current")
    }

    /// The version before `current`, kept until `current` is confirmed.
    pub fn previous(&self) -> PathBuf {
        self.versions.join("previous")
    }

    /// A verified download waiting for the next start.
    pub fn staged(&self) -> PathBuf {
        self.versions.join("staged")
    }

    /// A download in progress.
    pub fn staging(&self) -> PathBuf {
        self.versions.join(".staging")
    }

    /// The trial of `current` (`trial`).
    pub fn trial_file(&self) -> PathBuf {
        self.versions.join("trial.json")
    }

    /// "Restart now" was pressed: switch and start again once the window is closed.
    pub fn restart_file(&self) -> PathBuf {
        self.versions.join("restart")
    }

    /// The shared copy of folder `name` with this hash.
    pub fn component(&self, name: &str, sha256: &str) -> PathBuf {
        let short = sha256.get(..16).unwrap_or(sha256);
        self.components.join(format!("{name}-{short}"))
    }

    /// A component being unpacked.
    pub fn component_staging(&self) -> PathBuf {
        self.components.join(".staging")
    }

    /// The program of a version folder.
    pub fn binary(dir: &Path) -> PathBuf {
        dir.join(BUNDLE_BINARY)
    }

    /// Whether `exe` (resolved) is a program of this store.
    pub fn contains(&self, fs: &dyn Fs, exe: &Path) -> bool {
        exe.starts_with(&self.versions)
            || fs
                .canonicalize(&self.versions)
                .is_ok_and(|root| exe.starts_with(root))
    }

    /// The version a link names, with its folder; `None` when the link is missing, names
    /// anything but a version folder beside it, or that folder is gone.
    pub fn linked(&self, fs: &dyn Fs, link: &Path) -> Option<(Version, PathBuf)> {
        let target = fs.read_link(link).ok()?;
        let name = target.to_str()?;
        let v = Version::parse(name).filter(|v| v.to_string() == name)?;
        let dir = self.versions.join(name);
        fs.metadata(&dir).ok()?.is_dir().then_some((v, dir))
    }

    /// Point `link` at the folder of `v`: a new link renamed over the old one.
    pub fn set_link(&self, fs: &dyn Fs, link: &Path, v: &Version) -> Result<(), UpdateError> {
        let tmp = link.with_extension("new");
        remove_if_present(fs, &tmp)?;
        fs.symlink(Path::new(&v.to_string()), &tmp)?;
        fs.rename(&tmp, link)?;
        Ok(())
    }

    /// Keep the folders of `keep` and the components they link to; remove every other
    /// version folder, component and unfinished download. Best effort.
    pub fn prune(&self, fs: &dyn Fs, keep: &[Version]) {
        let kept: Vec<PathBuf> = keep.iter().map(|v| self.version_dir(v)).collect();
        let mut used: Vec<PathBuf> = Vec::new();
        for dir in &kept {
            for e in fs.read_dir(dir).unwrap_or_default() {
                if let Ok(t) = fs.read_link(&dir.join(&e.name))
                    && let Some(name) = t.file_name()
                {
                    used.push(self.components.join(name));
                }
            }
        }
        for e in fs.read_dir(&self.versions).unwrap_or_default() {
            let path = self.versions.join(&e.name);
            let version = e.name.to_str().and_then(Version::parse).is_some();
            if (version || e.name == ".staging") && !kept.contains(&path) {
                // Best effort: a folder left behind is removed by the next prune.
                let _ = clear_dir(fs, &path);
            }
        }
        for e in fs.read_dir(&self.components).unwrap_or_default() {
            let path = self.components.join(&e.name);
            if !used.contains(&path) {
                let _ = clear_dir(fs, &path); // as above
            }
        }
    }
}

/// Remove a folder of files and links (the store has no deeper folders); missing is fine.
pub(super) fn clear_dir(fs: &dyn Fs, dir: &Path) -> Result<(), UpdateError> {
    let entries = match fs.read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind == FsErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    for e in entries {
        fs.remove_file(&dir.join(&e.name))?;
    }
    fs.remove_dir(dir)?;
    Ok(())
}

pub(super) fn remove_if_present(fs: &dyn Fs, path: &Path) -> Result<(), UpdateError> {
    match fs.remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind == FsErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
