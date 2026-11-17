//! Headless `launch`, `stop` and `status` (M6 walking skeleton; CLI-003 partial).
//!
//! Every verb opens the library (creating and migrating it if needed) and runs session
//! recovery first (PROCESS_LIFECYCLE §9), so a row left open by a killed supervisor is
//! closed by the next command. `<GAME>` resolution is the temporary one of
//! [`crate::game_source`]; `launch` writes the derived game into the library under the
//! library lock so its sessions have a game row.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use easyplay_core::clock::Clock;
use easyplay_core::err;
use easyplay_core::error::{Error, ErrorCode};
use easyplay_core::ids::{GameId, SessionId};
use easyplay_core::launch::{self, LaunchError, LaunchRequest, LaunchState, OrchestratorDeps};
use easyplay_core::model::{EndReason, Game, Session};
use easyplay_core::procfs::{ProcSource, RealProc, parse_ns_link};
use easyplay_core::session::{
    self, FlockLocks, RealLauncher, RecoveryOptions, SessionLauncher, StopMode, StopOutcome,
};
use easyplay_core::storage::lock::{LibraryLock, LockMode};
use easyplay_core::storage::{
    Db, ExecutableRepo, GamePatch, GameRepo, KvRepo, MetaKey, MigrateOptions, MigrationOutcome,
    SessionRepo, StorageError,
};

use crate::cli::exit;
use crate::game_source::{HostContext, derived_id, fail, load_game, print_issues};

/// Open (create, migrate) the library.
pub fn open_library(hc: &HostContext) -> Result<Db, Error> {
    hc.fs.create_dir_all(&hc.paths.data_dir, 0o700)?;
    let mut db = Db::open_rw(&hc.paths.db_file)?;
    let opts = MigrateOptions {
        backups_dir: Some(hc.paths.backups_dir.clone()),
        now: hc.clock.now(),
    };
    match db.migrate(&opts)? {
        MigrationOutcome::RefusedNewer { .. } => Err(Error::new(ErrorCode::DbNewerVersion)),
        _ => Ok(db),
    }
}

fn recover(hc: &HostContext, db: &mut Db, proc: &dyn ProcSource) -> Result<(), Error> {
    let recovered = session::recover(
        db,
        proc,
        &*hc.fs,
        &hc.paths,
        &*hc.clock,
        &RecoveryOptions::default(),
    )?;
    for r in recovered {
        tracing::info!(session = %r.id, reason = r.reason.as_str(), "recovered");
        #[allow(clippy::print_stderr)]
        {
            eprintln!(
                "note: closed session {} left open ({}, {})",
                r.id.short(),
                r.reason.as_str(),
                format_ms(r.duration_ms)
            );
        }
    }
    Ok(())
}

fn format_ms(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}h{:02}m{:02}s", s / 3600, (s / 60) % 60, s % 60)
    } else if s >= 60 {
        format!("{}m{:02}s", s / 60, s % 60)
    } else {
        format!("{}.{:01}s", s, (ms % 1000) / 100)
    }
}

fn is_path_arg(arg: &str) -> bool {
    arg.starts_with('/') || arg.starts_with('.') || arg.ends_with(".json") || arg.ends_with(".exe")
}

/// Resolve `<GAME>` against the library: game ID, ID prefix (≥ 6), or unique name.
fn find_in_library(db: &mut Db, arg: &str) -> Result<Option<Game>, Error> {
    if let Ok(id) = arg.parse::<GameId>() {
        return Ok(db.game(id)?);
    }
    let rows = db.load_library()?;
    if rows.games.is_empty() {
        return Ok(None);
    }
    match easyplay_core::library::resolve::resolve_game(
        rows.games.iter().map(|g| (g.id, g.name.as_str())),
        arg,
    ) {
        Ok(id) => Ok(db.game(id)?),
        Err(e)
            if e.context_value("detail")
                .is_some_and(|d| d.starts_with("no game")) =>
        {
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// TEMPORARY (until `add`, LIB-*): write the derived game into the library — insert it, or
/// refresh settings and the executable's fingerprint of the existing row.
fn upsert_temporary_game(hc: &HostContext, db: &mut Db, game: &Game) -> Result<Game, Error> {
    let _lock = LibraryLock::try_acquire(&hc.paths.library_lock, LockMode::Exclusive).map_err(
        |e| match e {
            StorageError::LockHeld => Error::new(ErrorCode::LibBusy),
            other => other.into(),
        },
    )?;
    match db.game(game.id)? {
        None => db.insert_game(game)?,
        Some(existing) => {
            let patch = GamePatch {
                policy: Some(game.policy.clone()),
                launch: Some(game.launch.clone()),
                runtime: Some(game.runtime.clone()),
                umu_id: Some(game.umu_id.clone()),
                ..GamePatch::default()
            };
            db.update_game(game.id, &patch, hc.clock.now())?;
            if let (Some(new), Some(old)) = (game.primary(), existing.primary())
                && new.fingerprint != old.fingerprint
            {
                let mut exe = old.clone();
                exe.fingerprint = new.fingerprint;
                db.update_executable(&exe)?;
            }
        }
    }
    db.game(game.id)?
        .ok_or_else(|| err!(LibGameNotFound, game = game.id))
}

fn install_id(db: &mut Db) -> String {
    db.meta_get(MetaKey::InstallId)
        .ok()
        .flatten()
        .filter(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .unwrap_or_else(|| "0".repeat(32))
}

pub(crate) fn supervisor_exe() -> Result<PathBuf, Error> {
    std::env::current_exe()
        .map_err(|e| Error::new(ErrorCode::SesSupervisorNotStarted).with_source(e))
}

pub(crate) fn real_proc(hc: &HostContext) -> Arc<dyn ProcSource> {
    Arc::new(RealProc::new(Arc::clone(&hc.fs)))
}

/// `easyplay launch <GAME> [--exe <LABEL|ID>] [--wait] [-- ARGS]`.
#[allow(clippy::print_stdout, clippy::print_stderr)]
pub fn launch(
    game_arg: &str,
    exe_arg: Option<&str>,
    wait: bool,
    extra: &[String],
) -> Result<u8, Error> {
    let hc = HostContext::probe()?;
    let mut db = open_library(&hc)?;
    let proc = real_proc(&hc);
    recover(&hc, &mut db, &*proc)?;

    let game = if is_path_arg(game_arg) {
        let derived = load_game(&*hc.fs, &hc.mounts, &hc.host, &*hc.clock, game_arg)?;
        upsert_temporary_game(&hc, &mut db, &derived)?
    } else {
        find_in_library(&mut db, game_arg)?
            .ok_or_else(|| fail(ErrorCode::LibInvalidField, format!("no game {game_arg:?}")))?
    };
    let exe = exe_arg
        .map(|a| crate::library_verbs::resolve_exe(&game, a))
        .transpose()?;
    if !hc.fs.metadata_follow(&hc.init).is_ok_and(|m| m.is_file()) {
        return Err(Error::new(ErrorCode::HostInitMissing).with("path", hc.init.display()));
    }
    let install = install_id(&mut db);
    let env = hc.host_env(&install);
    let runtimes = hc.runtime_service();
    let launcher = RealLauncher::new(
        supervisor_exe()?,
        hc.paths.clone(),
        Arc::clone(&hc.fs),
        Arc::clone(&proc),
        Arc::clone(&hc.clock),
    );
    let steps = crate::runtime_verbs::trusted_steps(&hc)?;
    let deps = OrchestratorDeps {
        host: &hc.host,
        env: &env,
        runtimes: &runtimes,
        launcher: &launcher,
        locks: &FlockLocks,
        fs: Arc::clone(&hc.fs),
        clock: &*hc.clock,
        steps: Some(&steps),
        cancel: None,
    };
    let session_id = SessionId::generate(&*hc.clock);
    let req = LaunchRequest {
        game: &game,
        exe,
        extra_args: extra,
        session_id,
        network_consent: false,
    };
    let mut last_kind = "";
    let mut last_mib = 0u64;
    let mut on_state = |s: &LaunchState| {
        tracing::debug!(state = s.kind().as_str(), "launch");
        let kind = s.kind().as_str();
        match s {
            LaunchState::Provisioning { phase, bytes } => {
                if kind != last_kind {
                    eprintln!(
                        "downloading the compatibility layer (once; the game stays offline) …"
                    );
                }
                if let Some(p) = phase {
                    tracing::info!(phase = ?p, "provisioning");
                }
                if bytes >> 26 != last_mib {
                    last_mib = bytes >> 26;
                    eprintln!("  {} MiB", bytes >> 20);
                }
            }
            LaunchState::Preparing { .. } if kind != last_kind => {
                eprintln!("preparing the private Windows storage …");
            }
            LaunchState::AuditingRuntime => eprintln!("checking the compatibility layer …"),
            _ => {}
        }
        last_kind = kind;
    };
    let outcome = match launch::launch(&req, &deps, &mut on_state) {
        Ok(o) => o,
        Err(LaunchError::Failed(e)) => {
            if let Some(src) = e.context_value("issue") {
                eprintln!("issue: {src}");
            }
            return Err(e);
        }
        Err(e) => return Err(e.into()),
    };
    print_issues(&outcome.report.warnings);
    for n in &outcome.notices {
        eprintln!("warning: {n} [{}]", n.code.as_str());
    }
    for t in &outcome.trusted {
        eprintln!(
            "note: {} session {} ({})",
            t.kind.as_str(),
            t.session.short(),
            if t.network == easyplay_core::model::NetworkAccess::Full {
                "with internet"
            } else {
                "offline"
            }
        );
    }
    if let Some((avail, wanted)) = outcome.report.low_space {
        eprintln!(
            "note: low disk space: {} MiB free, {} MiB recommended (EP-FS-002)",
            avail >> 20,
            wanted >> 20
        );
    }
    println!(
        "started {} (session {}, supervisor {})",
        game.name, outcome.handle.session_id, outcome.handle.supervisor.pid
    );
    println!(
        "log: {}",
        hc.paths.session_dir(outcome.handle.session_id).display()
    );
    if !wait {
        return Ok(exit::OK);
    }
    drop(db);
    wait_for_end(&hc, outcome.handle.session_id)
}

/// `--wait`: poll the row until it ends; 0 normal end, 1 failure to start, 2 crash.
#[allow(clippy::print_stdout)]
fn wait_for_end(hc: &HostContext, id: SessionId) -> Result<u8, Error> {
    let mut db = Db::open_ro(&hc.paths.db_file)?;
    let proc = real_proc(hc);
    loop {
        if let Some(s) = db.session(id)?
            && !s.is_open()
        {
            println!("{}", describe_end(&s));
            return Ok(match s.end_reason {
                Some(EndReason::FailedToStart { .. }) => 1,
                Some(EndReason::Crashed { .. }) => 2,
                _ => 0,
            });
        }
        // The supervisor may have died: recovery closes the row.
        if let Some(s) = db.session(id)?
            && let Some(sup) = session::stop::supervisor_identity(&s)
            && !proc.is_alive(sup).unwrap_or(true)
        {
            let mut rw = open_library(hc)?;
            recover(hc, &mut rw, &*proc)?;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn describe_end(s: &Session) -> String {
    let reason = s
        .end_reason
        .map(|r| match r.detail() {
            Some(d) => format!("{} {d}", r.as_str()),
            None => r.as_str().to_owned(),
        })
        .unwrap_or_else(|| "-".into());
    format!(
        "ended: {} after {} ({}{})",
        reason,
        format_ms(s.duration_ms.unwrap_or(s.active_ms)),
        if s.counted { "counted" } else { "not counted" },
        s.exit_status
            .map(|c| format!(", exit status {c}"))
            .unwrap_or_default()
    )
}

/// The open session `arg` refers to: a session ID, or a game (path, fixture, ID, name).
fn find_open_session(hc: &HostContext, db: &mut Db, arg: &str) -> Result<Option<Session>, Error> {
    let open = db.open_sessions()?;
    if let Ok(sid) = arg.parse::<SessionId>() {
        return Ok(open.into_iter().find(|s| s.id == sid));
    }
    let game_id = if is_path_arg(arg) {
        let derived = load_game(&*hc.fs, &hc.mounts, &hc.host, &*hc.clock, arg)
            .map(|g| g.id)
            .or_else(|_| {
                hc.fs
                    .canonicalize(Path::new(arg))
                    .map(|p| derived_id(&p))
                    .map_err(Error::from)
            })?;
        Some(derived)
    } else {
        find_in_library(db, arg)?.map(|g| g.id)
    };
    Ok(game_id.and_then(|g| open.into_iter().find(|s| s.game_id == Some(g))))
}

/// `easyplay stop <GAME> [--force]`.
#[allow(clippy::print_stdout)]
pub fn stop(game_arg: &str, force: bool) -> Result<u8, Error> {
    let hc = HostContext::probe()?;
    let mut db = open_library(&hc)?;
    let proc = real_proc(&hc);
    recover(&hc, &mut db, &*proc)?;
    let Some(s) = find_open_session(&hc, &mut db, game_arg)? else {
        println!("not running");
        return Ok(exit::OK);
    };
    let mode = if force {
        StopMode::Force
    } else {
        StopMode::Graceful
    };
    let launcher = RealLauncher::new(
        supervisor_exe()?,
        hc.paths.clone(),
        Arc::clone(&hc.fs),
        Arc::clone(&proc),
        Arc::clone(&hc.clock),
    );
    match launcher.request_stop(&s, mode)? {
        StopOutcome::Signalled => {
            println!(
                "{} stop requested for {} (session {})",
                mode.as_str(),
                s.game_name,
                s.id.short()
            );
        }
        StopOutcome::SupervisorGone => {
            recover(&hc, &mut db, &*proc)?;
            println!("the session's supervisor was gone; the session was closed");
        }
    }
    Ok(exit::OK)
}

/// `easyplay status [--json]`: active sessions (and their processes) and the latest ended
/// ones.
#[allow(clippy::print_stdout)]
pub fn status(json: bool) -> Result<u8, Error> {
    let hc = HostContext::probe()?;
    let mut db = open_library(&hc)?;
    let proc = real_proc(&hc);
    recover(&hc, &mut db, &*proc)?;
    let registry = session::Registry::rebuild(&mut db, &*proc)?;
    let recent: Vec<Session> = db
        .session_history(None, &[], 0, 5)?
        .into_iter()
        .filter(|s| !s.is_open())
        .collect();
    let own_ns = hc
        .fs
        .read_link(Path::new("/proc/self/ns/pid"))
        .ok()
        .and_then(|t| t.to_str().and_then(parse_ns_link));
    let now = hc.clock.now();
    if json {
        let active: Vec<serde_json::Value> = registry
            .active()
            .map(|a| {
                let procs = session::process_list(&*proc, &a.session, own_ns, None, false);
                serde_json::json!({
                    "session": a.session.id.to_string(),
                    "game": a.session.game_name,
                    "game_id": a.session.game_id.map(|g| g.to_string()),
                    "state": a.session.state.as_str(),
                    "started_at": a.session.started_at.as_millis(),
                    "active_ms": a.session.active_ms,
                    "supervisor_pid": a.supervisor.pid,
                    "sandbox_pid": a.session.sandbox_pid,
                    "processes": procs.iter().map(|p| serde_json::json!({
                        "pid": p.pid, "ppid": p.ppid, "comm": p.comm, "role": p.role.as_str(),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        let ended: Vec<serde_json::Value> = recent
            .iter()
            .map(|s| {
                serde_json::json!({
                    "session": s.id.to_string(),
                    "game": s.game_name,
                    "end_reason": s.end_reason.map(|r| r.as_str()),
                    "end_detail": s.end_reason.and_then(|r| r.detail()),
                    "duration_ms": s.duration_ms,
                    "counted": s.counted,
                    "exit_status": s.exit_status,
                })
            })
            .collect();
        let out = serde_json::json!({ "format": 1, "active": active, "recent": ended });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return Ok(exit::OK);
    }
    if registry.is_empty() {
        println!("no game is running");
    }
    for a in registry.active() {
        let s = &a.session;
        let elapsed = now
            .since(s.started_at)
            .map(|d| format_ms(u64::try_from(d.as_millis()).unwrap_or(0)))
            .unwrap_or_default();
        println!(
            "{}  {}  {}  {}  supervisor {}{}",
            s.id.short(),
            s.state.as_str(),
            elapsed,
            s.game_name,
            a.supervisor.pid,
            s.sandbox_pid
                .map(|p| format!(", sandbox {p}"))
                .unwrap_or_default()
        );
        for p in session::process_list(&*proc, s, own_ns, None, false) {
            println!(
                "    {:>7} {:>7}  {:<16} {}",
                p.pid,
                p.ppid,
                p.comm,
                p.role.as_str()
            );
        }
    }
    if !recent.is_empty() {
        println!("recent:");
        for s in &recent {
            println!(
                "  {}  {}  {}",
                s.id.short(),
                s.game_name,
                describe_end(s).trim_start_matches("ended: ")
            );
        }
    }
    Ok(exit::OK)
}
