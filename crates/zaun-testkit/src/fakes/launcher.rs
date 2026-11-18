//! `FakeLauncher` (PROC-008): a `SessionLauncher` for flow tests. It records every plan it is
//! given and answers with a scripted result, without starting any process.

use std::collections::VecDeque;

use parking_lot::Mutex;
use zaun_core::error::{Error, ErrorCode};
use zaun_core::launch::LaunchPlan;
use zaun_core::model::Session;
use zaun_core::procfs::ProcIdentity;
use zaun_core::session::{SessionHandle, SessionLauncher, StopMode, StopOutcome};

/// What the next `start` returns.
#[derive(Clone, Debug)]
pub enum FakeStart {
    /// A session with this supervisor identity.
    Started(ProcIdentity),
    /// This error.
    Fails(ErrorCode),
}

/// A scripted launcher.
#[derive(Debug, Default)]
pub struct FakeLauncher {
    script: Mutex<VecDeque<FakeStart>>,
    plans: Mutex<Vec<LaunchPlan>>,
    stops: Mutex<Vec<(Session, StopMode)>>,
}

impl FakeLauncher {
    /// Every start succeeds with a made-up supervisor unless scripted otherwise.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue the result of the next `start`.
    pub fn push(&self, r: FakeStart) -> &Self {
        self.script.lock().push_back(r);
        self
    }

    /// Plans passed to `start`, in order.
    pub fn plans(&self) -> Vec<LaunchPlan> {
        self.plans.lock().clone()
    }

    /// Stop requests, in order.
    pub fn stops(&self) -> Vec<(Session, StopMode)> {
        self.stops.lock().clone()
    }
}

impl SessionLauncher for FakeLauncher {
    fn start(&self, plan: &LaunchPlan) -> Result<SessionHandle, Error> {
        self.plans.lock().push(plan.clone());
        match self.script.lock().pop_front() {
            Some(FakeStart::Fails(code)) => Err(Error::new(code)),
            Some(FakeStart::Started(supervisor)) => Ok(SessionHandle {
                session_id: plan.session.id,
                supervisor,
            }),
            None => Ok(SessionHandle {
                session_id: plan.session.id,
                supervisor: ProcIdentity {
                    pid: 4242,
                    start_time_ticks: 1,
                },
            }),
        }
    }

    fn request_stop(&self, session: &Session, mode: StopMode) -> Result<StopOutcome, Error> {
        self.stops.lock().push((session.clone(), mode));
        Ok(StopOutcome::Signalled)
    }
}
