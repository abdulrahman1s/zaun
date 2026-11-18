//! Removing unused builds and leftovers safely (RUNTIME_MANAGEMENT §8, §6.4, §7; RT-013).
//!
//! - A store build is *removable* when no game resolves to it, it is not the newest of its
//!   family, and it has been unused for [`UNUSED_FOR`]. External builds are never removed.
//! - [`remove_build`] refuses external builds, the newest of a family and builds in use
//!   (with the list of games); the age rule applies to automatic removal only.
//! - [`expire_quarantine`] deletes quarantined runtimes older than [`QUARANTINE_KEEP`].
//! - [`sweep_session_var`] removes `session-var/<sid>` directories of inactive sessions.
//!
//! Every deletion goes through `fsx::safe_remove_tree` confined to the store.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::catalog::RuntimeCatalog;
use super::store::RuntimeStore;
use crate::clock::Timestamp;
use crate::error::{Error, ErrorCode, Result};
use crate::fsx::{FileType, Fs, WalkControl, WalkLimits, safe_remove_tree, safe_walk};
use crate::ids::{RuntimeName, SessionId};
use crate::model::{RuntimeBuild, RuntimeOrigin};

/// A build unused for this long may be removed automatically.
pub const UNUSED_FOR: Duration = Duration::from_secs(14 * 24 * 3600);
/// Quarantined runtimes are kept this long.
pub const QUARANTINE_KEEP: Duration = Duration::from_secs(7 * 24 * 3600);

/// Why a build is kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeepReason {
    /// Not managed by Zaun.
    External,
    /// Games resolve to it.
    InUse(Vec<String>),
    /// The newest build of its family.
    Newest,
    /// Used within [`UNUSED_FOR`].
    RecentlyUsed,
}

/// One row of the removal list in Settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovalCandidate {
    /// The build.
    pub name: RuntimeName,
    /// Its directory.
    pub path: PathBuf,
    /// Origin.
    pub origin: RuntimeOrigin,
    /// Size, when known.
    pub size_bytes: Option<u64>,
    /// `None`: removable (automatic removal allowed); otherwise why it stays.
    pub keep: Option<KeepReason>,
}

impl RemovalCandidate {
    /// Whether the user may remove it by hand (not external, not newest, not in use).
    pub fn manually_removable(&self) -> bool {
        matches!(self.keep, None | Some(KeepReason::RecentlyUsed))
    }
}

fn is_newest(catalog: &RuntimeCatalog, b: &RuntimeBuild) -> bool {
    catalog
        .builds
        .iter()
        .filter(|o| o.family == b.family && o.origin == RuntimeOrigin::Store)
        .max_by(|x, y| super::store::compare_builds(x.name.as_str(), y.name.as_str()))
        .is_some_and(|n| n.name == b.name)
}

/// Classify every build. `users(name)` lists the games that resolve to `name`.
pub fn candidates(
    catalog: &RuntimeCatalog,
    users: &dyn Fn(&RuntimeName) -> Vec<String>,
    now: Timestamp,
) -> Vec<RemovalCandidate> {
    catalog
        .builds
        .iter()
        .map(|b| {
            let in_use = users(&b.name);
            let keep = if b.origin == RuntimeOrigin::External {
                Some(KeepReason::External)
            } else if !in_use.is_empty() {
                Some(KeepReason::InUse(in_use))
            } else if is_newest(catalog, b) {
                Some(KeepReason::Newest)
            } else if b
                .last_used_at
                .or(Some(b.discovered_at))
                .and_then(|t| now.since(t))
                .is_none_or(|age| age < UNUSED_FOR)
            {
                Some(KeepReason::RecentlyUsed)
            } else {
                None
            };
            RemovalCandidate {
                name: b.name.clone(),
                path: b.path.clone(),
                origin: b.origin,
                size_bytes: b.size_bytes,
                keep,
            }
        })
        .collect()
}

/// Remove a store build by hand (Settings, `zaun runtimes remove`).
pub fn remove_build(
    fs: &dyn Fs,
    store: &RuntimeStore,
    catalog: &RuntimeCatalog,
    users: &dyn Fn(&RuntimeName) -> Vec<String>,
    name: &RuntimeName,
    now: Timestamp,
) -> Result<PathBuf> {
    let Some(c) = candidates(catalog, users, now)
        .into_iter()
        .find(|c| &c.name == name)
    else {
        return Err(Error::new(ErrorCode::RtNoLongerInstalled).with("build", name));
    };
    let refuse = |why: String| {
        Err(Error::new(ErrorCode::RtInvalidFolder)
            .with("build", name)
            .with("rule", why))
    };
    match &c.keep {
        Some(KeepReason::External) => {
            return refuse("not managed by Zaun; it is never removed".into());
        }
        Some(KeepReason::InUse(games)) => {
            return refuse(format!("in use by {}", games.join(", ")));
        }
        Some(KeepReason::Newest) => {
            return refuse("the newest build of its family is kept".into());
        }
        Some(KeepReason::RecentlyUsed) | None => {}
    }
    if !c.path.starts_with(&store.compat) || c.path.parent() != Some(store.compat.as_path()) {
        return refuse("outside the store".into());
    }
    let report = safe_remove_tree(fs, &store.compat, &c.path)?;
    if fs.exists(&c.path) {
        return Err(Error::new(ErrorCode::FsWriteDenied)
            .with("path", c.path.display())
            .with("detail", format!("{report:?}")));
    }
    Ok(c.path)
}

/// The apparent size of a directory tree (no symlink following, bounded).
pub fn tree_size(fs: &dyn Fs, clock: &dyn crate::clock::Clock, dir: &Path) -> u64 {
    let mut total = 0;
    let limits = WalkLimits {
        max_entries: super::manifest::MAX_ENTRIES,
        max_depth: super::manifest::MAX_DEPTH,
        deadline: None,
    };
    let _ = safe_walk(fs, dir, limits, clock, |e| {
        if e.metadata.file_type == FileType::File {
            total += e.metadata.size;
        }
        WalkControl::Continue
    });
    total
}

/// Delete quarantine directories older than [`QUARANTINE_KEEP`] (name `<variant>-<ms>`).
pub fn expire_quarantine(fs: &dyn Fs, store: &RuntimeStore, now: Timestamp) -> Vec<PathBuf> {
    let mut removed = Vec::new();
    let Ok(entries) = fs.read_dir(&store.quarantine) else {
        return removed;
    };
    for e in entries {
        let name = e.name.to_string_lossy().into_owned();
        let Some(ms) = name
            .rsplit_once('-')
            .and_then(|(_, ms)| ms.parse::<i64>().ok())
        else {
            continue;
        };
        let old = now
            .since(Timestamp::from_millis(ms))
            .is_some_and(|age| age >= QUARANTINE_KEEP);
        if old && safe_remove_tree(fs, &store.quarantine, &e.path).is_ok() {
            removed.push(e.path);
        }
    }
    removed
}

/// Remove `session-var/<sid>` directories whose session is not in `active`.
pub fn sweep_session_var(fs: &dyn Fs, store: &RuntimeStore, active: &[SessionId]) -> Vec<PathBuf> {
    let mut removed = Vec::new();
    let Ok(entries) = fs.read_dir(&store.session_var) else {
        return removed;
    };
    for e in entries {
        let sid = e.name.to_string_lossy().parse::<SessionId>().ok();
        if sid.is_some_and(|s| active.contains(&s)) {
            continue;
        }
        if safe_remove_tree(fs, &store.session_var, &e.path).is_ok() {
            removed.push(e.path);
        }
    }
    removed
}
