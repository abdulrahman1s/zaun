//! `LaunchState`, its legal transitions (LOGIC_SPECS §3) and the launch timeline
//! (LOGGING §5).

use std::fmt::Write as _;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::clock::MonoInstant;
use crate::error::UserError;
use crate::ids::{LaunchId, SessionId};
use crate::model::{EndReason, LaunchStateTag};
use crate::umu::UmuPhase;

/// The state of one launch as reported to the UI.
///
/// Deviation from the listing in LOGIC_SPECS §3: the phase of `Provisioning` and `Preparing`
/// is optional because no umu line has been read when the state is entered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchState {
    /// Preflight checks.
    Validating,
    /// `runtime::resolve`.
    ResolvingRuntime,
    /// A Provision session runs.
    Provisioning {
        /// Last umu phase.
        phase: Option<UmuPhase>,
        /// Bytes downloaded so far.
        bytes: u64,
    },
    /// A Prepare session runs.
    Preparing {
        /// Last umu phase.
        phase: Option<UmuPhase>,
    },
    /// Waiting for a runtime audit (Shared mode).
    AuditingRuntime,
    /// Building the plan.
    Planning,
    /// Starting the supervisor.
    Spawning,
    /// The session is starting.
    Starting {
        /// Last umu phase.
        phase: Option<UmuPhase>,
    },
    /// The game runs.
    Running {
        /// The session.
        session: SessionId,
    },
    /// The launch failed.
    Failed(Box<UserError>),
    /// Cancelled before the session started.
    Cancelled,
    /// The session ended.
    Finished {
        /// Why.
        reason: EndReason,
    },
}

/// The state's discriminant, for transition tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateKind {
    /// [`LaunchState::Validating`].
    Validating,
    /// [`LaunchState::ResolvingRuntime`].
    ResolvingRuntime,
    /// [`LaunchState::Provisioning`].
    Provisioning,
    /// [`LaunchState::Preparing`].
    Preparing,
    /// [`LaunchState::AuditingRuntime`].
    AuditingRuntime,
    /// [`LaunchState::Planning`].
    Planning,
    /// [`LaunchState::Spawning`].
    Spawning,
    /// [`LaunchState::Starting`].
    Starting,
    /// [`LaunchState::Running`].
    Running,
    /// [`LaunchState::Failed`].
    Failed,
    /// [`LaunchState::Cancelled`].
    Cancelled,
    /// [`LaunchState::Finished`].
    Finished,
}

impl StateKind {
    /// Every kind.
    pub const ALL: [StateKind; 12] = [
        StateKind::Validating,
        StateKind::ResolvingRuntime,
        StateKind::Provisioning,
        StateKind::Preparing,
        StateKind::AuditingRuntime,
        StateKind::Planning,
        StateKind::Spawning,
        StateKind::Starting,
        StateKind::Running,
        StateKind::Failed,
        StateKind::Cancelled,
        StateKind::Finished,
    ];

    /// Stable name (events, logs).
    pub const fn as_str(self) -> &'static str {
        match self {
            StateKind::Validating => "validating",
            StateKind::ResolvingRuntime => "resolving_runtime",
            StateKind::Provisioning => "provisioning",
            StateKind::Preparing => "preparing",
            StateKind::AuditingRuntime => "auditing_runtime",
            StateKind::Planning => "planning",
            StateKind::Spawning => "spawning",
            StateKind::Starting => "starting",
            StateKind::Running => "running",
            StateKind::Failed => "failed",
            StateKind::Cancelled => "cancelled",
            StateKind::Finished => "finished",
        }
    }

    /// Whether no transition leaves this state.
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            StateKind::Failed | StateKind::Cancelled | StateKind::Finished
        )
    }

    /// Whether the launch UI offers Cancel (every state before `Starting`).
    pub const fn cancellable(self) -> bool {
        matches!(
            self,
            StateKind::Validating
                | StateKind::ResolvingRuntime
                | StateKind::Provisioning
                | StateKind::Preparing
                | StateKind::AuditingRuntime
                | StateKind::Planning
                | StateKind::Spawning
        )
    }

    /// Whether `self → next` is legal (LOGIC_SPECS §3). Progress updates within
    /// `Provisioning`, `Preparing` and `Starting` are self-transitions.
    pub const fn can_go_to(self, next: StateKind) -> bool {
        use StateKind as S;
        if matches!(next, S::Cancelled) {
            return self.cancellable();
        }
        if matches!(next, S::Failed) {
            return !self.is_terminal() && !matches!(self, S::Running);
        }
        matches!(
            (self, next),
            (S::Validating, S::ResolvingRuntime)
                | (
                    S::ResolvingRuntime,
                    S::Provisioning | S::Preparing | S::AuditingRuntime | S::Planning
                )
                | (S::Provisioning, S::Provisioning | S::ResolvingRuntime)
                | (S::Preparing, S::Preparing | S::ResolvingRuntime)
                | (S::AuditingRuntime, S::Planning | S::ResolvingRuntime)
                | (S::Planning, S::Spawning)
                | (S::Spawning, S::Starting)
                | (S::Starting, S::Starting | S::Running | S::Finished)
                | (S::Running, S::Finished)
        )
    }
}

impl LaunchState {
    /// The discriminant.
    pub const fn kind(&self) -> StateKind {
        match self {
            LaunchState::Validating => StateKind::Validating,
            LaunchState::ResolvingRuntime => StateKind::ResolvingRuntime,
            LaunchState::Provisioning { .. } => StateKind::Provisioning,
            LaunchState::Preparing { .. } => StateKind::Preparing,
            LaunchState::AuditingRuntime => StateKind::AuditingRuntime,
            LaunchState::Planning => StateKind::Planning,
            LaunchState::Spawning => StateKind::Spawning,
            LaunchState::Starting { .. } => StateKind::Starting,
            LaunchState::Running { .. } => StateKind::Running,
            LaunchState::Failed(_) => StateKind::Failed,
            LaunchState::Cancelled => StateKind::Cancelled,
            LaunchState::Finished { .. } => StateKind::Finished,
        }
    }

    /// The coarse tag shown on cards, for states before `Running`.
    pub const fn tag(&self) -> Option<LaunchStateTag> {
        Some(match self.kind() {
            StateKind::Validating => LaunchStateTag::Validating,
            StateKind::ResolvingRuntime => LaunchStateTag::ResolvingRuntime,
            StateKind::Provisioning => LaunchStateTag::Provisioning,
            StateKind::Preparing => LaunchStateTag::Preparing,
            StateKind::AuditingRuntime => LaunchStateTag::AuditingRuntime,
            StateKind::Planning => LaunchStateTag::Planning,
            StateKind::Spawning => LaunchStateTag::Spawning,
            StateKind::Starting => LaunchStateTag::Starting,
            _ => return None,
        })
    }
}

/// A rejected transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("illegal launch transition {from:?} → {to:?}")]
pub struct IllegalTransition {
    /// Current state.
    pub from: StateKind,
    /// Requested state.
    pub to: StateKind,
}

/// One launch: its identifier, current state and timeline. Every change goes through
/// [`Launch::advance`], so an illegal sequence cannot be recorded.
#[derive(Clone, Debug)]
pub struct Launch {
    id: LaunchId,
    state: LaunchState,
    timeline: Timeline,
}

impl Launch {
    /// A launch entering `Validating` at `now`.
    pub fn start(id: LaunchId, now: MonoInstant) -> Self {
        let mut timeline = Timeline::new(now);
        timeline.mark(Mark::State(StateKind::Validating), now);
        Self {
            id,
            state: LaunchState::Validating,
            timeline,
        }
    }

    /// The identifier.
    pub fn id(&self) -> LaunchId {
        self.id
    }

    /// The current state.
    pub fn state(&self) -> &LaunchState {
        &self.state
    }

    /// The timeline so far.
    pub fn timeline(&self) -> &Timeline {
        &self.timeline
    }

    /// Move to `next` at `now`.
    pub fn advance(
        &mut self,
        next: LaunchState,
        now: MonoInstant,
    ) -> Result<(), IllegalTransition> {
        let (from, to) = (self.state.kind(), next.kind());
        if !from.can_go_to(to) {
            return Err(IllegalTransition { from, to });
        }
        if from != to {
            self.timeline.mark(Mark::State(to), now);
        }
        self.state = next;
        Ok(())
    }

    /// Record a non-state event (LOGGING §5 segment boundary) at `now`.
    pub fn mark(&mut self, mark: Mark, now: MonoInstant) {
        self.timeline.mark(mark, now);
    }
}

// ---- timeline -----------------------------------------------------------------------------

/// A point on the launch timeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mark {
    /// A state was entered.
    State(StateKind),
    /// bwrap reported the sandbox's child (`child-pid`).
    ChildPid,
    /// The first `Launching` phase line.
    UmuLaunching,
    /// The first pressure-vessel `pv-adverb` process was seen.
    ContainerSeen,
    /// The first Wine process was seen.
    WineSeen,
    /// Running was detected.
    RunningDetected,
}

/// The timeline of one launch on the monotonic clock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timeline {
    start: MonoInstant,
    marks: Vec<(Mark, MonoInstant)>,
}

impl Timeline {
    /// A timeline starting at `start`.
    pub fn new(start: MonoInstant) -> Self {
        Self {
            start,
            marks: Vec::new(),
        }
    }

    /// Record `mark` (only its first occurrence counts for segments).
    pub fn mark(&mut self, mark: Mark, at: MonoInstant) {
        self.marks.push((mark, at));
    }

    /// Every mark with its offset from the start, in order.
    pub fn entries(&self) -> impl Iterator<Item = (Mark, Duration)> + '_ {
        self.marks
            .iter()
            .map(|(m, t)| (*m, t.saturating_duration_since(self.start)))
    }

    fn first(&self, m: Mark) -> Option<MonoInstant> {
        self.marks.iter().find(|(x, _)| *x == m).map(|(_, t)| *t)
    }

    /// Time spent in state `s` (all visits), up to the next state mark.
    fn in_state(&self, s: StateKind) -> Duration {
        let states: Vec<(StateKind, MonoInstant)> = self
            .marks
            .iter()
            .filter_map(|(m, t)| match m {
                Mark::State(k) => Some((*k, *t)),
                _ => None,
            })
            .collect();
        let mut total = Duration::ZERO;
        for (i, (k, t)) in states.iter().enumerate() {
            if *k == s
                && let Some((_, end)) = states.get(i + 1)
            {
                total += end.saturating_duration_since(*t);
            }
        }
        total
    }

    fn between(&self, a: Mark, b: Mark) -> Option<Duration> {
        Some(self.first(b)?.saturating_duration_since(self.first(a)?))
    }

    /// The segments of LOGGING §5.
    pub fn timing(&self) -> LaunchTiming {
        let ms = |d: Option<Duration>| d.map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        let state = |s| ms(Some(self.in_state(s))).unwrap_or(0);
        LaunchTiming {
            validate: state(StateKind::Validating),
            resolve: state(StateKind::ResolvingRuntime),
            audit: state(StateKind::AuditingRuntime),
            plan: state(StateKind::Planning),
            spawn: ms(self.between(Mark::State(StateKind::Spawning), Mark::ChildPid)),
            umu_setup: ms(self.between(Mark::ChildPid, Mark::UmuLaunching)),
            container: ms(self.between(Mark::UmuLaunching, Mark::ContainerSeen)),
            proton: ms(self.between(Mark::ContainerSeen, Mark::WineSeen)),
            first_game_process: ms(self
                .first(Mark::RunningDetected)
                .map(|t| t.saturating_duration_since(self.start))),
            total: ms(self
                .marks
                .last()
                .map(|(_, t)| t.saturating_duration_since(self.start))),
        }
    }
}

/// Launch timing in milliseconds; `None` when a boundary was not reached.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchTiming {
    /// Total time to the last mark.
    pub total: Option<u64>,
    /// Validating.
    pub validate: u64,
    /// Resolving the runtime.
    pub resolve: u64,
    /// Waiting for an audit.
    pub audit: u64,
    /// Planning.
    pub plan: u64,
    /// Spawning to `child-pid`.
    pub spawn: Option<u64>,
    /// `child-pid` to the first `Launching` phase.
    pub umu_setup: Option<u64>,
    /// `Launching` to the first `pv-adverb`.
    pub container: Option<u64>,
    /// `pv-adverb` to the first Wine process.
    pub proton: Option<u64>,
    /// Start to Running.
    pub first_game_process: Option<u64>,
}

impl LaunchTiming {
    /// The application-log line of LOGGING §5 (`-` for unreached segments).
    pub fn summary_line(&self, game_short: &str) -> String {
        let o = |v: Option<u64>| v.map_or_else(|| "-".to_owned(), |v| v.to_string());
        let mut s = String::new();
        let _ = write!(
            s,
            "launch timing game={game_short} total_ms={} validate={} resolve={} audit={} plan={} \
             spawn={} umu_setup={} container={} proton={} first_game_process={}",
            o(self.total),
            self.validate,
            self.resolve,
            self.audit,
            self.plan,
            o(self.spawn),
            o(self.umu_setup),
            o(self.container),
            o(self.proton),
            o(self.first_game_process),
        );
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{ErrorCode, catalog};

    fn kind_example(k: StateKind) -> LaunchState {
        match k {
            StateKind::Validating => LaunchState::Validating,
            StateKind::ResolvingRuntime => LaunchState::ResolvingRuntime,
            StateKind::Provisioning => LaunchState::Provisioning {
                phase: None,
                bytes: 0,
            },
            StateKind::Preparing => LaunchState::Preparing { phase: None },
            StateKind::AuditingRuntime => LaunchState::AuditingRuntime,
            StateKind::Planning => LaunchState::Planning,
            StateKind::Spawning => LaunchState::Spawning,
            StateKind::Starting => LaunchState::Starting { phase: None },
            StateKind::Running => LaunchState::Running {
                session: SessionId::from_uuid(uuid::Uuid::nil()),
            },
            StateKind::Failed => LaunchState::Failed(Box::new(catalog::describe(
                &crate::Error::new(ErrorCode::LibExeNotFound),
                &catalog::DescribeCtx {
                    game: None,
                    session: None,
                    redact: &|s| s.to_owned(),
                },
            ))),
            StateKind::Cancelled => LaunchState::Cancelled,
            StateKind::Finished => LaunchState::Finished {
                reason: EndReason::Exited { code: 0 },
            },
        }
    }

    /// The full transition table.
    #[test]
    fn transition_table() {
        use StateKind as S;
        let legal: &[(S, &[S])] = &[
            (
                S::Validating,
                &[S::ResolvingRuntime, S::Failed, S::Cancelled],
            ),
            (
                S::ResolvingRuntime,
                &[
                    S::Provisioning,
                    S::Preparing,
                    S::AuditingRuntime,
                    S::Planning,
                    S::Failed,
                    S::Cancelled,
                ],
            ),
            (
                S::Provisioning,
                &[
                    S::Provisioning,
                    S::ResolvingRuntime,
                    S::Failed,
                    S::Cancelled,
                ],
            ),
            (
                S::Preparing,
                &[S::Preparing, S::ResolvingRuntime, S::Failed, S::Cancelled],
            ),
            (
                S::AuditingRuntime,
                &[S::Planning, S::ResolvingRuntime, S::Failed, S::Cancelled],
            ),
            (S::Planning, &[S::Spawning, S::Failed, S::Cancelled]),
            (S::Spawning, &[S::Starting, S::Failed, S::Cancelled]),
            (
                S::Starting,
                &[S::Starting, S::Running, S::Finished, S::Failed],
            ),
            (S::Running, &[S::Finished]),
            (S::Failed, &[]),
            (S::Cancelled, &[]),
            (S::Finished, &[]),
        ];
        for (from, tos) in legal {
            for to in StateKind::ALL {
                assert_eq!(from.can_go_to(to), tos.contains(&to), "{from:?} → {to:?}");
            }
        }
    }

    #[test]
    fn launch_rejects_illegal_and_records_timeline() {
        let t = |ms| MonoInstant::from_duration(Duration::from_millis(ms));
        let id = LaunchId::from_uuid(uuid::Uuid::nil());
        let mut l = Launch::start(id, t(0));
        assert_eq!(
            l.advance(LaunchState::Planning, t(1)),
            Err(IllegalTransition {
                from: StateKind::Validating,
                to: StateKind::Planning
            })
        );
        let sid = SessionId::from_uuid(uuid::Uuid::nil());
        let steps = [
            (12, LaunchState::ResolvingRuntime),
            (15, LaunchState::Planning),
            (19, LaunchState::Spawning),
            (20, LaunchState::Starting { phase: None }),
        ];
        for (ms, s) in steps {
            l.advance(s, t(ms)).unwrap();
        }
        l.mark(Mark::ChildPid, t(57));
        l.advance(
            LaunchState::Starting {
                phase: Some(UmuPhase::UsingProton { name: "x".into() }),
            },
            t(300),
        )
        .unwrap();
        l.mark(Mark::UmuLaunching, t(697));
        l.mark(Mark::ContainerSeen, t(2587));
        l.mark(Mark::WineSeen, t(3797));
        l.mark(Mark::RunningDetected, t(4312));
        l.advance(LaunchState::Running { session: sid }, t(4312))
            .unwrap();
        let timing = l.timeline().timing();
        assert_eq!(
            timing.summary_line("0192f0c2"),
            "launch timing game=0192f0c2 total_ms=4312 validate=12 resolve=3 audit=0 plan=4 \
             spawn=38 umu_setup=640 container=1890 proton=1210 first_game_process=4312"
        );
        assert!(l.advance(LaunchState::Cancelled, t(5000)).is_err());
        assert_eq!(l.state().tag(), None);
        for k in StateKind::ALL {
            assert_eq!(kind_example(k).kind(), k);
        }
    }
}
