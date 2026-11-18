//! `<state>/update.json`: the last check, the newest verified version and what was already
//! said, shared by the GUI, the resident service and the CLI without the library lock.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::manifest::Release;
use super::source::CHECK_EVERY;
use super::version::Version;
use crate::clock::Timestamp;
use crate::fsx::Fs;
use crate::model::UpdateOffer;
use crate::paths::AppPaths;

/// Largest state file read.
const MAX_STATE: u64 = 16 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateState {
    #[serde(default)]
    pub last_checked: Option<Timestamp>,
    /// From the last verified manifest.
    #[serde(default)]
    pub latest: Option<Latest>,
    /// The version the toast already announced.
    #[serde(default)]
    pub announced: Option<String>,
    /// A version that did not start and was rolled back.
    #[serde(default)]
    pub rolled_back: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Latest {
    pub version: String,
    pub date: String,
    pub notes: String,
    pub notes_url: Option<String>,
}

impl Latest {
    pub fn of(r: &Release) -> Self {
        Self {
            version: r.version.to_string(),
            date: r.date.clone(),
            notes: r.notes.clone(),
            notes_url: r.notes_url.clone(),
        }
    }
}

/// The state file of `paths`.
pub fn state_file(paths: &AppPaths) -> PathBuf {
    paths.state_dir.join("update.json")
}

impl UpdateState {
    /// Read the file; missing or unreadable is the empty state.
    pub fn load(fs: &dyn Fs, path: &Path) -> Self {
        fs.read(path, MAX_STATE)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Write it atomically, user-only.
    pub fn save(&self, fs: &dyn Fs, path: &Path) -> crate::Result<()> {
        if let Some(dir) = path.parent() {
            fs.create_dir_all(dir, 0o700)?;
        }
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| err!(IntTaskFailed, detail = e.to_string()))?;
        fs.atomic_write(path, &bytes, 0o600)?;
        Ok(())
    }

    /// Whether an automatic check is due at `now` (never checked, a day passed, or the clock
    /// went back).
    pub fn due(&self, now: Timestamp) -> bool {
        match self.last_checked {
            None => true,
            Some(t) => now.since(t).is_none_or(|d| d >= CHECK_EVERY),
        }
    }

    /// The offer to show while running `running` (an installed or older version is none).
    pub fn offer(&self, running: &Version) -> Option<UpdateOffer> {
        let l = self.latest.as_ref()?;
        let v = Version::parse(&l.version)?;
        (v > *running).then(|| UpdateOffer {
            version: l.version.clone(),
            date: l.date.clone(),
            notes: l.notes.clone(),
            has_notes: l.notes_url.is_some(),
        })
    }
}
