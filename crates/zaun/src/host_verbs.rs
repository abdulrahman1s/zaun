//! `zaun doctor [--json]` (HOST-009), `zaun runtimes list` (RT-004, with sizes of
//! the store's builds and runtimes) and `zaun runtimes adopt`.

use std::path::PathBuf;
use std::sync::Arc;

use zaun_core::Error;
use zaun_core::clock::{Clock, SystemClock};
use zaun_core::consts::{INIT_BIN_NAME, LIBEXEC_DIR};
use zaun_core::diagnostics::doctor_report;
use zaun_core::error::ErrorCode;
use zaun_core::fsx::{Fs, RealFs};
use zaun_core::host::{
    CheckStatus, DoctorContext, HostProbe, LiveCompositor, LiveHostProbe, ProbeDeps, RealAccess,
    RealRunner,
};
use zaun_core::jobs::CancelToken;
use zaun_core::jobs::{JobPool, JobPoolConfig};
use zaun_core::model::SlrVariant;
use zaun_core::paths::{AppPaths, PathEnv};
use zaun_core::runtime::{
    LocalRuntimeService, MemCatalogStore, RuntimeCatalog, RuntimeSettings, RuntimeStore,
};
use zaun_core::runtime::{adopt_slr, adoptable};
use zaun_core::storage::lock::{LibraryLock, LockMode};
use zaun_core::storage::{RuntimeRepo, StorageError};

struct Ctx {
    env: PathEnv,
    paths: AppPaths,
    fs: Arc<dyn Fs>,
    clock: Arc<dyn Clock>,
    jobs: Arc<JobPool>,
}

fn ctx() -> Result<Ctx, Error> {
    let env = PathEnv::from_process();
    let paths = AppPaths::resolve(&env)
        .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with("paths", e))?;
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let jobs = Arc::new(
        JobPool::new(JobPoolConfig::default(), Arc::clone(&clock))
            .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with_source(e))?,
    );
    Ok(Ctx {
        env,
        paths,
        fs: Arc::new(RealFs::new()),
        clock,
        jobs,
    })
}

fn scan_runtimes(c: &Ctx, reflink_ok: Option<bool>) -> Arc<RuntimeCatalog> {
    let home = c.paths.home.clone().unwrap_or_else(|| PathBuf::from("/"));
    LocalRuntimeService::new(
        Arc::clone(&c.fs),
        Arc::clone(&c.clock),
        Arc::new(MemCatalogStore::new()),
        RuntimeStore::new(&c.paths),
        home,
        RuntimeSettings {
            reflink_ok,
            ..RuntimeSettings::default()
        },
        Arc::clone(&c.jobs),
    )
    .scan_now()
}

fn json<T: serde::Serialize>(v: &T) -> Result<String, Error> {
    serde_json::to_string_pretty(v).map_err(|e| Error::new(ErrorCode::IntTaskFailed).with_source(e))
}

/// `zaun doctor [--json]`: probe afresh, print the ordered checks.
#[allow(clippy::print_stdout)]
pub fn doctor(as_json: bool) -> Result<(), Error> {
    let c = ctx()?;
    let user_name = c
        .env
        .get("USER")
        .or_else(|| c.env.get("LOGNAME"))
        .and_then(|u| u.to_str())
        .map(str::to_owned);
    let probe = LiveHostProbe::new(ProbeDeps {
        fs: Arc::clone(&c.fs),
        runner: Arc::new(RealRunner::new(Arc::clone(&c.clock))),
        access: Arc::new(RealAccess),
        clock: Arc::clone(&c.clock),
        env: c.env.clone(),
        paths: c.paths.clone(),
        umu_override: None,
        user_name,
        jobs: Some(Arc::clone(&c.jobs)),
        compositor: Arc::new(LiveCompositor),
    });
    let facts = probe.refresh();
    let mut init_candidates = vec![PathBuf::from(LIBEXEC_DIR).join(INIT_BIN_NAME)];
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
    {
        init_candidates.push(dir.join(INIT_BIN_NAME));
    }
    let dctx = DoctorContext {
        fs: &*c.fs,
        paths: &c.paths,
        uid: c.env.uid(),
        init_candidates,
    };
    let mut report = doctor_report(&facts, &dctx, &scan_runtimes(&c, facts.fs.reflink_ok));
    let runner = RealRunner::new(Arc::clone(&c.clock));
    zaun_core::diagnostics::complete(
        &mut report,
        &dctx,
        &zaun_core::diagnostics::DoctorProbes {
            runner: Some(&runner),
            db_file: &c.paths.db_file,
        },
    );
    if as_json {
        println!("{}", json(&report)?);
        return Ok(());
    }
    for check in &report.checks {
        let mark = match check.status {
            CheckStatus::Pass => "ok  ",
            CheckStatus::Warn => "warn",
            CheckStatus::Fail => "FAIL",
            CheckStatus::Skipped => "skip",
        };
        println!("{mark} {} {}", check.id, check.summary);
        if let Some(fix) = &check.fix {
            println!("          {fix}");
        }
    }
    Ok(())
}

/// `zaun runtimes list`.
#[allow(clippy::print_stdout)]
pub fn runtimes_list() -> Result<(), Error> {
    let c = ctx()?;
    let catalog = scan_runtimes(&c, None);
    if catalog.builds.is_empty() {
        println!("No compatibility layers installed.");
    }
    let size = |p: &std::path::Path| {
        format!(
            "{} MiB",
            zaun_core::runtime::gc::tree_size(&*c.fs, &*c.clock, p) >> 20
        )
    };
    for b in &catalog.builds {
        let managed = b.origin == zaun_core::model::RuntimeOrigin::Store;
        println!(
            "{:<44} {:<12} {:<9} {:<20} {:>9} {}",
            b.name,
            b.family.as_str(),
            b.required_slr.as_str(),
            b.origin_label,
            if managed {
                size(&b.path)
            } else {
                String::new()
            },
            b.path.display()
        );
    }
    for s in &catalog.slr {
        println!(
            "runtime {:<8} {:<24} {:<10} {:>9}",
            s.variant.as_str(),
            s.version.as_deref().unwrap_or("?"),
            if s.is_complete() {
                "installed"
            } else {
                "incomplete"
            },
            size(&s.path)
        );
    }
    for s in &catalog.foreign.slr {
        println!(
            "umu's own runtime {} {} at {} (not used by Zaun)",
            s.variant.as_str(),
            s.version.as_deref().unwrap_or("?"),
            s.path.display()
        );
    }
    for r in &catalog.rejected {
        println!("skipped {}: {}", r.path.display(), r.reason);
    }
    Ok(())
}

/// `zaun runtimes adopt [--variant V]`: copy umu's runtime into the store.
#[allow(clippy::print_stdout)]
pub fn runtimes_adopt(variant: Option<crate::cli::VariantArg>) -> Result<u8, Error> {
    let c = ctx()?;
    let catalog = scan_runtimes(&c, None);
    let want = variant.map(|v| match v {
        crate::cli::VariantArg::Steamrt3 => SlrVariant::Steamrt3,
        crate::cli::VariantArg::Steamrt4 => SlrVariant::Steamrt4,
    });
    let todo: Vec<_> = adoptable(&catalog.foreign.slr, &catalog.slr)
        .filter(|s| want.is_none_or(|w| w == s.variant))
        .cloned()
        .collect();
    if todo.is_empty() {
        let have: Vec<&str> = catalog
            .slr
            .iter()
            .filter(|s| s.is_complete())
            .map(|s| s.variant.as_str())
            .collect();
        println!(
            "Nothing to copy (Zaun has: {}; umu has: {}).",
            if have.is_empty() {
                "none".to_owned()
            } else {
                have.join(", ")
            },
            catalog
                .foreign
                .slr
                .iter()
                .map(|s| s.variant.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        return Ok(crate::cli::exit::OK);
    }
    c.fs.create_dir_all(&c.paths.data_dir, 0o700)?;
    let _lock = LibraryLock::try_acquire(&c.paths.library_lock, LockMode::Exclusive).map_err(
        |e| match e {
            StorageError::LockHeld => Error::new(ErrorCode::LibBusy),
            other => other.into(),
        },
    )?;
    let mut db = zaun_core::service::open_db(&*c.fs, &c.paths, &*c.clock)?;
    let store = RuntimeStore::new(&c.paths);
    for s in todo {
        println!(
            "copying {} {} from {} …",
            s.variant.as_str(),
            s.version.as_deref().unwrap_or("?"),
            s.path.display()
        );
        let now = c.clock.now();
        let r = adopt_slr(
            &*c.fs,
            &*c.clock,
            &store,
            &s.path,
            s.variant,
            now,
            &CancelToken::new(),
        )?;
        if let Some(install) = &r.install {
            db.put_slr_install(install)?;
            db.record_audit_result(install.variant, now, "clean")?;
        }
        println!(
            "  {} files ({} MiB, {} reflinked), manifest with {} entries -> {}",
            r.files,
            r.bytes >> 20,
            r.reflinked,
            r.manifest_entries,
            r.path.display()
        );
    }
    Ok(crate::cli::exit::OK)
}
