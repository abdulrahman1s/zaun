//! `easyplay doctor [--json]` (HOST-009) and `easyplay runtimes list` (RT-004).

use std::path::PathBuf;
use std::sync::Arc;

use easyplay_core::Error;
use easyplay_core::clock::{Clock, SystemClock};
use easyplay_core::consts::{APP_VERSION, INIT_BIN_NAME, LIBEXEC_DIR};
use easyplay_core::error::ErrorCode;
use easyplay_core::fsx::{Fs, RealFs};
use easyplay_core::host::{
    CheckStatus, DOCTOR_FORMAT, DoctorContext, DoctorReport, HostProbe, LiveHostProbe, ProbeDeps,
    RealAccess, RealRunner, host_checks,
};
use easyplay_core::jobs::{JobPool, JobPoolConfig};
use easyplay_core::paths::{AppPaths, PathEnv};
use easyplay_core::runtime::{
    LocalRuntimeService, MemCatalogStore, RuntimeCatalog, RuntimeSettings, RuntimeStore,
    catalog_check,
};

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

/// `easyplay doctor [--json]`: probe afresh, print the ordered checks.
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
    });
    let facts = probe.refresh();
    let mut init_candidates = vec![PathBuf::from(LIBEXEC_DIR).join(INIT_BIN_NAME)];
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
    {
        init_candidates.push(dir.join(INIT_BIN_NAME));
    }
    let mut checks = host_checks(
        &facts,
        &DoctorContext {
            fs: &*c.fs,
            paths: &c.paths,
            uid: c.env.uid(),
            init_candidates,
        },
    );
    checks.push(catalog_check(&scan_runtimes(&c, facts.fs.reflink_ok)));
    let report = DoctorReport {
        format: DOCTOR_FORMAT,
        version: APP_VERSION.to_owned(),
        checks,
        facts: (*facts).clone(),
    };
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

/// `easyplay runtimes list`.
#[allow(clippy::print_stdout)]
pub fn runtimes_list() -> Result<(), Error> {
    let c = ctx()?;
    let catalog = scan_runtimes(&c, None);
    if catalog.builds.is_empty() {
        println!("No compatibility layers installed.");
    }
    for b in &catalog.builds {
        println!(
            "{:<44} {:<12} {:<9} {:<20} {}",
            b.name,
            b.family.as_str(),
            b.required_slr.as_str(),
            b.origin_label,
            b.path.display()
        );
    }
    for s in &catalog.slr {
        println!(
            "runtime {:<8} {:<24} {}",
            s.variant.as_str(),
            s.version.as_deref().unwrap_or("?"),
            if s.is_complete() {
                "installed"
            } else {
                "incomplete"
            }
        );
    }
    for s in &catalog.foreign.slr {
        println!(
            "umu's own runtime {} {} at {} (not used by EasyPlay)",
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
