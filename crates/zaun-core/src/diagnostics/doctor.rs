//! The Doctor report: host checks, then the compatibility-layer check D-15 with the
//! adoption hint for runtimes umu already installed outside the store.
//!
//! [`complete`] adds the checks that need more than the host facts: D-03 opens the library
//! read-only and runs `quick_check` ([`database_check`]); D-10 starts `zaun-init` inside
//! a minimal sandbox ([`init_check`]). Both are slow enough (milliseconds to a second) to run
//! on a job, never on the UI thread.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::consts::{APP_VERSION, DB_SCHEMA_VERSION};
use crate::error::ErrorCode;
use crate::host::runner::{CommandRunner, CommandSpec};
use crate::host::{
    Check, CheckStatus, DOCTOR_FORMAT, DoctorContext, DoctorReport, HostFacts, host_checks,
};
use crate::runtime::{RuntimeCatalog, adoptable, catalog_check};
use crate::storage::integrity::{IntegrityReport, check_file};
use crate::storage::{ReadDb, StorageError, Store};

/// D-03: the library opens, `quick_check` passes, the schema is current. A missing file is
/// fine (it is created on first start).
pub fn database_check(fs: &dyn crate::fsx::Fs, db_file: &Path) -> Check {
    let id = "D-03";
    if !fs.exists(db_file) {
        return Check::new(
            id,
            CheckStatus::Pass,
            "No library yet; it is created on first start",
        );
    }
    match check_file(db_file) {
        Ok(IntegrityReport::Ok) => {}
        Ok(IntegrityReport::Problems(p)) => {
            return Check::new(
                id,
                CheckStatus::Fail,
                format!(
                    "The library file is damaged ({})",
                    p.first()
                        .map(String::as_str)
                        .unwrap_or("quick_check failed")
                ),
            )
            .code(ErrorCode::DbCorrupt)
            .fix("Restart Zaun: it repairs the library or offers to restore a backup.");
        }
        Err(StorageError::Foreign { .. }) => {
            return Check::new(
                id,
                CheckStatus::Fail,
                "The library file belongs to another program",
            )
            .code(ErrorCode::DbCorrupt)
            .fix("Move the file away; Zaun creates a new library on the next start.");
        }
        Err(StorageError::Busy | StorageError::LockHeld) => {
            return Check::new(
                id,
                CheckStatus::Pass,
                "The library is in use and could not be checked now",
            );
        }
        Err(e) => {
            return Check::new(
                id,
                CheckStatus::Fail,
                format!("The library cannot be opened: {e}"),
            )
            .code(e.code());
        }
    }
    let version = ReadDb::open(db_file).and_then(|mut db| db.schema_version());
    match version {
        Ok(v) if v == DB_SCHEMA_VERSION => Check::new(
            id,
            CheckStatus::Pass,
            format!("The library opens and is intact (schema {v})"),
        ),
        Ok(v) if v > DB_SCHEMA_VERSION => Check::new(
            id,
            CheckStatus::Fail,
            format!("The library was written by a newer Zaun (schema {v}, this version reads {DB_SCHEMA_VERSION})"),
        )
        .code(ErrorCode::DbNewerVersion)
        .fix("Install the newer Zaun again, or restore a backup."),
        Ok(v) => Check::new(
            id,
            CheckStatus::Warn,
            format!("The library is intact but at schema {v}; it is updated to {DB_SCHEMA_VERSION} on the next start"),
        ),
        Err(e) => Check::new(id, CheckStatus::Fail, format!("The library cannot be read: {e}"))
            .code(e.code()),
    }
}

/// The argument vector of the D-10 self-test (after the bwrap path): the init shim runs
/// `true` as PID 2 of a fresh user and PID namespace over a read-only view of `/`.
pub fn init_test_args(init: &Path) -> Vec<OsString> {
    let mut v: Vec<OsString> = [
        "--unshare-user",
        "--unshare-pid",
        "--die-with-parent",
        "--ro-bind",
        "/",
        "/",
        "--dev",
        "/dev",
        "--proc",
        "/proc",
        "--",
    ]
    .map(OsString::from)
    .to_vec();
    v.push(init.as_os_str().to_owned());
    v.push("--".into());
    v.push("true".into());
    v
}

/// D-10: `zaun-init` is present and runs inside a minimal sandbox. Without a usable
/// bubblewrap or without a runner only presence is reported.
pub fn init_check(
    facts: &HostFacts,
    runner: Option<&dyn CommandRunner>,
    candidates: &[PathBuf],
    fs: &dyn crate::fsx::Fs,
) -> Check {
    let id = "D-10";
    let Some(init) = candidates
        .iter()
        .find(|p| crate::host::exec::is_executable(fs, p))
    else {
        return Check::new(id, CheckStatus::Fail, "zaun-init was not found")
            .code(ErrorCode::HostInitMissing)
            .fix("Reinstall Zaun.");
    };
    let (Some(bwrap), Some(runner)) = (facts.bwrap.as_ref().filter(|b| b.userns_ok), runner) else {
        return Check::new(
            id,
            CheckStatus::Pass,
            format!(
                "zaun-init at {} (not started: no working sandbox)",
                init.display()
            ),
        );
    };
    let spec = CommandSpec::new(&bwrap.path, &init_test_args(init))
        .env("PATH", facts.layout.path_env.clone());
    match runner.run(&spec) {
        Ok(out) if out.success() => Check::new(
            id,
            CheckStatus::Pass,
            format!("zaun-init at {} runs inside a sandbox", init.display()),
        ),
        Ok(out) => {
            let why = if out.timed_out {
                "it did not finish in time".to_owned()
            } else if out.status == 127 {
                "it could not start a program".to_owned()
            } else {
                let err = out.stderr_text();
                let last = err.lines().last().unwrap_or("").trim().to_owned();
                if last.is_empty() {
                    format!("exit status {}", out.status)
                } else {
                    last
                }
            };
            Check::new(
                id,
                CheckStatus::Fail,
                format!("zaun-init at {} does not run inside a sandbox: {why}", init.display()),
            )
            .code(ErrorCode::SbxInitNotRunnable)
            .fix("Reinstall Zaun; the helper must be a static program in a folder every sandbox can see.")
        }
        Err(e) => Check::new(
            id,
            CheckStatus::Fail,
            format!("The self-test could not run: {e}"),
        )
        .code(ErrorCode::SbxInitNotRunnable),
    }
}

/// What [`complete`] needs besides the report.
pub struct DoctorProbes<'a> {
    /// Runs the D-10 self-test (`None`: presence only).
    pub runner: Option<&'a dyn CommandRunner>,
    /// The library file (D-03).
    pub db_file: &'a Path,
}

/// Replace the facts-only D-03 and D-10 entries with the full checks.
pub fn complete(report: &mut DoctorReport, ctx: &DoctorContext<'_>, probes: &DoctorProbes<'_>) {
    let db = database_check(ctx.fs, probes.db_file);
    let init = init_check(&report.facts, probes.runner, &ctx.init_candidates, ctx.fs);
    for c in &mut report.checks {
        match c.id.as_str() {
            "D-03" => *c = db.clone(),
            "D-10" => *c = init.clone(),
            _ => {}
        }
    }
}

/// D-15, with the hint to adopt umu's runtime when that is what is missing.
pub fn runtime_check(catalog: &RuntimeCatalog) -> Check {
    let mut c = catalog_check(catalog);
    let can_adopt: Vec<&str> = adoptable(&catalog.foreign.slr, &catalog.slr)
        .map(|s| s.variant.as_str())
        .collect();
    let needed: Vec<_> = catalog
        .builds
        .iter()
        .map(|b| b.required_slr)
        .filter(|v| {
            !catalog
                .slr
                .iter()
                .any(|s| s.variant == *v && s.is_complete())
        })
        .collect();
    if !can_adopt.is_empty() && c.status != CheckStatus::Fail {
        let useful = needed.iter().any(|v| can_adopt.contains(&v.as_str()));
        if useful {
            c.status = CheckStatus::Warn;
        }
        c = c.fix(format!(
            "Copy umu's runtime into Zaun without downloading: zaun runtimes adopt ({})",
            can_adopt.join(", ")
        ));
    }
    c
}

/// The ordered checks of `zaun doctor` and the Doctor screen.
pub fn doctor_report(
    facts: &HostFacts,
    ctx: &DoctorContext<'_>,
    catalog: &RuntimeCatalog,
) -> DoctorReport {
    let mut checks = host_checks(facts, ctx);
    checks.push(runtime_check(catalog));
    DoctorReport {
        format: DOCTOR_FORMAT,
        version: APP_VERSION.to_owned(),
        checks,
        facts: facts.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsx::{Fs, RealFs};
    use crate::host::runner::{CommandOutput, RunError};
    use std::sync::Mutex;

    struct Scripted {
        out: CommandOutput,
        seen: Mutex<Vec<Vec<OsString>>>,
    }

    impl CommandRunner for Scripted {
        fn run(&self, cmd: &CommandSpec) -> Result<CommandOutput, RunError> {
            self.seen.lock().unwrap().push(cmd.argv());
            Ok(self.out.clone())
        }
    }

    fn facts_with_bwrap() -> HostFacts {
        let mut f = HostFacts::empty(crate::clock::Timestamp::EPOCH);
        f.bwrap = Some(crate::host::BwrapFacts {
            path: "/usr/bin/bwrap".into(),
            version: None,
            flags: Default::default(),
            setuid: false,
            userns_ok: true,
            nested_userns_ok: true,
            overlay_ok: false,
        });
        f
    }

    #[test]
    fn database_check_states() {
        let dir = tempfile::tempdir().unwrap();
        let fs = RealFs::new();
        let p = dir.path().join("library.db");
        assert_eq!(database_check(&fs, &p).status, CheckStatus::Pass);
        let mut db = crate::storage::Db::open_rw(&p).unwrap();
        db.migrate(&crate::storage::MigrateOptions::default())
            .unwrap();
        db.close().unwrap();
        let c = database_check(&fs, &p);
        assert_eq!(c.status, CheckStatus::Pass, "{c:?}");
        assert!(c.summary.contains("intact"));
        fs.atomic_write(&p, &[0x55; 8192], 0o600).unwrap();
        let c = database_check(&fs, &p);
        assert_eq!(c.status, CheckStatus::Fail, "{c:?}");
        assert_eq!(c.code, Some(ErrorCode::DbCorrupt));
        assert!(c.fix.is_some());
    }

    #[test]
    fn init_check_runs_the_shim_in_a_sandbox() {
        let dir = tempfile::tempdir().unwrap();
        let fs = RealFs::new();
        let init = dir.path().join("zaun-init");
        fs.atomic_write(&init, b"#!/bin/sh\n", 0o755).unwrap();
        let ok = Scripted {
            out: CommandOutput::default(),
            seen: Mutex::new(Vec::new()),
        };
        let facts = facts_with_bwrap();
        let c = init_check(&facts, Some(&ok), std::slice::from_ref(&init), &fs);
        assert_eq!(c.status, CheckStatus::Pass, "{c:?}");
        let argv = ok.seen.lock().unwrap()[0].clone();
        assert_eq!(argv[0], "/usr/bin/bwrap");
        assert!(argv.contains(&init.clone().into_os_string()));
        assert_eq!(argv.last().unwrap(), "true");

        let bad = Scripted {
            out: CommandOutput {
                status: 1,
                stderr: b"bwrap: execvp zaun-init: No such file or directory\n".to_vec(),
                ..CommandOutput::default()
            },
            seen: Mutex::new(Vec::new()),
        };
        let c = init_check(&facts, Some(&bad), std::slice::from_ref(&init), &fs);
        assert_eq!(c.status, CheckStatus::Fail);
        assert_eq!(c.code, Some(ErrorCode::SbxInitNotRunnable));
        assert!(c.summary.contains("No such file"), "{}", c.summary);

        let c = init_check(&facts, Some(&ok), &[dir.path().join("missing")], &fs);
        assert_eq!(c.code, Some(ErrorCode::HostInitMissing));
        // No sandbox: presence only, nothing run.
        let none = Scripted {
            out: CommandOutput::default(),
            seen: Mutex::new(Vec::new()),
        };
        let c = init_check(
            &HostFacts::empty(crate::clock::Timestamp::EPOCH),
            Some(&none),
            &[init],
            &fs,
        );
        assert_eq!(c.status, CheckStatus::Pass);
        assert!(none.seen.lock().unwrap().is_empty());
    }

    #[test]
    fn d09_accepts_the_nixpkgs_fhs_wrapper_and_warns_for_others() {
        use crate::host::facts::{UMU_VERIFIED_VERSION, UmuFacts, UmuWrapper};
        let fs = RealFs::new();
        let paths = crate::paths::AppPaths::resolve(
            &crate::paths::PathEnv::new(1000).with("HOME", "/home/sam"),
        )
        .unwrap();
        let ctx = DoctorContext {
            fs: &fs,
            paths: &paths,
            uid: 1000,
            init_candidates: Vec::new(),
        };
        let d09 = |fhs_env: bool| {
            let mut f = HostFacts::empty(crate::clock::Timestamp::EPOCH);
            f.umu = Some(UmuFacts {
                path: "/run/current-system/sw/bin/umu-run".into(),
                version: Some(UMU_VERIFIED_VERSION),
                closure: Vec::new(),
                wrapper: Some(UmuWrapper {
                    chain: vec!["/nix/store/x-umu-launcher-1.4.4-bwrap".into()],
                    sandboxed: true,
                    reaches_python: false,
                    fhs_env,
                }),
            });
            host_checks(&f, &ctx)
                .into_iter()
                .find(|c| c.id == "D-09")
                .unwrap()
        };
        let fhs = d09(true);
        assert_eq!(fhs.status, CheckStatus::Pass, "{fhs:?}");
        assert!(fhs.summary.contains("FHS"));
        let other = d09(false);
        assert_eq!(other.status, CheckStatus::Warn, "{other:?}");
    }
}
