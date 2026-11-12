//! `easyplay runtimes install|remove|verify` (RT-009, RT-013, RT-008; CLI-003 partial) and
//! the `--runtime` argument of `add` and `edit`.
//!
//! These verbs work on the runtime store directly, like `runtimes adopt`: `install` plans a
//! Provision session and runs it through the same `launch::LocalTrustedSteps` as a launch
//! (store lock, queueing behind Shared-mode sessions, fast-path stop, post-conditions,
//! manifest rebuild); `verify` audits every installed variant and quarantines a modified
//! one; `remove` deletes a store build that no game uses and that is not the newest of its
//! family.

use std::path::PathBuf;
use std::sync::Arc;

use easyplay_core::clock::{Clock, SystemClock};
use easyplay_core::error::{Error, ErrorCode};
use easyplay_core::ids::{RuntimeName, SessionId};
use easyplay_core::jobs::CancelToken;
use easyplay_core::launch::{LocalTrustedSteps, TrustedProgress, TrustedSteps, plan_provision};
use easyplay_core::model::{Channel, RuntimeRef, SlrVariant};
use easyplay_core::runtime::provision::{check_postconditions, interpret};
use easyplay_core::runtime::{
    AuditAction, ProvisionNeed, Resolution, RuntimeService, RuntimeStore, gc,
};
use easyplay_core::session::RealLauncher;
use easyplay_core::storage::{GameRepo, RuntimeRepo};

use crate::cli::exit;
use crate::game_source::HostContext;
use crate::session_verbs::{open_library, real_proc, supervisor_exe};

/// Parse a `--runtime` value: `default`, `umu-proton`, `ge-proton`, an absolute path
/// (external build, used in place), or a build name (pinned).
pub fn parse_runtime_ref(s: &str) -> Result<RuntimeRef, Error> {
    let lower = s.to_ascii_lowercase();
    Ok(match lower.as_str() {
        "default" => RuntimeRef::Default,
        "umu-proton" | "umu" => RuntimeRef::Channel(Channel::UmuProton),
        "ge-proton" | "ge" => RuntimeRef::Channel(Channel::GeProton),
        _ if s.starts_with('/') => {
            let p = std::path::absolute(s)
                .map_err(|e| Error::new(ErrorCode::LibInvalidField).with_source(e))?;
            RuntimeRef::External(p)
        }
        _ => RuntimeRef::Pinned(RuntimeName::new(s).map_err(|e| {
            Error::new(ErrorCode::LibInvalidField)
                .with("runtime", s)
                .with("detail", format!("{e:?}"))
        })?),
    })
}

/// The trusted-session runner of the command line.
pub fn trusted_steps(hc: &HostContext) -> Result<LocalTrustedSteps, Error> {
    let proc = real_proc(hc);
    let launcher = RealLauncher::new(
        supervisor_exe()?,
        hc.paths.clone(),
        Arc::clone(&hc.fs),
        Arc::clone(&proc),
        Arc::clone(&hc.clock),
    );
    let prefs = crate::wiring::load_prefs(&*hc.fs, &hc.paths, &*hc.clock);
    Ok(LocalTrustedSteps {
        launcher: Arc::new(launcher),
        fs: Arc::clone(&hc.fs),
        clock: Arc::clone(&hc.clock),
        proc,
        paths: hc.paths.clone(),
        store: RuntimeStore::new(&hc.paths),
        prepare_online: prefs.protection.prepare_online,
    })
}

/// Print trusted-session progress on stderr (phase changes; bytes at most every 2 s).
pub fn progress_printer() -> impl FnMut(TrustedProgress) {
    let clock = SystemClock::new();
    let mut last_bytes_at = clock.mono();
    let mut last_phase = String::new();
    move |p| {
        #[allow(clippy::print_stderr)]
        match p {
            TrustedProgress::Waiting => {
                eprintln!("waiting for a running game or another download to finish …");
            }
            TrustedProgress::Phase(ph) => {
                let s = format!("{ph:?}");
                if s != last_phase {
                    eprintln!("  {s}");
                    last_phase = s;
                }
            }
            TrustedProgress::Bytes { bytes, rate } => {
                let now = clock.mono();
                if now.saturating_duration_since(last_bytes_at).as_secs() >= 2 {
                    last_bytes_at = now;
                    eprintln!("  {} MiB downloaded ({} KiB/s)", bytes >> 20, rate >> 10);
                }
            }
        }
    }
}

fn need_for(rt: &dyn RuntimeService, what: &str) -> Result<Option<ProvisionNeed>, Error> {
    let reference = parse_runtime_ref(what)?;
    if let RuntimeRef::Channel(c) = reference {
        return Ok(Some(ProvisionNeed::Codename(c)));
    }
    match rt.resolve(&reference, None) {
        Resolution::NeedsProvision(n) => Ok(Some(n)),
        Resolution::Ready(_) => Ok(None),
        Resolution::Unavailable(r) => Err(Error::new(r.code()).with("reason", format!("{r:?}"))),
    }
}

/// `easyplay runtimes install [WHAT]`.
#[allow(clippy::print_stdout, clippy::print_stderr)]
pub fn install(what: &str) -> Result<u8, Error> {
    let hc = HostContext::probe()?;
    let runtimes = hc.runtime_service();
    let Some(need) = need_for(&runtimes, what)? else {
        println!("{what} is already installed.");
        return Ok(exit::OK);
    };
    // The library must exist: the waiter reads the session rows.
    drop(open_library(&hc)?);
    let steps = trusted_steps(&hc)?;
    let install = install_id(&hc)?;
    let env = hc.host_env(&install);
    let sid = SessionId::generate(&*hc.clock);
    let planned = plan_provision(&need, &runtimes.catalog(), &hc.host, &env, sid)?;
    println!(
        "downloading {} (session {}) …",
        need.protonpath(),
        sid.short()
    );
    let t0 = hc.clock.mono();
    let end = steps.run(&planned, true, &mut progress_printer(), &CancelToken::new())?;
    interpret(&end)?;
    let change = steps.after_install()?;
    let catalog = runtimes.scan_now();
    let installed = check_postconditions(&catalog, &need)?;
    for p in &change.swept {
        println!("removed partial install {}", p.display());
    }
    for v in &change.manifests {
        println!("runtime {} verified and recorded", v.as_str());
    }
    println!(
        "done in {:.0} s; runtimes: {}",
        hc.clock.mono().saturating_duration_since(t0).as_secs_f64(),
        installed
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(exit::OK)
}

fn install_id(hc: &HostContext) -> Result<String, Error> {
    use easyplay_core::storage::{KvRepo, MetaKey};
    let mut db = open_library(hc)?;
    Ok(db
        .meta_get(MetaKey::InstallId)
        .ok()
        .flatten()
        .filter(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .unwrap_or_else(|| "0".repeat(32)))
}

/// `easyplay runtimes verify`: audit every installed variant (RUNTIME_MANAGEMENT §6.3
/// "Verify now"); a modified one is quarantined (§6.4).
#[allow(clippy::print_stdout, clippy::print_stderr)]
pub fn verify() -> Result<u8, Error> {
    let hc = HostContext::probe()?;
    let runtimes = hc.runtime_service();
    let catalog = runtimes.catalog();
    drop(open_library(&hc)?);
    let steps = trusted_steps(&hc)?;
    let mut code = exit::OK;
    let variants: Vec<SlrVariant> = catalog
        .slr
        .iter()
        .filter(|s| s.is_complete())
        .map(|s| s.variant)
        .collect();
    if variants.is_empty() {
        println!("No runtime installed.");
    }
    for v in variants {
        let t0 = hc.clock.mono();
        match steps.audit(v, &CancelToken::new())? {
            AuditAction::Clean => println!(
                "{}: unchanged ({:.1} s)",
                v.as_str(),
                hc.clock.mono().saturating_duration_since(t0).as_secs_f64()
            ),
            AuditAction::Missing => println!("{}: not installed", v.as_str()),
            a @ AuditAction::Quarantined { .. } => {
                code = exit::FAILED;
                if let AuditAction::Quarantined {
                    quarantine,
                    summary,
                    suspects,
                    ..
                } = &a
                {
                    println!("{}: MODIFIED ({})", v.as_str(), summary.line());
                    for p in summary
                        .changed
                        .iter()
                        .chain(&summary.added)
                        .chain(&summary.removed)
                    {
                        println!("    {p}");
                    }
                    println!("  moved to {}", quarantine.display());
                    if !suspects.is_empty() {
                        let names: Vec<&str> = suspects.iter().map(|s| s.name.as_str()).collect();
                        println!("  ran since the last check: {}", names.join(", "));
                    }
                    println!("  it is downloaded again before the next launch");
                }
                if let Some(n) = a.notice() {
                    eprintln!("easyplay: {n} [{}]", n.code.as_str());
                }
            }
        }
    }
    Ok(code)
}

/// `easyplay runtimes remove NAME`.
#[allow(clippy::print_stdout)]
pub fn remove(name: &str) -> Result<u8, Error> {
    let hc = HostContext::probe()?;
    let runtimes = hc.runtime_service();
    let catalog = runtimes.catalog();
    let name = RuntimeName::new(name).map_err(|e| {
        Error::new(ErrorCode::LibInvalidField)
            .with("runtime", name)
            .with("detail", format!("{e:?}"))
    })?;
    let mut db = open_library(&hc)?;
    let rows = db.load_library()?;
    let games: Vec<_> = rows
        .games
        .iter()
        .filter_map(|g| db.game(g.id).ok().flatten())
        .collect();
    let users = |n: &RuntimeName| -> Vec<String> {
        games
            .iter()
            .filter(|g| match &g.runtime {
                RuntimeRef::Pinned(p) => p == n,
                r => matches!(runtimes.resolve(r, Some(g)), Resolution::Ready(rt) if &rt.name == n),
            })
            .map(|g| g.name.clone())
            .collect()
    };
    let store = RuntimeStore::new(&hc.paths);
    let path: PathBuf = gc::remove_build(&*hc.fs, &store, &catalog, &users, &name, hc.clock.now())?;
    let _ = db.add_runtime_event(&easyplay_core::storage::RuntimeEvent {
        id: 0,
        at: hc.clock.now(),
        variant: None,
        kind: "removed".into(),
        summary_json: serde_json::json!({ "build": name.as_str() }).to_string(),
    });
    println!("removed {} ({})", name, path.display());
    Ok(exit::OK)
}
