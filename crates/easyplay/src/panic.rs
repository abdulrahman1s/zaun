//! Panic hook (ERROR_HANDLING §2, §4.11; LOGGING §6): log the panic with a backtrace, write a
//! redacted crash record to `<state>/crash/<timestamp>-<role>.json`, then run the default
//! hook. Never kills sessions: they belong to their supervisors, which are separate
//! processes and keep running.
//!
//! The hook is installed first thing in `main`; [`configure`] adds what the record needs
//! once logging is up (state directory, redaction, the log ring buffer). Until then a panic
//! is only logged. [`set_phase`] marks graphics initialisation for the renderer heuristic.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use easyplay_core::clock::{Clock, SystemClock};
use easyplay_core::diagnostics::crash::{self, CRASH_FORMAT, CrashPhase, CrashRecord};
use easyplay_core::fsx::RealFs;
use easyplay_core::logging::{ProcessRole, RecentLines, Redactor};

struct CrashCtx {
    dir: PathBuf,
    redactor: Redactor,
    role: ProcessRole,
    recent: Option<RecentLines>,
}

static CTX: OnceLock<CrashCtx> = OnceLock::new();
static CLOCK: OnceLock<SystemClock> = OnceLock::new();
static PHASE: AtomicU8 = AtomicU8::new(0);

fn clock() -> &'static SystemClock {
    CLOCK.get_or_init(SystemClock::new)
}

/// Record what the process is doing (read by the hook).
pub fn set_phase(phase: CrashPhase) {
    let v = match phase {
        CrashPhase::Startup => 0,
        CrashPhase::Graphics => 1,
        CrashPhase::Running => 2,
    };
    PHASE.store(v, Ordering::Relaxed);
}

fn phase() -> CrashPhase {
    match PHASE.load(Ordering::Relaxed) {
        1 => CrashPhase::Graphics,
        2 => CrashPhase::Running,
        _ => CrashPhase::Startup,
    }
}

/// Where and how crash records are written (once, after logging is initialised).
pub fn configure(dir: PathBuf, redactor: Redactor, role: ProcessRole, recent: Option<RecentLines>) {
    let _ = CTX.set(CrashCtx {
        dir,
        redactor,
        role,
        recent,
    });
}

/// The record of one panic (pure apart from the clock).
fn record(info: &std::panic::PanicHookInfo<'_>, backtrace: &str, ctx: &CrashCtx) -> CrashRecord {
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".to_owned());
    let c = clock();
    CrashRecord {
        format: CRASH_FORMAT,
        at: c.now(),
        version: crate::cli::VERSION.to_owned(),
        commit: env!("EASYPLAY_COMMIT").to_owned(),
        role: ctx.role.as_str().to_owned(),
        thread: std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .to_owned(),
        message,
        location: info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line())),
        backtrace: backtrace.to_owned(),
        uptime_ms: u64::try_from(c.mono().as_duration().as_millis()).unwrap_or(u64::MAX),
        phase: phase(),
        log_tail: ctx
            .recent
            .as_ref()
            .map(RecentLines::snapshot)
            .unwrap_or_default(),
    }
    .redacted(&ctx.redactor)
}

/// Install the hook.
pub fn install() {
    let _ = clock();
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        tracing::error!(
            code = "EP-INT-002",
            thread = thread.name().unwrap_or("unnamed"),
            "panic: {info}\n{backtrace}"
        );
        if let Some(ctx) = CTX.get() {
            let rec = record(info, &backtrace, ctx);
            match crash::write(&RealFs::new(), &ctx.dir, &rec) {
                Ok(p) => {
                    tracing::error!("crash record written to {}", ctx.redactor.redact_path(&p))
                }
                Err(e) => tracing::error!("crash record not written: {e}"),
            }
        }
        default(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ERR-005: a panic on a named thread (the AppService's in the GUI) leaves a redacted
    /// record; other threads, and therefore running sessions' supervisors (separate
    /// processes), are unaffected.
    #[test]
    fn panic_on_a_thread_leaves_a_record() {
        let dir = tempfile::tempdir().unwrap();
        let crash_dir = dir.path().join("crash");
        install();
        configure(
            crash_dir.clone(),
            Redactor::new(Some(std::path::Path::new("/home/sam")), Some("sam"), None),
            ProcessRole::Gui,
            None,
        );
        set_phase(CrashPhase::Running);
        let t = std::thread::Builder::new()
            .name("app-service".into())
            .spawn(|| panic!("forced panic reading /home/sam/x"))
            .unwrap();
        assert!(t.join().is_err());
        let recs = crash::list(&RealFs::new(), &crash_dir);
        assert_eq!(recs.len(), 1, "{recs:?}");
        let r = &recs[0].1;
        assert_eq!(r.thread, "app-service");
        assert_eq!(r.role, "gui");
        assert_eq!(r.phase, CrashPhase::Running);
        assert!(
            r.message.contains("forced panic reading ~/x"),
            "{}",
            r.message
        );
        assert!(r.location.as_deref().unwrap_or("").contains("panic.rs"));
        assert!(crash::pending(&RealFs::new(), &crash_dir).is_some());
    }
}
