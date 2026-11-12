//! The runtime catalogue snapshot and its synchronisation with the `runtimes` and
//! `slr_installs` tables (RT-004, RUNTIME_MANAGEMENT §10).
//!
//! Synchronisation is written against the small [`CatalogStore`] trait.
//! [`SqliteCatalogStore`] implements it on the `runtimes` table through
//! `storage::RuntimeRepo` (STORE-012); [`MemCatalogStore`] is the in-memory version.

use std::path::Path;

use parking_lot::Mutex;

use super::discovery::{Discovery, ForeignUmu, Rejected, SlrInstall};
use super::store::compare_builds;
use crate::clock::Timestamp;
use crate::ids::RuntimeName;
use crate::model::{RuntimeBuild, RuntimeFamily, RuntimeOrigin, SlrVariant};

/// An immutable snapshot of what is installed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeCatalog {
    /// Valid Proton builds, sorted by family, then natural version (newest last).
    pub builds: Vec<RuntimeBuild>,
    /// SLR variants in the store.
    pub slr: Vec<SlrInstall>,
    /// Candidates that failed validation (shown in Settings with the rule that failed).
    pub rejected: Vec<Rejected>,
    /// umu's own runtime and cache outside the store (informational).
    pub foreign: ForeignUmu,
    /// When the scan ran.
    pub scanned_at: Option<Timestamp>,
}

impl RuntimeCatalog {
    /// Build a snapshot from a discovery result.
    pub fn from_discovery(d: Discovery, at: Timestamp) -> Self {
        let mut builds = d.builds;
        builds.sort_by(|a, b| {
            a.family
                .as_str()
                .cmp(b.family.as_str())
                .then_with(|| compare_builds(a.name.as_str(), b.name.as_str()))
        });
        Self {
            builds,
            slr: d.slr,
            rejected: d.rejected,
            foreign: d.foreign,
            scanned_at: Some(at),
        }
    }

    /// The build named `name`.
    pub fn by_name(&self, name: &RuntimeName) -> Option<&RuntimeBuild> {
        self.builds.iter().find(|b| &b.name == name)
    }

    /// The build at `path`.
    pub fn by_path(&self, path: &Path) -> Option<&RuntimeBuild> {
        self.builds.iter().find(|b| b.path == path)
    }

    /// The newest build of a family by natural version order; a store build wins a tie.
    pub fn newest(&self, family: RuntimeFamily) -> Option<&RuntimeBuild> {
        self.builds
            .iter()
            .filter(|b| b.family == family)
            .max_by(|a, b| {
                compare_builds(a.name.as_str(), b.name.as_str()).then_with(|| {
                    (a.origin == RuntimeOrigin::Store).cmp(&(b.origin == RuntimeOrigin::Store))
                })
            })
    }

    /// The store's install of `variant`.
    pub fn slr(&self, variant: SlrVariant) -> Option<&SlrInstall> {
        self.slr.iter().find(|s| s.variant == variant)
    }
}

/// Persistence of the catalogue rows.
pub trait CatalogStore: Send + Sync {
    /// Every stored build.
    fn load_builds(&self) -> Vec<RuntimeBuild>;
    /// Apply a diff in one transaction.
    fn apply(&self, diff: &CatalogDiff);
}

/// Changes between the stored rows and a scan.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogDiff {
    /// New rows.
    pub added: Vec<RuntimeBuild>,
    /// Rows whose scanned fields changed (stored `discovered_at`, `last_used_at` and
    /// `size_bytes` are kept).
    pub updated: Vec<RuntimeBuild>,
    /// Names no longer found.
    pub removed: Vec<RuntimeName>,
}

impl CatalogDiff {
    /// Nothing to do.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.updated.is_empty() && self.removed.is_empty()
    }
}

fn same_scan(a: &RuntimeBuild, b: &RuntimeBuild) -> bool {
    a.family == b.family
        && a.version_key == b.version_key
        && a.origin == b.origin
        && a.origin_label == b.origin_label
        && a.path == b.path
        && a.required_slr == b.required_slr
        && a.display_name == b.display_name
        && a.has_fixes == b.has_fixes
}

/// Compute the diff from stored rows to scanned builds.
pub fn diff(stored: &[RuntimeBuild], scanned: &[RuntimeBuild]) -> CatalogDiff {
    let mut d = CatalogDiff::default();
    for s in scanned {
        match stored.iter().find(|o| o.name == s.name) {
            None => d.added.push(s.clone()),
            Some(old) if !same_scan(old, s) => d.updated.push(RuntimeBuild {
                discovered_at: old.discovered_at,
                last_used_at: old.last_used_at,
                size_bytes: if old.path == s.path {
                    old.size_bytes
                } else {
                    None
                },
                ..s.clone()
            }),
            Some(_) => {}
        }
    }
    for o in stored {
        if !scanned.iter().any(|s| s.name == o.name) {
            d.removed.push(o.name.clone());
        }
    }
    d
}

/// Synchronise `store` with a scan and return the snapshot with the stored timestamps.
pub fn sync(
    store: &dyn CatalogStore,
    scanned: Discovery,
    at: Timestamp,
) -> (RuntimeCatalog, CatalogDiff) {
    let stored = store.load_builds();
    let d = diff(&stored, &scanned.builds);
    if !d.is_empty() {
        store.apply(&d);
    }
    let rows = store.load_builds();
    let mut catalog = RuntimeCatalog::from_discovery(scanned, at);
    for b in &mut catalog.builds {
        if let Some(r) = rows.iter().find(|r| r.name == b.name) {
            b.discovered_at = r.discovered_at;
            b.last_used_at = r.last_used_at;
            b.size_bytes = r.size_bytes;
        }
    }
    (catalog, d)
}

/// [`CatalogStore`] on the library database (`runtimes` table). The trait is infallible:
/// storage errors are logged, a failed load reads as an empty catalogue and a failed apply
/// leaves the table unchanged (the next scan retries).
#[derive(Debug)]
pub struct SqliteCatalogStore {
    db: Mutex<crate::storage::Db>,
}

impl SqliteCatalogStore {
    /// Wrap a read-write handle.
    pub fn new(db: crate::storage::Db) -> Self {
        Self { db: Mutex::new(db) }
    }

    /// Give the handle back.
    pub fn into_inner(self) -> crate::storage::Db {
        self.db.into_inner()
    }
}

impl CatalogStore for SqliteCatalogStore {
    fn load_builds(&self) -> Vec<RuntimeBuild> {
        use crate::storage::RuntimeRepo;
        self.db.lock().runtimes().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "cannot read the runtime catalogue");
            Vec::new()
        })
    }

    fn apply(&self, diff: &CatalogDiff) {
        use crate::storage::RuntimeRepo;
        let upsert: Vec<RuntimeBuild> = diff.added.iter().chain(&diff.updated).cloned().collect();
        if let Err(e) = self.db.lock().apply_runtime_changes(&upsert, &diff.removed) {
            tracing::warn!(error = %e, "cannot store the runtime catalogue");
        }
    }
}

/// In-memory [`CatalogStore`].
#[derive(Debug, Default)]
pub struct MemCatalogStore {
    rows: Mutex<Vec<RuntimeBuild>>,
    applies: Mutex<usize>,
}

impl MemCatalogStore {
    /// Empty.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many non-empty diffs were applied.
    pub fn applies(&self) -> usize {
        *self.applies.lock()
    }
}

impl CatalogStore for MemCatalogStore {
    fn load_builds(&self) -> Vec<RuntimeBuild> {
        self.rows.lock().clone()
    }

    fn apply(&self, diff: &CatalogDiff) {
        let mut rows = self.rows.lock();
        rows.retain(|r| !diff.removed.contains(&r.name));
        for u in &diff.updated {
            if let Some(r) = rows.iter_mut().find(|r| r.name == u.name) {
                *r = u.clone();
            }
        }
        rows.extend(diff.added.iter().cloned());
        *self.applies.lock() += 1;
    }
}
