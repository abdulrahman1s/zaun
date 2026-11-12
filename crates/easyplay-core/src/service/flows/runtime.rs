//! Runtime commands (SVC-008, LOGIC_SPECS §16, RUNTIME_MANAGEMENT §6.3, §8): rescans,
//! adopting an SLR variant umu already installed for the user (`runtime::adopt`),
//! provisioning from Settings, removing a build, verifying the runtimes, and the background
//! audit after Shared-mode sessions and at start.
//!
//! Provision runs through the same `launch::LocalTrustedSteps` as the launch pipeline: it
//! takes the store lock and waits while a Shared-mode game session is open ("queued"), so a
//! Provision never starts during one. Results come back as `Internal::RuntimeJob`; tamper
//! notices (`EP-RT-020`/`021`) are reported as failures.
//!
//! [`CatalogUmuDb`] replaces the umu-database stubs of drafts and Steam artwork with the
//! real reader (`runtime::umu_db`) over the default (drafts) or the game's (artwork) build.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::super::app_service::{AppService, Internal};
use super::super::event::{Event, Notice};
use crate::error::{Error, ErrorCode};
use crate::fsx::Fs;
use crate::ids::{RuntimeName, SessionId};
use crate::jobs::{JobLabel, JobPriority};
use crate::launch::{LocalTrustedSteps, TrustedProgress, TrustedSteps, plan_provision};
use crate::model::{Game, RuntimeRef, SlrVariant};
use crate::runtime::provision::{check_postconditions, interpret};
use crate::runtime::{
    AdoptReport, AuditDue, ProvisionNeed, Resolution, RuntimeCatalog, RuntimeService, RuntimeStore,
    UmuDbCache, adopt_slr, gc,
};
use crate::storage::RuntimeRepo;

/// The result of a runtime job (provision, verify, remove, background audit).
#[derive(Debug, Default)]
pub(crate) struct RuntimeJobDone {
    /// Warnings and errors to show (`EP-RT-020/021`, failures).
    pub notices: Vec<Error>,
}

/// Background audits running (one at a time).
static AUDIT_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

impl AppService {
    pub(crate) fn scan_runtimes(&self) {
        let runtimes = Arc::clone(&self.deps.runtimes);
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Background,
            JobLabel("runtime-refresh"),
            move |_| {
                // `refresh` scans on another worker; the pool has at least two.
                if let Ok(c) = runtimes.refresh().wait() {
                    post(Internal::Runtimes(c));
                }
            },
        );
    }

    /// The trusted-session runner for launches and runtime commands.
    pub(crate) fn trusted_steps(&self) -> LocalTrustedSteps {
        LocalTrustedSteps {
            launcher: Arc::clone(&self.deps.launcher),
            fs: Arc::clone(&self.deps.fs),
            clock: Arc::clone(&self.deps.clock),
            proc: Arc::clone(&self.deps.proc),
            paths: self.deps.paths.clone(),
            store: RuntimeStore::new(&self.deps.paths),
            prepare_online: self.prefs.protection.prepare_online,
        }
    }

    pub(crate) fn adopt_runtime(&mut self, variant: SlrVariant) {
        let catalog = self.hub.runtimes.load();
        let Some(source) = catalog
            .foreign
            .slr
            .iter()
            .find(|s| s.variant == variant && s.is_complete())
            .map(|s| s.path.clone())
        else {
            return self.fail(
                Error::new(ErrorCode::RtInvalidFolder)
                    .with("variant", variant.as_str())
                    .with(
                        "rule",
                        "no complete install of this runtime outside the store",
                    ),
            );
        };
        let fs = Arc::clone(&self.deps.fs);
        let clock = Arc::clone(&self.deps.clock);
        let store = RuntimeStore::new(&self.deps.paths);
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Background,
            JobLabel("runtime-adopt"),
            move |cancel| {
                let now = clock.now();
                let r = adopt_slr(&*fs, &*clock, &store, &source, variant, now, cancel);
                post(Internal::Adopted(r));
            },
        );
    }

    pub(crate) fn on_adopted(&mut self, r: Result<AdoptReport, Error>) {
        let report = match r {
            Ok(r) => r,
            Err(e) => return self.fail(e),
        };
        if let Some(install) = &report.install {
            let now = self.deps.clock.now();
            let db = self.lib.db_mut();
            if let Err(e) = db
                .put_slr_install(install)
                .and_then(|()| db.record_audit_result(install.variant, now, "clean"))
            {
                tracing::warn!(error = %e, "runtime install not recorded");
            }
        }
        self.notice(Notice::RuntimeAdopted {
            variant: report.variant,
            reflinked: report.files > 0 && report.reflinked == report.files,
        });
        self.scan_runtimes();
    }

    /// `ProvisionRuntime { target }`: install (or update, for a channel) a build and its
    /// runtime in a Provision session.
    pub(crate) fn provision_runtime(&mut self, target: RuntimeRef) {
        let need = match &target {
            RuntimeRef::Channel(c) => Some(ProvisionNeed::Codename(*c)),
            other => match self.deps.runtimes.resolve(other, None) {
                Resolution::NeedsProvision(n) => Some(n),
                Resolution::Ready(_) => None,
                Resolution::Unavailable(r) => {
                    return self.fail(Error::new(r.code()).with("reason", format!("{r:?}")));
                }
            },
        };
        let Some(need) = need else {
            // Already installed: nothing to download.
            self.emit(Event::RuntimesChanged);
            return;
        };
        let steps = self.trusted_steps();
        let runtimes = Arc::clone(&self.deps.runtimes);
        let host = Arc::clone(&self.deps.host);
        let env = self.deps.launch_env.clone();
        let paths = self.deps.paths.clone();
        let install_id = self.install_id.clone();
        let fs = Arc::clone(&self.deps.fs);
        let clock = Arc::clone(&self.deps.clock);
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Background,
            JobLabel("runtime-provision"),
            move |cancel| {
                let r = (|| -> Result<(), Error> {
                    let facts = host.facts();
                    let henv = env.host_env(&paths, &install_id, &[], &*fs);
                    let sid = SessionId::generate(&*clock);
                    let planned = plan_provision(&need, &runtimes.catalog(), &facts, &henv, sid)?;
                    let end = steps.run(
                        &planned,
                        true,
                        &mut |p| match p {
                            TrustedProgress::Waiting => {
                                tracing::info!("provision queued behind a running session");
                            }
                            TrustedProgress::Phase(p) => tracing::info!(phase = ?p, "provision"),
                            TrustedProgress::Bytes { bytes, rate } => {
                                tracing::debug!(bytes, rate, "provision download");
                            }
                        },
                        cancel,
                    )?;
                    interpret(&end)?;
                    steps.after_install()?;
                    let catalog = runtimes.refresh().wait().map_err(|e| {
                        Error::new(ErrorCode::IntTaskFailed).with("detail", e.to_string())
                    })?;
                    check_postconditions(&catalog, &need)?;
                    Ok(())
                })();
                let mut done = RuntimeJobDone::default();
                if let Err(e) = r {
                    done.notices.push(e);
                }
                post(Internal::RuntimeJob(done));
            },
        );
    }

    /// `RemoveRuntime { name }` (RT-013): refused for external builds, the newest of a
    /// family and builds in use.
    pub(crate) fn remove_runtime(&mut self, name: RuntimeName) {
        let catalog = self.deps.runtimes.catalog();
        let games: Vec<Game> = self
            .lib
            .snapshot()
            .games
            .iter()
            .filter_map(|g| self.lib.game(g.id).ok())
            .collect();
        let runtimes = Arc::clone(&self.deps.runtimes);
        let users = move |n: &RuntimeName| users_of(&*runtimes, &games, n);
        let store = RuntimeStore::new(&self.deps.paths);
        let r = gc::remove_build(
            &*self.deps.fs,
            &store,
            &catalog,
            &users,
            &name,
            self.deps.clock.now(),
        );
        match r {
            Ok(path) => {
                tracing::info!(build = %name, path = %path.display(), "runtime removed");
                let _ = self
                    .lib
                    .db_mut()
                    .add_runtime_event(&crate::storage::RuntimeEvent {
                        id: 0,
                        at: self.deps.clock.now(),
                        variant: None,
                        kind: "removed".into(),
                        summary_json: serde_json::json!({ "build": name.as_str() }).to_string(),
                    });
                self.scan_runtimes();
            }
            Err(e) => self.fail(e),
        }
    }

    /// `VerifyRuntime`: audit every installed variant now ("Verify now").
    pub(crate) fn verify_runtimes(&mut self) {
        self.audit_runtimes(true);
    }

    /// Audit the variants with `pending = 1` in the background (after a Shared-mode session
    /// and at start, §6.3), or every installed variant when `all`.
    pub(crate) fn audit_runtimes(&mut self, all: bool) {
        let steps = self.trusted_steps();
        let catalog = self.hub.runtimes.load();
        let variants: Vec<SlrVariant> = [SlrVariant::Steamrt3, SlrVariant::Steamrt4]
            .into_iter()
            .filter(|v| all || steps.audit_due(*v) == AuditDue::Pending)
            .filter(|v| !all || catalog.slr(*v).is_some_and(|s| s.is_complete()))
            .collect();
        if variants.is_empty() {
            return;
        }
        let post = self.poster();
        let priority = if all {
            JobPriority::Interactive
        } else {
            JobPriority::Background
        };
        let _ = self
            .deps
            .jobs
            .submit(priority, JobLabel("runtime-audit"), move |cancel| {
                let _one = AUDIT_LOCK.lock();
                let mut done = RuntimeJobDone::default();
                for v in variants {
                    match steps.audit(v, cancel) {
                        Ok(action) => done.notices.extend(action.notice()),
                        Err(e) => done.notices.push(e),
                    }
                }
                post(Internal::RuntimeJob(done));
            });
    }

    /// A Shared-mode session ended: audit in the background.
    pub(crate) fn after_session_ended(&mut self) {
        self.audit_runtimes(false);
    }

    pub(crate) fn on_runtime_job(&mut self, done: RuntimeJobDone) {
        for n in done.notices {
            self.fail(n);
        }
        self.scan_runtimes();
    }
}

/// The names of the games that resolve to `name` (pinned to it, or resolving to it now).
pub(crate) fn users_of(
    runtimes: &dyn RuntimeService,
    games: &[Game],
    name: &RuntimeName,
) -> Vec<String> {
    games
        .iter()
        .filter(|g| match &g.runtime {
            RuntimeRef::Pinned(n) => n == name,
            r => matches!(runtimes.resolve(r, Some(g)), Resolution::Ready(rt) if &rt.name == name),
        })
        .map(|g| g.name.clone())
        .collect()
}

/// The umu database over the runtime catalogue (UMU_INTEGRATION §8): drafts match titles
/// in the default build's database; Steam artwork looks up the game's resolved build.
pub struct CatalogUmuDb {
    fs: Arc<dyn Fs>,
    runtimes: Arc<dyn RuntimeService>,
    cache: UmuDbCache,
    warned: AtomicBool,
}

impl CatalogUmuDb {
    /// A reader over `runtimes`.
    pub fn new(fs: Arc<dyn Fs>, runtimes: Arc<dyn RuntimeService>) -> Self {
        Self {
            fs,
            runtimes,
            cache: UmuDbCache::new(),
            warned: AtomicBool::new(false),
        }
    }

    /// The build directory whose database applies to `game` (default reference without a
    /// game); falls back to the newest build that ships fixes.
    fn build_dir(&self, game: Option<&Game>) -> Option<std::path::PathBuf> {
        let reference = game.map_or(RuntimeRef::Default, |g| g.runtime.clone());
        match self.runtimes.resolve(&reference, game) {
            Resolution::Ready(rt) => Some(rt.proton_path_host),
            _ => {
                let c: Arc<RuntimeCatalog> = self.runtimes.catalog();
                c.builds
                    .iter()
                    .rev()
                    .find(|b| b.has_fixes)
                    .map(|b| b.path.clone())
            }
        }
    }

    fn db(&self, game: Option<&Game>) -> Option<Arc<crate::runtime::UmuDb>> {
        let dir = self.build_dir(game)?;
        let db = self.cache.get(&*self.fs, &dir);
        if db.is_none() && !self.warned.swap(true, Ordering::Relaxed) {
            tracing::debug!(build = %dir.display(), "no umu database in the build");
        }
        db
    }

    /// The Steam app ID of `game` from the database (artwork step 3.3).
    pub fn steam_app_id_of(&self, game: &Game) -> Option<u32> {
        self.db(Some(game))?
            .steam_app_id(game.umu_id.as_deref(), &game.name)
    }
}

impl std::fmt::Debug for CatalogUmuDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CatalogUmuDb").finish_non_exhaustive()
    }
}

impl crate::library::draft::UmuTitleMatch for CatalogUmuDb {
    fn exact_title(&self, name: &str) -> Option<String> {
        self.db(None)?.exact_title(name)
    }
}

impl crate::artwork::steam::UmuDbSteamId for CatalogUmuDb {
    fn steam_app_id(&self, game: &Game) -> Option<u32> {
        self.steam_app_id_of(game)
    }
}
