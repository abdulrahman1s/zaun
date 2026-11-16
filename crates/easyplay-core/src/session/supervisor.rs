//! The supervisor (PROC-003, PROC-007, PROC-012; PROCESS_LIFECYCLE §3, §5–7): the detached
//! process that owns one session. It takes the prefix lock, inserts the one row of the
//! session, runs the host actions, spawns the sandbox, follows it through the status
//! descriptors, detects `Running` and `Lingering`, writes heartbeats, carries out stop
//! requests, and finally closes the row and applies the statistics in one transaction
//! (or writes `final.json` when the database is unavailable).
//!
//! Single-threaded event loop over `poll` (bwrap status, init status, sync descriptor, a
//! wake pipe fed by signal handlers and the log pump) with the log pump as helper thread.
//! The heartbeat is a timer of this loop (see [`super::heartbeat`]).

use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::clock::{Clock, MonoInstant, Timestamp};
use crate::consts::{HEARTBEAT_INTERVAL, SESSION_LOG_CAP_DEFAULT, STARTING_TIMEOUT};
use crate::error::{Error, ErrorCode};
use crate::fsx::{Fs, safe_remove_tree};
use crate::ids::{GameId, SessionId};
use crate::launch::LaunchPlan;
use crate::model::{
    EndReason, RuntimeIsolationMode, Session, SessionKind, SessionState, SlrVariant,
    SupervisorIdent,
};
use crate::procfs::{ProcEntry, ProcSource, parse_ns_link};
use crate::sandbox::{HostAction, sound_socket};
use crate::stats::CountRule;
use crate::stats::recorder::{SessionEnd, close_for};
use crate::storage::{
    CloseOutcome, Db, GameRepo, RuntimeRepo, SessionClose, SessionRepo, StorageResult,
};
use crate::umu::UmuPhase;

use super::classify::{EndClass, EndFacts, StopOrigin, classify_end, counted};
use super::heartbeat::Heartbeat;
use super::lock::{LockError, PrefixLock};
use super::log_pump::{LogPump, PumpConfig, PumpEvent, PumpReport};
use super::registry::SandboxView;
use super::spawn::{
    HostActionCtx, SandboxChild, SandboxIo, SandboxSpawner, SoundServer, run_host_actions,
    write_sound_config,
};
use super::status::{BwrapStatus, BwrapStatusParser, InitStatus, InitStatusParser};
use super::stop::{StopAction, StopMode, Stopper, take_stop_request};
use super::sys::{self, Sig};

/// Interval of the `Running` scan while `Starting`.
pub const RUNNING_SCAN: Duration = Duration::from_millis(500);
/// How long `wineserver` must exist before rule 2 of the `Running` detection applies.
pub const WINESERVER_SETTLE: Duration = Duration::from_secs(10);
/// Delay after `main-exit` before a non-empty sandbox is reported as `Lingering`.
pub const LINGER_CHECK: Duration = Duration::from_millis(300);
/// Longest poll timeout (a safety net for a missed hang-up).
const MAX_POLL: Duration = Duration::from_secs(5);
/// Name of the file the supervisor writes when it exits before running the sandbox.
pub const START_FAILED_FILE: &str = "start-failed";
/// Name of the final record written when the database is unavailable at the end.
pub const FINAL_FILE: &str = "final.json";

/// Windows processes that do not count as "the game" for rule 2 of the `Running` detection.
const WINE_SERVICES: [&str; 10] = [
    "services.exe",
    "winedevice.exe",
    "plugplay.exe",
    "explorer.exe",
    "rpcss.exe",
    "svchost.exe",
    "conhost.exe",
    "tabtip.exe",
    "start.exe",
    // Proton 10's accessibility helper, started with every prefix (observed in M6).
    "xalia.exe",
];

// ---- the store seam -----------------------------------------------------------------------

/// What the supervisor writes. Implemented for [`Db`]; tests wrap it to inject failures.
pub trait SessionStore: Send {
    /// Insert the row.
    fn insert(&mut self, session: &Session) -> StorageResult<()>;
    /// Change the state.
    fn set_state(&mut self, id: SessionId, state: SessionState) -> StorageResult<bool>;
    /// Heartbeat.
    fn heartbeat(&mut self, id: SessionId, active_ms: u64, at: Timestamp) -> StorageResult<bool>;
    /// Sandbox PID and namespace.
    fn set_sandbox(
        &mut self,
        id: SessionId,
        pid: Option<u32>,
        ns: Option<u64>,
    ) -> StorageResult<bool>;
    /// Append an event.
    fn event(&mut self, id: SessionId, at: Timestamp, kind: &str, data: &str) -> StorageResult<()>;
    /// `games.prefix_tainted_at` once.
    fn mark_tainted(&mut self, game: GameId, at: Timestamp) -> StorageResult<()>;
    /// The closing transaction: the guarded close with the statistics and, for a Shared
    /// untrusted session, `runtime_audit.pending`.
    fn close(
        &mut self,
        close: &SessionClose,
        audit: Option<SlrVariant>,
    ) -> StorageResult<CloseOutcome>;
}

impl SessionStore for Db {
    fn insert(&mut self, session: &Session) -> StorageResult<()> {
        self.insert_session(session)
    }
    fn set_state(&mut self, id: SessionId, state: SessionState) -> StorageResult<bool> {
        self.set_session_state(id, state)
    }
    fn heartbeat(&mut self, id: SessionId, active_ms: u64, at: Timestamp) -> StorageResult<bool> {
        SessionRepo::heartbeat(self, id, active_ms, at)
    }
    fn set_sandbox(
        &mut self,
        id: SessionId,
        pid: Option<u32>,
        ns: Option<u64>,
    ) -> StorageResult<bool> {
        self.set_session_sandbox(id, pid, ns)
    }
    fn event(&mut self, id: SessionId, at: Timestamp, kind: &str, data: &str) -> StorageResult<()> {
        self.add_session_event(id, at, kind, data).map(|_| ())
    }
    fn mark_tainted(&mut self, game: GameId, at: Timestamp) -> StorageResult<()> {
        if let Some(g) = self.game(game)?
            && g.prefix_tainted_at.is_some()
        {
            return Ok(());
        }
        self.set_prefix_tainted_at(game, Some(at)).map(|_| ())
    }
    fn close(
        &mut self,
        close: &SessionClose,
        audit: Option<SlrVariant>,
    ) -> StorageResult<CloseOutcome> {
        self.transaction(|tx| {
            let outcome = tx.close_session(close)?;
            if let (CloseOutcome::Closed { .. }, Some(v)) = (&outcome, audit) {
                tx.set_audit_pending(v)?;
            }
            Ok(outcome)
        })
    }
}

// ---- stop trigger ---------------------------------------------------------------------------

/// The signals the supervisor reacts to, as flags plus a wake-up pipe. In a real supervisor
/// the flags are set by signal handlers ([`StopTrigger::install_signal_handlers`]); tests and
/// embedders call [`StopTrigger::raise`].
#[derive(Clone, Debug)]
pub struct StopTrigger {
    term: Arc<AtomicBool>,
    usr1: Arc<AtomicBool>,
    wake_w: Arc<OwnedFd>,
    wake_r: Arc<OwnedFd>,
}

impl StopTrigger {
    /// A trigger with a fresh wake pipe.
    pub fn new() -> io::Result<Self> {
        let (r, w) = io::pipe()?;
        let r = OwnedFd::from(r);
        let w = OwnedFd::from(w);
        rustix::io::ioctl_fionbio(&r, true)?;
        rustix::io::ioctl_fionbio(&w, true)?;
        Ok(Self {
            term: Arc::new(AtomicBool::new(false)),
            usr1: Arc::new(AtomicBool::new(false)),
            wake_w: Arc::new(w),
            wake_r: Arc::new(r),
        })
    }

    /// Route `SIGTERM`, `SIGINT`, `SIGHUP` (graceful) and `SIGUSR1` (force) to this trigger,
    /// and ignore `SIGPIPE`. For the supervisor process only.
    pub fn install_signal_handlers(&self) -> io::Result<()> {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGPIPE, SIGTERM, SIGUSR1};
        for sig in [SIGTERM, SIGINT, SIGHUP] {
            signal_hook::flag::register(sig, Arc::clone(&self.term))?;
            signal_hook::low_level::pipe::register(sig, self.wake_w.try_clone()?)?;
        }
        signal_hook::flag::register(SIGUSR1, Arc::clone(&self.usr1))?;
        signal_hook::low_level::pipe::register(SIGUSR1, self.wake_w.try_clone()?)?;
        // Registering a flag replaces the default action; SIGPIPE then no longer kills.
        signal_hook::flag::register(SIGPIPE, Arc::new(AtomicBool::new(false)))?;
        Ok(())
    }

    /// Deliver `sig` as if it had been received (`Term` graceful, `Usr1` force).
    pub fn raise(&self, sig: Sig) {
        match sig {
            Sig::Term => self.term.store(true, Ordering::SeqCst),
            Sig::Usr1 | Sig::Kill => self.usr1.store(true, Ordering::SeqCst),
        }
        self.wake();
    }

    fn wake(&self) {
        let _ = rustix::io::write(&*self.wake_w, &[1]);
    }

    fn drain(&self) {
        let mut buf = [0u8; 64];
        while matches!(rustix::io::read(&*self.wake_r, &mut buf), Ok(n) if n > 0) {}
    }

    fn take(&self) -> (bool, bool) {
        (
            self.term.swap(false, Ordering::SeqCst),
            self.usr1.swap(false, Ordering::SeqCst),
        )
    }
}

// ---- configuration --------------------------------------------------------------------------

/// Tunables of one supervisor run.
#[derive(Clone, Debug)]
pub struct SupervisorConfig {
    /// The counted rule (`statistics.*` preferences).
    pub count_rule: CountRule,
    /// Byte cap of `game.log`.
    pub log_cap_bytes: u64,
    /// Heartbeat interval.
    pub heartbeat_interval: Duration,
    /// Attempts of the closing transaction before `final.json` is written.
    pub close_attempts: u32,
    /// Pause between attempts.
    pub close_retry_delay: Duration,
    /// This user's ID (owner checks of host actions).
    pub uid: u32,
    /// The runtime store (log classification) and the root of session variable
    /// directories (clean-up).
    pub runtime_store: Option<PathBuf>,
}

impl SupervisorConfig {
    /// Defaults of PROCESS_LIFECYCLE §3 and LOGIC_SPECS §7 for user `uid`.
    pub fn new(uid: u32) -> Self {
        Self {
            count_rule: CountRule::default(),
            log_cap_bytes: SESSION_LOG_CAP_DEFAULT,
            heartbeat_interval: HEARTBEAT_INTERVAL,
            close_attempts: 3,
            close_retry_delay: Duration::from_millis(333),
            uid,
            runtime_store: None,
        }
    }
}

/// Injected dependencies.
pub struct SupervisorDeps {
    /// The database.
    pub store: Box<dyn SessionStore>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Process information.
    pub proc: Arc<dyn ProcSource>,
    /// Filesystem.
    pub fs: Arc<dyn Fs>,
    /// Starts the sandbox.
    pub spawner: Box<dyn SandboxSpawner>,
    /// Stop requests.
    pub trigger: StopTrigger,
}

/// How a supervisor run ended.
#[derive(Debug)]
pub enum SupervisorOutcome {
    /// The prefix lock is held by another session; no row was inserted (exit 3).
    LockHeld,
    /// The supervisor could not start: no row was inserted (exit 1).
    NotStarted(Error),
    /// The session ran (or failed to start) and was closed.
    Ended {
        /// The session.
        session: SessionId,
        /// Why it ended.
        reason: EndReason,
        /// Whether it counted.
        counted: bool,
        /// The row was closed in the database (`false`: `final.json` was written, or the
        /// row was already closed by recovery).
        recorded: bool,
        /// The sandbox could not be spawned (exit 4).
        spawn_failed: bool,
    },
}

impl SupervisorOutcome {
    /// The process exit code of PROCESS_LIFECYCLE §3.
    pub fn exit_code(&self) -> u8 {
        match self {
            SupervisorOutcome::LockHeld => 3,
            SupervisorOutcome::NotStarted(_) => 1,
            SupervisorOutcome::Ended {
                spawn_failed: true, ..
            } => 4,
            SupervisorOutcome::Ended { .. } => 0,
        }
    }
}

/// The record written to `<session log dir>/final.json` when the closing transaction fails
/// (PROCESS_LIFECYCLE §3); imported by recovery.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalRecord {
    /// Format version (1).
    pub format: u32,
    /// Session.
    pub session: SessionId,
    /// End (wall clock).
    pub ended_at: Timestamp,
    /// Monotonic duration.
    pub duration_ms: u64,
    /// Why it ended.
    pub end_reason: EndReason,
    /// Raw exit status.
    pub exit_status: Option<i32>,
    /// Whether it counts.
    pub counted: bool,
    /// Variant to mark for audit.
    pub audit: Option<SlrVariant>,
}

// ---- helpers --------------------------------------------------------------------------------

fn tz_offset_min(at: Timestamp) -> i16 {
    let Ok(ts) = jiff::Timestamp::from_millisecond(at.as_millis()) else {
        return 0;
    };
    let off = jiff::tz::TimeZone::system().to_offset(ts);
    i16::try_from(off.seconds() / 60).unwrap_or(0)
}

fn own_pid_ns(fs: &dyn Fs) -> Option<u64> {
    fs.read_link(Path::new("/proc/self/ns/pid"))
        .ok()
        .and_then(|t| t.to_str().and_then(parse_ns_link))
}

fn write_start_failed(fs: &dyn Fs, plan: &LaunchPlan, code: ErrorCode) {
    let _ = fs.create_dir_all(&plan.paths.runtime_dir, 0o700);
    let _ = fs.atomic_write(
        &plan.paths.runtime_dir.join(START_FAILED_FILE),
        format!("{}\n", code.as_str()).as_bytes(),
        0o600,
    );
}

/// The target executable's `comm` (first 15 bytes of the file name, lower case) for rule 1
/// of the `Running` detection: the argument after `umu-run` in the sandbox command.
pub fn target_comm(plan: &LaunchPlan) -> Option<String> {
    let cmd = &plan.sandbox.command;
    let i = cmd.iter().position(|a| a == "--")?;
    let exe = cmd.get(i + 2)?;
    let name = Path::new(exe).file_name()?.to_string_lossy().to_lowercase();
    let mut end = name.len().min(15);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    Some(name[..end].to_owned())
}

/// Rules of PROCESS_LIFECYCLE §5 over one process listing.
#[derive(Debug, Default)]
struct RunningDetector {
    target: Option<String>,
    wineserver_since: Option<MonoInstant>,
    launching_at: Option<MonoInstant>,
}

impl RunningDetector {
    fn check(&mut self, members: &[ProcEntry], now: MonoInstant) -> Option<&'static str> {
        let comm = |e: &ProcEntry| e.comm.to_lowercase();
        if let Some(t) = &self.target
            && members.iter().any(|e| &comm(e) == t)
        {
            return Some("target");
        }
        if members.iter().any(|e| comm(e) == "wineserver") {
            let since = *self.wineserver_since.get_or_insert(now);
            let game_exe = members.iter().any(|e| {
                let c = comm(e);
                c.ends_with(".exe") && !WINE_SERVICES.contains(&c.as_str())
            });
            if game_exe && now.saturating_duration_since(since) >= WINESERVER_SETTLE {
                return Some("wine");
            }
        } else {
            self.wineserver_since = None;
        }
        if let Some(l) = self.launching_at
            && now.saturating_duration_since(l) >= STARTING_TIMEOUT
        {
            return Some("timeout");
        }
        None
    }
}

/// A non-blocking read end and its parser state.
struct Stream {
    file: File,
    open: bool,
}

impl Stream {
    fn new(fd: OwnedFd) -> io::Result<Self> {
        rustix::io::ioctl_fionbio(&fd, true)?;
        Ok(Self {
            file: File::from(fd),
            open: true,
        })
    }

    /// Read what is available; `None` at end of file.
    fn read_available(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match self.file.read(&mut buf) {
                Ok(0) => {
                    self.open = false;
                    break;
                }
                Ok(n) => {
                    out.extend_from_slice(&buf[..n]);
                    if out.len() > 1 << 20 {
                        break;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        out
    }
}

fn pipe_pair() -> io::Result<(OwnedFd, OwnedFd)> {
    let (r, w) = io::pipe()?;
    Ok((OwnedFd::from(r), OwnedFd::from(w)))
}

// ---- the run --------------------------------------------------------------------------------

struct Run<'a> {
    plan: &'a LaunchPlan,
    cfg: &'a SupervisorConfig,
    deps: SupervisorDeps,
    id: SessionId,
    state: SessionState,
    t0: MonoInstant,
    view: Option<SandboxView>,
    bwrap_exit: Option<i32>,
    main_exit: Option<i32>,
    exec_failed: bool,
    running_reached: bool,
    stopper: Stopper,
    detector: RunningDetector,
    linger_at: Option<MonoInstant>,
    next_scan: MonoInstant,
    own_ns: Option<u64>,
    child: Option<Box<dyn SandboxChild>>,
    /// The session's sound server (ADR-0026), stopped at cleanup.
    sound: Option<Box<dyn SandboxChild>>,
    locks: Vec<PrefixLock>,
    row: Session,
}

impl Run<'_> {
    fn now(&self) -> MonoInstant {
        self.deps.clock.mono()
    }

    fn event(&mut self, kind: &str, data: serde_json::Value) {
        let at = self.deps.clock.now();
        if let Err(e) = self.deps.store.event(self.id, at, kind, &data.to_string()) {
            tracing::warn!("session event not recorded: {e}");
        }
    }

    fn set_state(&mut self, next: SessionState) {
        if self.state == next || !self.state.can_become(next) {
            return;
        }
        self.state = next;
        if let Err(e) = self.deps.store.set_state(self.id, next) {
            tracing::warn!("state {} not recorded: {e}", next.as_str());
        }
        self.event("state", serde_json::json!({ "state": next.as_str() }));
        tracing::info!(session = %self.id, state = next.as_str(), "session state");
    }

    fn on_bwrap(&mut self, st: BwrapStatus, pump: &LogPump) {
        match st {
            BwrapStatus::ChildPid { pid, pid_ns } => {
                let own_namespace = pid_ns.is_some() && pid_ns != self.own_ns;
                self.view = Some(SandboxView {
                    child_pid: pid,
                    pid_ns,
                    own_namespace,
                });
                pump.set_sandbox_started();
                if let Err(e) = self.deps.store.set_sandbox(self.id, Some(pid), pid_ns) {
                    tracing::warn!("sandbox PID not recorded: {e}");
                }
                tracing::info!(child_pid = pid, own_namespace, "sandbox started");
            }
            BwrapStatus::ExitCode(c) => self.bwrap_exit = Some(c),
        }
    }

    fn on_init(&mut self, st: InitStatus) {
        match st {
            InitStatus::MainExit(code) => {
                self.main_exit = Some(code);
                self.linger_at = Some(self.now().saturating_add(LINGER_CHECK));
            }
            InitStatus::ExecFailed(errno) => {
                self.exec_failed = true;
                tracing::warn!(errno, "the init shim could not execute umu-run");
            }
            InitStatus::StopRequested(sig) => tracing::info!(sig, "init shim relays a stop"),
            InitStatus::Started(_) | InitStatus::Empty => {}
        }
    }

    fn on_phase(&mut self, phase: UmuPhase) {
        if matches!(phase, UmuPhase::Launching { .. }) && self.detector.launching_at.is_none() {
            self.detector.launching_at = Some(self.now());
        }
        let data = serde_json::to_value(&phase).unwrap_or_default();
        self.event("phase", data);
    }

    fn stop_request(&mut self, mode: StopMode, origin: StopOrigin) {
        let now = self.now();
        let actions = self.stopper.request(mode, origin, now);
        if !actions.is_empty() {
            self.event(
                "stop_requested",
                serde_json::json!({
                    "mode": mode.as_str(),
                    "origin": match origin { StopOrigin::User => "user", StopOrigin::System => "system" },
                }),
            );
        }
        self.act(&actions);
    }

    /// Returns `true` when the supervisor should stop waiting for the sandbox.
    fn act(&mut self, actions: &[StopAction]) -> bool {
        let proc = Arc::clone(&self.deps.proc);
        let mut give_up = false;
        for a in actions {
            match a {
                StopAction::MarkStopping => self.set_state(SessionState::Stopping),
                StopAction::TermInit => {
                    if let Some(v) = self.view {
                        let _ = sys::signal_pid(v.init_pid(&*proc), Sig::Term);
                    }
                }
                StopAction::TermAll => {
                    if let Some(v) = self.view {
                        let _ = sys::signal_pid(v.init_pid(&*proc), Sig::Term);
                        for m in v.members(&*proc) {
                            let _ = sys::signal_pid(m.pid, Sig::Term);
                        }
                    }
                }
                StopAction::KillAll => {
                    if let Some(v) = self.view {
                        // List first: once PID 1 is gone, orphans leave the tree (without a
                        // namespace of its own, nothing else would find them).
                        let members = v.members(&*proc);
                        let _ = sys::signal_pid(v.child_pid, Sig::Kill);
                        for m in members {
                            let _ = sys::signal_pid(m.pid, Sig::Kill);
                        }
                    }
                    if let Some(c) = self.child.as_mut() {
                        let _ = c.kill_monitor();
                    }
                }
                StopAction::GiveUp => {
                    tracing::warn!(
                        code = ErrorCode::SesForcedStop.as_str(),
                        "the sandbox did not disappear after the force stop"
                    );
                    give_up = true;
                }
            }
        }
        give_up
    }
}

/// Supervise the session of `plan` (PROCESS_LIFECYCLE §3).
pub fn run(
    plan: &LaunchPlan,
    cfg: &SupervisorConfig,
    mut deps: SupervisorDeps,
) -> SupervisorOutcome {
    let fs = Arc::clone(&deps.fs);
    let id = plan.session.id;
    let Some(me) = sys::own_identity(&*deps.proc) else {
        write_start_failed(&*fs, plan, ErrorCode::IntTaskFailed);
        return SupervisorOutcome::NotStarted(
            Error::new(ErrorCode::IntTaskFailed).with("detail", "own process not readable"),
        );
    };
    let boot_id = deps.proc.boot_id().unwrap_or_default();

    let mut locks = Vec::new();
    if let Some(p) = &plan.paths.prefix_lock {
        match PrefixLock::try_acquire(p) {
            Ok(l) => locks.push(l),
            Err(LockError::Held) => {
                tracing::info!(
                    code = ErrorCode::SesAlreadyRunning.as_str(),
                    "prefix lock held"
                );
                write_start_failed(&*fs, plan, ErrorCode::SesAlreadyRunning);
                return SupervisorOutcome::LockHeld;
            }
            Err(e) => {
                let e: Error = e.into();
                write_start_failed(&*fs, plan, e.code);
                return SupervisorOutcome::NotStarted(e);
            }
        }
    }

    let started_at = deps.clock.now();
    let row = Session {
        id,
        game_id: plan.session.game_id,
        game_name: plan.session.game_name.clone(),
        executable_id: plan.session.executable_id,
        kind: plan.session.kind,
        state: SessionState::Starting,
        started_at,
        ended_at: None,
        duration_ms: None,
        active_ms: 0,
        last_heartbeat_at: Some(started_at),
        end_reason: None,
        exit_status: None,
        counted: false,
        supervisor: Some(SupervisorIdent {
            pid: me.pid,
            start_ticks: me.start_time_ticks,
            boot_id,
        }),
        sandbox_pid: None,
        pid_ns: None,
        context: plan.context.clone(),
        tz_offset_min: tz_offset_min(started_at),
    };
    if let Err(e) = deps.store.insert(&row) {
        let e: Error = e.into();
        write_start_failed(&*fs, plan, e.code);
        return SupervisorOutcome::NotStarted(e);
    }
    let _ = fs.create_dir_all(&plan.paths.log_dir, 0o700);
    let grace = Duration::from_secs(u64::from(plan.session.stop_grace_secs.max(1)));
    let now = deps.clock.mono();
    let own_ns = own_pid_ns(&*fs);
    let mut r = Run {
        plan,
        cfg,
        deps,
        id,
        state: SessionState::Starting,
        t0: now,
        view: None,
        bwrap_exit: None,
        main_exit: None,
        exec_failed: false,
        running_reached: false,
        stopper: Stopper::new(grace),
        detector: RunningDetector {
            target: target_comm(plan),
            ..RunningDetector::default()
        },
        linger_at: None,
        next_scan: now,
        own_ns,
        child: None,
        sound: None,
        locks,
        row,
    };
    r.event("state", serde_json::json!({ "state": "starting" }));
    tracing::info!(session = %id, kind = plan.session.kind.as_str(), "session row inserted");
    r.supervise()
}

impl Run<'_> {
    fn spawn_failure(&mut self, err: &Error) -> SupervisorOutcome {
        tracing::warn!(code = err.code.as_str(), "sandbox not started: {err}");
        let facts = EndFacts {
            kind: self.plan.session.kind,
            spawn_failed: Some(err.code),
            child_pid_seen: false,
            stop: None,
            forced: false,
            log_failure: None,
            exit_status: None,
            running_reached: false,
            duration: Duration::ZERO,
        };
        let class = classify_end(&facts);
        self.finish(&facts, class, true)
    }

    fn supervise(mut self) -> SupervisorOutcome {
        let plan = self.plan;
        // Host actions (BUBBLEWRAP_SANDBOX §8).
        let host_result = {
            let store = &mut self.deps.store;
            let clock = Arc::clone(&self.deps.clock);
            let mut mark = |g: GameId| -> Result<(), Error> {
                store.mark_tainted(g, clock.now()).map_err(Error::from)
            };
            let mut ctx = HostActionCtx {
                fs: &*self.deps.fs,
                uid: self.cfg.uid,
                locks: &mut self.locks,
                mark_tainted: &mut mark,
            };
            run_host_actions(&plan.host_actions, &mut ctx)
        };
        if let Err(e) = host_result {
            return self.spawn_failure(&e);
        }
        for a in &plan.host_actions {
            if let HostAction::StartSoundServer {
                program,
                dir,
                remote,
                log,
            } = a
            {
                let config = match write_sound_config(&*self.deps.fs, self.cfg.uid, dir) {
                    Ok(c) => c,
                    Err(e) => return self.spawn_failure(&e),
                };
                let socket = sound_socket(dir);
                let server = SoundServer {
                    program,
                    config: &config,
                    socket: &socket,
                    remote,
                    log,
                };
                match self.deps.spawner.start_sound_server(server) {
                    Ok(s) => self.sound = Some(s),
                    Err(e) => return self.spawn_failure(&e),
                }
            }
        }

        let pipes =
            (|| -> io::Result<_> { Ok((pipe_pair()?, pipe_pair()?, pipe_pair()?, pipe_pair()?)) })(
            );
        let Ok(((log_r, log_w), (st_r, st_w), (in_r, in_w), (sy_r, sy_w))) = pipes else {
            return self
                .spawn_failure(&Error::new(ErrorCode::SbxSetupFailed).with("step", "pipes"));
        };
        let (ev_tx, ev_rx) = mpsc::channel();
        let trigger = self.deps.trigger.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || trigger.wake());
        let pump_cfg = PumpConfig {
            cap_bytes: self.cfg.log_cap_bytes,
            runtime_store: self.cfg.runtime_store.clone(),
            ..PumpConfig::in_dir(&plan.paths.log_dir)
        };
        self.t0 = self.now();
        let pump = match LogPump::start(
            log_r,
            pump_cfg,
            Arc::clone(&self.deps.clock),
            self.t0,
            ev_tx,
            Some(wake),
        ) {
            Ok(p) => p,
            Err(e) => {
                return self.spawn_failure(&Error::new(ErrorCode::SbxSetupFailed).with_source(e));
            }
        };
        let io_set = SandboxIo {
            log: log_w,
            status: st_w,
            init_status: in_w,
            sync: sy_w,
        };
        match self.deps.spawner.spawn(&plan.bwrap, &plan.sandbox, io_set) {
            Ok(c) => {
                tracing::info!(monitor = c.monitor_pid(), "bwrap started");
                self.child = Some(c);
            }
            Err(e) => {
                let _ = pump.finish(Duration::from_secs(1));
                return self.spawn_failure(&e);
            }
        }
        let mut heartbeat = Heartbeat::with_interval(self.id, self.t0, self.cfg.heartbeat_interval);
        let (Ok(mut status), Ok(mut init)) = (Stream::new(st_r), Stream::new(in_r)) else {
            return self
                .spawn_failure(&Error::new(ErrorCode::SbxSetupFailed).with("step", "pipes"));
        };
        let mut bparser = BwrapStatusParser::new();
        let mut iparser = InitStatusParser::new();
        let mut sync_gone = false;
        let mut monitor_status: Option<i32> = None;
        let mut monitor_exit_at: Option<MonoInstant> = None;
        let may_run = !matches!(
            plan.session.kind,
            SessionKind::Prepare | SessionKind::Provision
        );
        let stop_file = plan.paths.runtime_dir.join("stop-request");
        let wake_fd = self.deps.trigger.wake_r.as_raw_fd();

        loop {
            let now = self.now();
            let mut next = heartbeat.next_due();
            if self.state == SessionState::Starting && may_run {
                next = next.min(self.next_scan);
            }
            if let Some(d) = self.stopper.deadline() {
                next = next.min(d);
            }
            if let Some(l) = self.linger_at {
                next = next.min(l);
            }
            if let Some(m) = monitor_exit_at {
                next = next.min(m.saturating_add(Duration::from_secs(2)));
            }
            let wait = next.saturating_duration_since(now).min(MAX_POLL);
            let timeout_ms = i32::try_from(wait.as_millis()).unwrap_or(i32::MAX).max(1);

            let mut fds = vec![wake_fd];
            if status.open {
                fds.push(status.file.as_raw_fd());
            }
            if init.open {
                fds.push(init.file.as_raw_fd());
            }
            if !sync_gone {
                fds.push(sy_r.as_raw_fd());
            }
            let ready = match sys::poll_fds(&fds, timeout_ms) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("poll failed: {e}");
                    std::thread::sleep(Duration::from_millis(100));
                    vec![(false, false); fds.len()]
                }
            };
            for (fd, (readable, hup)) in fds.iter().zip(ready) {
                if !(readable || hup) {
                    continue;
                }
                if *fd == wake_fd {
                    self.deps.trigger.drain();
                } else if status.open && *fd == status.file.as_raw_fd() {
                    let data = status.read_available();
                    for st in bparser.push(&data) {
                        self.on_bwrap(st, &pump);
                    }
                } else if init.open && *fd == init.file.as_raw_fd() {
                    let data = init.read_available();
                    for st in iparser.push(&data) {
                        self.on_init(st);
                    }
                } else if *fd == sy_r.as_raw_fd() {
                    let mut b = [0u8; 16];
                    match rustix::io::read(sy_r.as_fd(), &mut b) {
                        Ok(0) | Err(_) => sync_gone = true,
                        Ok(_) => {}
                    }
                }
            }

            let (term, usr1) = self.deps.trigger.take();
            if usr1 {
                let _ = take_stop_request(&*self.deps.fs, &stop_file);
                self.stop_request(StopMode::Force, StopOrigin::User);
            }
            if term {
                match take_stop_request(&*self.deps.fs, &stop_file) {
                    Some(mode) => self.stop_request(mode, StopOrigin::User),
                    None => self.stop_request(StopMode::Graceful, StopOrigin::System),
                }
            }
            while let Ok(PumpEvent::Phase { phase, .. }) = ev_rx.try_recv() {
                self.on_phase(phase);
            }

            let now = self.now();
            if self.state == SessionState::Starting && may_run && now >= self.next_scan {
                self.next_scan = now.saturating_add(RUNNING_SCAN);
                if let Some(v) = self.view {
                    let members = v.members(&*self.deps.proc);
                    if let Some(rule) = self.detector.check(&members, now) {
                        self.running_reached = true;
                        self.set_state(SessionState::Running);
                        self.event("note", serde_json::json!({ "running_by": rule }));
                        // The main program may have exited while the session was still
                        // `Starting` (its check for `Lingering` found the wrong state):
                        // look at what it left behind now.
                        if self.main_exit.is_some() {
                            self.linger_at = Some(now);
                        }
                    }
                }
            }
            heartbeat.tick(&mut *self.deps.store, &*self.deps.clock);
            let actions = self.stopper.tick(now);
            if self.act(&actions) {
                break;
            }
            if let Some(l) = self.linger_at
                && now >= l
            {
                self.linger_at = None;
                if !sync_gone
                    && self.state == SessionState::Running
                    && let Some(v) = self.view
                {
                    let init_pid = v.init_pid(&*self.deps.proc);
                    let others = v
                        .members(&*self.deps.proc)
                        .iter()
                        .any(|m| m.pid != v.child_pid && m.pid != init_pid);
                    if others {
                        self.set_state(SessionState::Lingering);
                    }
                }
            }
            if monitor_status.is_none()
                && let Some(c) = self.child.as_mut()
                && let Ok(Some(code)) = c.try_wait()
            {
                monitor_status = Some(code);
                monitor_exit_at = Some(now);
            }
            if sync_gone {
                break;
            }
            if let Some(m) = monitor_exit_at
                && now.saturating_duration_since(m) >= Duration::from_secs(2)
            {
                tracing::warn!("bwrap exited but the sync descriptor stayed open");
                break;
            }
        }
        let t1 = self.now();

        // Reap the monitor (it exits right after the sandbox).
        if monitor_status.is_none()
            && let Some(c) = self.child.as_mut()
        {
            for i in 0..200 {
                if let Ok(Some(code)) = c.try_wait() {
                    monitor_status = Some(code);
                    break;
                }
                if i == 100 {
                    let _ = c.kill_monitor();
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        // Late status lines.
        if status.open {
            let data = status.read_available();
            for st in bparser.push(&data) {
                self.on_bwrap(st, &pump);
            }
        }
        if init.open {
            let data = init.read_available();
            for st in iparser.push(&data) {
                self.on_init(st);
            }
        }
        let report: PumpReport = pump.finish(Duration::from_secs(2)).unwrap_or_else(|| {
            tracing::warn!("sandbox output was still open at the end; log pump abandoned");
            PumpReport::default()
        });
        while let Ok(PumpEvent::Phase { phase, .. }) = ev_rx.try_recv() {
            self.on_phase(phase);
        }

        let exit_status = self.main_exit.or(self.bwrap_exit).or(monitor_status);
        let mut log_failure = report.failure.as_ref().map(|f| f.code);
        if self.exec_failed && log_failure.is_none() {
            log_failure = Some(ErrorCode::HostUmuNotFound);
        }
        let facts = EndFacts {
            kind: plan.session.kind,
            spawn_failed: None,
            child_pid_seen: self.view.is_some(),
            stop: self.stopper.origin(),
            forced: self.stopper.forced(),
            log_failure,
            exit_status,
            running_reached: self.running_reached,
            duration: t1.saturating_duration_since(self.t0),
        };
        let class = classify_end(&facts);
        self.finish(&facts, class, false)
    }

    fn finish(
        &mut self,
        facts: &EndFacts,
        class: EndClass,
        spawn_failed: bool,
    ) -> SupervisorOutcome {
        let counted = counted(facts, &class, self.cfg.count_rule);
        let duration_ms = u64::try_from(facts.duration.as_millis()).unwrap_or(u64::MAX);
        let end = SessionEnd {
            id: self.id,
            ended_at: self.deps.clock.now(),
            duration_ms,
            end_reason: class.reason,
            exit_status: facts.exit_status,
        };
        let mut close = close_for(&self.row, &end, self.cfg.count_rule);
        close.counted = counted;
        if !counted {
            close.pieces.clear();
        }
        let audit = self
            .plan
            .audit_after
            .filter(|_| self.plan.session.kind.is_untrusted())
            .filter(|_| self.plan.context.isolation == Some(RuntimeIsolationMode::Shared))
            .map(|a| a.variant);
        tracing::info!(
            session = %self.id,
            reason = class.reason.as_str(),
            detail = class.reason.detail().unwrap_or_default(),
            duration_ms,
            counted,
            "session ended"
        );
        let mut recorded = false;
        for attempt in 0..self.cfg.close_attempts.max(1) {
            match self.deps.store.close(&close, audit) {
                Ok(outcome) => {
                    recorded = matches!(outcome, CloseOutcome::Closed { .. });
                    if !recorded {
                        tracing::warn!(?outcome, "the row was closed by someone else");
                    }
                    break;
                }
                Err(e) => {
                    tracing::warn!(attempt, "closing the session failed: {e}");
                    std::thread::sleep(self.cfg.close_retry_delay);
                    if attempt + 1 == self.cfg.close_attempts.max(1) {
                        self.write_final(&close, audit);
                    }
                }
            }
        }
        self.cleanup();
        SupervisorOutcome::Ended {
            session: self.id,
            reason: class.reason,
            counted,
            recorded,
            spawn_failed,
        }
    }

    fn write_final(&self, close: &SessionClose, audit: Option<SlrVariant>) {
        let rec = FinalRecord {
            format: 1,
            session: close.id,
            ended_at: close.ended_at,
            duration_ms: close.duration_ms,
            end_reason: close.end_reason,
            exit_status: close.exit_status,
            counted: close.counted,
            audit,
        };
        let path = self.plan.paths.log_dir.join(FINAL_FILE);
        let bytes = serde_json::to_vec_pretty(&rec).unwrap_or_default();
        match self.deps.fs.atomic_write(&path, &bytes, 0o600) {
            Ok(()) => tracing::warn!("database unavailable: final record written to final.json"),
            Err(e) => tracing::error!("final record could not be written either: {e}"),
        }
    }

    fn cleanup(&mut self) {
        if let Some(mut s) = self.sound.take() {
            if let Err(e) = s.kill_monitor() {
                tracing::warn!("sound server not stopped: {e}");
            }
            // Reap it: `SIGKILL` ends it at once.
            for _ in 0..100 {
                if !matches!(s.try_wait(), Ok(None)) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let fs = &*self.deps.fs;
        let paths = &self.plan.paths;
        if let Some(sv) = &paths.session_var
            && let Some(root) = sv.parent()
            && let Err(e) = safe_remove_tree(fs, root, sv)
        {
            tracing::warn!("session variable directory not removed: {e}");
        }
        if let Some(root) = paths.runtime_dir.parent()
            && let Err(e) = safe_remove_tree(fs, root, &paths.runtime_dir)
        {
            tracing::warn!("session runtime directory not removed: {e}");
        }
        self.locks.clear();
    }
}
