//! `LocalRuntimeService`: the M3 skeleton of [`RuntimeService`] (RT-004) and Doctor check
//! D-15.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;

use super::RuntimeService;
use super::catalog::{CatalogStore, RuntimeCatalog, sync};
use super::discovery::{DiscoveryInput, RejectReason, discover, validate_build};
use super::resolver::{IsolationInputs, Resolution, ResolveInput, UnavailableReason, resolve};
use super::store::RuntimeStore;
use crate::clock::Clock;
use crate::fsx::Fs;
use crate::host::{Check, CheckStatus};
use crate::jobs::{JobHandle, JobLabel, JobPool, JobPriority};
use crate::model::{Channel, Game, RuntimeOrigin, RuntimeRef};

/// Preferences the service needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSettings {
    /// `runtime.default`.
    pub default: RuntimeRef,
    /// `runtime.extra_search_dirs`.
    pub extra_search_dirs: Vec<PathBuf>,
    /// `protection.always_isolate_runtime`.
    pub always_isolate: bool,
    /// `HostFacts.fs.reflink_ok`.
    pub reflink_ok: Option<bool>,
}

impl Default for RuntimeSettings {
    fn default() -> Self {
        Self {
            default: RuntimeRef::Channel(Channel::UmuProton),
            extra_search_dirs: Vec::new(),
            always_isolate: false,
            reflink_ok: None,
        }
    }
}

struct Inner {
    fs: Arc<dyn Fs>,
    clock: Arc<dyn Clock>,
    rows: Arc<dyn CatalogStore>,
    store: RuntimeStore,
    home: PathBuf,
    settings: RuntimeSettings,
    current: Mutex<Arc<RuntimeCatalog>>,
}

impl Inner {
    fn scan(&self) -> Arc<RuntimeCatalog> {
        let now = self.clock.now();
        let d = discover(
            &*self.fs,
            &DiscoveryInput {
                store: &self.store,
                home: Some(&self.home),
                extra_dirs: &self.settings.extra_search_dirs,
                now,
            },
        );
        let (catalog, diff) = sync(&*self.rows, d, now);
        if !diff.is_empty() {
            tracing::info!(
                added = diff.added.len(),
                updated = diff.updated.len(),
                removed = diff.removed.len(),
                "runtime catalogue changed"
            );
        }
        let catalog = Arc::new(catalog);
        *self.current.lock() = Arc::clone(&catalog);
        catalog
    }
}

/// The runtime service over a filesystem and a catalogue store.
pub struct LocalRuntimeService {
    inner: Arc<Inner>,
    jobs: Arc<JobPool>,
}

impl LocalRuntimeService {
    /// A service with an empty snapshot; call [`LocalRuntimeService::scan_now`] or
    /// [`RuntimeService::refresh`].
    pub fn new(
        fs: Arc<dyn Fs>,
        clock: Arc<dyn Clock>,
        rows: Arc<dyn CatalogStore>,
        store: RuntimeStore,
        home: PathBuf,
        settings: RuntimeSettings,
        jobs: Arc<JobPool>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                fs,
                clock,
                rows,
                store,
                home,
                settings,
                current: Mutex::new(Arc::new(RuntimeCatalog::default())),
            }),
            jobs,
        }
    }

    /// Scan on the calling thread (CLI).
    pub fn scan_now(&self) -> Arc<RuntimeCatalog> {
        self.inner.scan()
    }
}

impl RuntimeService for LocalRuntimeService {
    fn catalog(&self) -> Arc<RuntimeCatalog> {
        Arc::clone(&self.inner.current.lock())
    }

    fn refresh(&self) -> JobHandle<Arc<RuntimeCatalog>> {
        let inner = Arc::clone(&self.inner);
        self.jobs.submit(
            JobPriority::Background,
            JobLabel("runtime-scan"),
            move |_| inner.scan(),
        )
    }

    fn resolve(&self, r: &RuntimeRef, game: Option<&Game>) -> Resolution {
        let inner = &self.inner;
        let mut catalog = self.catalog();
        if let RuntimeRef::External(path) = r
            && catalog.by_path(path).is_none()
        {
            match validate_build(
                &*inner.fs,
                path,
                RuntimeOrigin::External,
                "Custom",
                Some(&inner.home),
                inner.clock.now(),
            ) {
                Ok(b) => {
                    let mut c = (*catalog).clone();
                    c.builds.push(b);
                    catalog = Arc::new(c);
                }
                Err(RejectReason::HostRuntime | RejectReason::UnsupportedRuntime(_)) => {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .and_then(|n| crate::ids::RuntimeName::new(n).ok());
                    return match name {
                        Some(n) => {
                            Resolution::Unavailable(UnavailableReason::UnsupportedRuntime(n))
                        }
                        None => Resolution::Unavailable(UnavailableReason::ExternalInvalid(
                            path.clone(),
                        )),
                    };
                }
                Err(_) => {
                    return Resolution::Unavailable(UnavailableReason::ExternalInvalid(
                        path.clone(),
                    ));
                }
            }
        }
        let input = ResolveInput {
            reference: r,
            default: &inner.settings.default,
            follow_updates: game.is_none_or(|g| g.runtime_follow_updates),
            last_resolved: game.and_then(|g| g.runtime_last_resolved.as_ref()),
            store: &inner.store,
            home: &inner.home,
            isolation: IsolationInputs {
                policy: game.map_or(crate::model::RuntimeIsolation::Auto, |g| {
                    g.policy.hardening.runtime_isolation
                }),
                always_isolate: inner.settings.always_isolate,
                reflink_ok: inner.settings.reflink_ok,
            },
        };
        resolve(&input, &catalog)
    }
}

/// Doctor check D-15: compatibility layers and runtime variants.
pub fn catalog_check(catalog: &RuntimeCatalog) -> Check {
    let builds = catalog.builds.len();
    let complete: Vec<&str> = catalog
        .slr
        .iter()
        .filter(|s| s.is_complete())
        .map(|s| s.variant.as_str())
        .collect();
    let mut summary = format!(
        "{builds} compatibility layer(s) ({} managed, {} external); runtime: {}",
        catalog
            .builds
            .iter()
            .filter(|b| b.origin == RuntimeOrigin::Store)
            .count(),
        catalog
            .builds
            .iter()
            .filter(|b| b.origin == RuntimeOrigin::External)
            .count(),
        if complete.is_empty() {
            "none installed".to_owned()
        } else {
            complete.join(", ")
        }
    );
    if !catalog.foreign.slr.is_empty() {
        let v: Vec<&str> = catalog
            .foreign
            .slr
            .iter()
            .map(|s| s.variant.as_str())
            .collect();
        summary.push_str(&format!(
            "; umu's own runtime ({}) in ~/.local/share/umu is not used",
            v.join(", ")
        ));
    }
    if !catalog.foreign.partial_downloads.is_empty() {
        summary.push_str(&format!(
            "; {} partial download(s) in ~/.cache/umu",
            catalog.foreign.partial_downloads.len()
        ));
    }
    let status = if builds == 0 || complete.is_empty() {
        CheckStatus::Warn
    } else {
        CheckStatus::Pass
    };
    let mut c = Check::new("D-15", status, summary);
    if status == CheckStatus::Warn {
        c = c.fix(
            "EasyPlay downloads what is missing before the first launch (internet needed once).",
        );
    }
    c
}
