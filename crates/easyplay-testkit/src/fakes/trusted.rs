//! Fakes for the trusted steps of the launch pipeline (RT-009…RT-012, LAUNCH-004):
//!
//! - [`ScriptedSteps`]: a `launch::TrustedSteps` answering every Provision/Prepare run with
//!   a scripted end (and an optional hook that changes the world, e.g. "the download
//!   happened"), and the audit gate with scripted actions. Records everything it was asked.
//! - [`RowLauncher`]: a `SessionLauncher` that behaves like a supervisor towards the
//!   library database: it inserts the session row, appends scripted `phase` events, runs a
//!   hook (files appearing in the store or the cache) and closes the row; a stop request
//!   closes it as `StoppedByUser`. Used to test `launch::LocalTrustedSteps` for real.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use easyplay_core::clock::{Clock, SystemClock};
use easyplay_core::error::{Error, Result};
use easyplay_core::ids::{GameId, RuntimeName};
use easyplay_core::jobs::CancelToken;
use easyplay_core::launch::{
    LaunchPlan, PlannedLaunch, StoreChange, TrustedProgress, TrustedSteps,
};
use easyplay_core::model::{EndReason, Session, SessionState, SlrVariant};
use easyplay_core::procfs::ProcIdentity;
use easyplay_core::runtime::{AuditAction, AuditDue, TrustedEnd};
use easyplay_core::session::{SessionHandle, SessionLauncher, StopMode, StopOutcome};
use easyplay_core::storage::{Db, SessionClose, SessionRepo};
use easyplay_core::umu::UmuPhase;
use parking_lot::Mutex;

/// A hook run when a scripted audit returns.
pub type AuditHook = Arc<dyn Fn() + Send + Sync>;

/// A hook run when a scripted session "runs".
pub type PlanHook = Arc<dyn Fn(&LaunchPlan) + Send + Sync>;

/// The scripted outcome of one trusted session.
#[derive(Clone)]
pub struct StepScript {
    /// Phases reported while it runs.
    pub phases: Vec<UmuPhase>,
    /// Bytes reported while it runs.
    pub bytes: Vec<u64>,
    /// The row's end reason.
    pub end: EndReason,
    /// umu reached `Launching`.
    pub launching: bool,
    /// The log shows a failed connection.
    pub network_failure: bool,
    /// Runs before the end is reported.
    pub hook: Option<PlanHook>,
}

impl StepScript {
    /// A successful session (`Exited{0}`, `Launching` seen).
    pub fn ok() -> Self {
        Self {
            phases: vec![UmuPhase::Launching {
                tool: "proton".into(),
                runtime: "sniper".into(),
            }],
            bytes: Vec::new(),
            end: EndReason::Exited { code: 0 },
            launching: true,
            network_failure: false,
            hook: None,
        }
    }

    /// A session that failed with this end.
    pub fn fails(end: EndReason) -> Self {
        Self {
            phases: Vec::new(),
            bytes: Vec::new(),
            end,
            launching: false,
            network_failure: false,
            hook: None,
        }
    }

    /// With a hook.
    pub fn with_hook(mut self, f: impl Fn(&LaunchPlan) + Send + Sync + 'static) -> Self {
        self.hook = Some(Arc::new(f));
        self
    }
}

impl std::fmt::Debug for StepScript {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StepScript")
            .field("end", &self.end)
            .field("launching", &self.launching)
            .finish_non_exhaustive()
    }
}

/// A scripted [`TrustedSteps`].
#[derive(Default)]
pub struct ScriptedSteps {
    /// `protection.prepare_online`.
    pub prepare_online: bool,
    script: Mutex<VecDeque<StepScript>>,
    runs: Mutex<Vec<PlannedLaunch>>,
    due: Mutex<Option<AuditDue>>,
    audits: Mutex<VecDeque<(AuditAction, Option<AuditHook>)>>,
    audited: Mutex<Vec<SlrVariant>>,
    prepared: Mutex<Vec<(GameId, RuntimeName)>>,
    launched: Mutex<Vec<(GameId, RuntimeName)>>,
    installs: Mutex<usize>,
}

impl ScriptedSteps {
    /// Online prepare on, nothing scripted (a run without a script fails).
    pub fn new() -> Self {
        Self {
            prepare_online: true,
            ..Self::default()
        }
    }

    /// Queue the outcome of the next run.
    pub fn push(&self, s: StepScript) -> &Self {
        self.script.lock().push_back(s);
        self
    }

    /// What `audit_due` answers (default `No`).
    pub fn set_due(&self, d: AuditDue) -> &Self {
        *self.due.lock() = Some(d);
        self
    }

    /// Queue the next audit's action, with a hook run before it is returned.
    pub fn push_audit(&self, a: AuditAction, hook: Option<Arc<dyn Fn() + Send + Sync>>) -> &Self {
        self.audits.lock().push_back((a, hook));
        self
    }

    /// The trusted sessions run, in order.
    pub fn runs(&self) -> Vec<PlannedLaunch> {
        self.runs.lock().clone()
    }

    /// Variants audited.
    pub fn audited(&self) -> Vec<SlrVariant> {
        self.audited.lock().clone()
    }

    /// `prepared` calls.
    pub fn prepared(&self) -> Vec<(GameId, RuntimeName)> {
        self.prepared.lock().clone()
    }

    /// `launched` calls.
    pub fn launched(&self) -> Vec<(GameId, RuntimeName)> {
        self.launched.lock().clone()
    }

    /// `after_install` calls.
    pub fn installs(&self) -> usize {
        *self.installs.lock()
    }
}

impl TrustedSteps for ScriptedSteps {
    fn prepare_online(&self) -> bool {
        self.prepare_online
    }

    fn run(
        &self,
        planned: &PlannedLaunch,
        fast_path: bool,
        on: &mut dyn FnMut(TrustedProgress),
        cancel: &CancelToken,
    ) -> Result<TrustedEnd> {
        self.runs.lock().push(planned.clone());
        let s = self.script.lock().pop_front().ok_or_else(|| {
            Error::new(easyplay_core::error::ErrorCode::IntTaskFailed)
                .with("detail", "no scripted trusted session left")
        })?;
        for p in &s.phases {
            on(TrustedProgress::Phase(p.clone()));
        }
        for b in &s.bytes {
            on(TrustedProgress::Bytes { bytes: *b, rate: 0 });
        }
        if let Some(h) = &s.hook {
            h(&planned.plan);
        }
        Ok(TrustedEnd {
            kind: planned.plan.session.kind,
            reason: s.end,
            launching_seen: s.launching,
            fast_path_stopped: fast_path && s.launching,
            cancelled: cancel.is_cancelled(),
            network_failure: s.network_failure,
            download_interrupted: false,
        })
    }

    fn after_install(&self) -> Result<StoreChange> {
        *self.installs.lock() += 1;
        Ok(StoreChange::default())
    }

    fn audit_due(&self, _variant: SlrVariant) -> AuditDue {
        self.due.lock().unwrap_or(AuditDue::No)
    }

    fn audit(&self, variant: SlrVariant, _cancel: &CancelToken) -> Result<AuditAction> {
        self.audited.lock().push(variant);
        *self.due.lock() = Some(AuditDue::No);
        let (a, hook) = self
            .audits
            .lock()
            .pop_front()
            .unwrap_or((AuditAction::Clean, None));
        if let Some(h) = hook {
            h();
        }
        Ok(a)
    }

    fn prepared(&self, game: GameId, build: &RuntimeName) -> Result<()> {
        self.prepared.lock().push((game, build.clone()));
        Ok(())
    }

    fn launched(&self, game: GameId, build: &RuntimeName) -> Result<()> {
        self.launched.lock().push((game, build.clone()));
        Ok(())
    }
}

/// The script of one [`RowLauncher`] session.
#[derive(Clone)]
pub struct RowScript {
    /// Phases appended as `phase` events, one per `step`.
    pub phases: Vec<UmuPhase>,
    /// Time between steps.
    pub step: Duration,
    /// Runs after the phases (files appear).
    pub hook: Option<PlanHook>,
    /// How the row ends unless stopped first.
    pub end: EndReason,
    /// Time after the hook before the row ends by itself.
    pub linger: Duration,
}

impl RowScript {
    /// A session reporting `phases` and ending with `end`.
    pub fn new(phases: Vec<UmuPhase>, end: EndReason) -> Self {
        Self {
            phases,
            step: Duration::from_millis(20),
            hook: None,
            end,
            linger: Duration::from_millis(50),
        }
    }

    /// With a hook.
    pub fn with_hook(mut self, f: impl Fn(&LaunchPlan) + Send + Sync + 'static) -> Self {
        self.hook = Some(Arc::new(f));
        self
    }

    /// Stay open this long after the hook.
    pub fn linger(mut self, d: Duration) -> Self {
        self.linger = d;
        self
    }
}

/// A launcher that plays a supervisor's part on the session rows.
pub struct RowLauncher {
    db_file: PathBuf,
    script: Mutex<VecDeque<RowScript>>,
    plans: Arc<Mutex<Vec<LaunchPlan>>>,
    stops: Arc<Mutex<Vec<StopMode>>>,
}

impl RowLauncher {
    /// Over the library at `db_file`.
    pub fn new(db_file: PathBuf) -> Self {
        Self {
            db_file,
            script: Mutex::new(VecDeque::new()),
            plans: Arc::new(Mutex::new(Vec::new())),
            stops: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Queue the next session's script.
    pub fn push(&self, s: RowScript) -> &Self {
        self.script.lock().push_back(s);
        self
    }

    /// Plans started.
    pub fn plans(&self) -> Vec<LaunchPlan> {
        self.plans.lock().clone()
    }

    /// Stop requests.
    pub fn stops(&self) -> Vec<StopMode> {
        self.stops.lock().clone()
    }

    fn close(db_file: &std::path::Path, id: easyplay_core::ids::SessionId, end: EndReason) {
        let Ok(mut db) = Db::open_rw(db_file) else {
            return;
        };
        if db.session(id).ok().flatten().is_some_and(|s| s.is_open()) {
            let _ = db.close_session(&SessionClose {
                id,
                ended_at: SystemClock::new().now(),
                duration_ms: 1,
                end_reason: end,
                exit_status: None,
                counted: false,
                pieces: Vec::new(),
            });
        }
    }
}

impl SessionLauncher for RowLauncher {
    fn start(&self, plan: &LaunchPlan) -> std::result::Result<SessionHandle, Error> {
        self.plans.lock().push(plan.clone());
        let script = self
            .script
            .lock()
            .pop_front()
            .unwrap_or_else(|| RowScript::new(Vec::new(), EndReason::Exited { code: 0 }));
        let id = plan.session.id;
        let row = Session {
            id,
            game_id: plan.session.game_id,
            game_name: plan.session.game_name.clone(),
            executable_id: plan.session.executable_id,
            kind: plan.session.kind,
            state: SessionState::Starting,
            started_at: SystemClock::new().now(),
            ended_at: None,
            duration_ms: None,
            active_ms: 0,
            last_heartbeat_at: None,
            end_reason: None,
            exit_status: None,
            counted: false,
            supervisor: None,
            sandbox_pid: None,
            pid_ns: None,
            context: plan.context.clone(),
            tz_offset_min: 0,
        };
        Db::open_rw(&self.db_file)?.insert_session(&row)?;
        let db_file = self.db_file.clone();
        let plan = plan.clone();
        std::thread::spawn(move || {
            for p in &script.phases {
                std::thread::sleep(script.step);
                if let Ok(mut db) = Db::open_rw(&db_file) {
                    let data = serde_json::to_string(p).unwrap_or_default();
                    let _ = db.add_session_event(id, SystemClock::new().now(), "phase", &data);
                }
            }
            if let Some(h) = &script.hook {
                h(&plan);
            }
            std::thread::sleep(script.linger);
            Self::close(&db_file, id, script.end);
        });
        Ok(SessionHandle {
            session_id: id,
            supervisor: ProcIdentity {
                pid: 4242,
                start_time_ticks: 1,
            },
        })
    }

    fn request_stop(
        &self,
        session: &Session,
        mode: StopMode,
    ) -> std::result::Result<StopOutcome, Error> {
        self.stops.lock().push(mode);
        Self::close(&self.db_file, session.id, EndReason::StoppedByUser);
        Ok(StopOutcome::Signalled)
    }
}
