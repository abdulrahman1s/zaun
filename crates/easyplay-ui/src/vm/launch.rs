//! Per-game launch state for buttons, cards and the progress row (USER_EXPERIENCE §7,
//! UI-016). Maps every `LaunchState` to a presentation; keeps the problem panel's failure
//! until the next launch or dismissal, and the suggestions already tried (UF-9).

use std::collections::HashMap;

use easyplay_core::clock::MonoInstant;
use easyplay_core::error::{RecoveryAction, UserError};
use easyplay_core::ids::GameId;
use easyplay_core::launch::{LaunchState, StateKind};
use easyplay_core::model::{BasePreset, EndReason};
use easyplay_core::umu::UmuPhase;

use crate::strings;

/// How a launch state is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Presentation {
    /// Phase text ("Preparing private Windows storage…").
    pub phase: String,
    /// Bytes downloaded (download row).
    pub bytes: Option<u64>,
    /// *Cancel* is offered.
    pub cancellable: bool,
    /// The game runs.
    pub running: bool,
    /// A terminal state.
    pub done: bool,
    /// The reassurance line after 10 s.
    pub reassure: bool,
    /// The sentence under a download: the game stays offline.
    pub offline_note: bool,
}

/// Present a state; `elapsed_s` is the time spent in it.
pub fn present(state: &LaunchState, elapsed_s: f32) -> Presentation {
    let kind = state.kind();
    let (phase, bytes, offline_note) = match state {
        LaunchState::Validating | LaunchState::ResolvingRuntime | LaunchState::Planning => {
            (strings::launch::CHECKING().to_owned(), None, false)
        }
        LaunchState::Provisioning { bytes, .. } => (
            strings::launch::DOWNLOADING().to_owned(),
            Some(*bytes),
            true,
        ),
        LaunchState::Preparing { phase } => (
            match phase {
                Some(UmuPhase::UpgradingPrefix) => strings::launch::UPDATING_STORAGE(),
                _ => strings::launch::PREPARING_STORAGE(),
            }
            .to_owned(),
            None,
            false,
        ),
        LaunchState::AuditingRuntime => (strings::launch::AUDITING().to_owned(), None, false),
        LaunchState::Spawning | LaunchState::Starting { .. } => {
            (strings::launch::STARTING().to_owned(), None, false)
        }
        LaunchState::Running { .. } => (strings::launch::RUNNING().to_owned(), None, false),
        LaunchState::Failed(e) => (e.title.clone(), None, false),
        LaunchState::Cancelled => (strings::launch::CANCELLED().to_owned(), None, false),
        LaunchState::Finished { reason } => {
            (strings::launch::ended(*reason).to_owned(), None, false)
        }
    };
    Presentation {
        phase,
        bytes,
        cancellable: kind.cancellable(),
        running: kind == StateKind::Running,
        done: kind.is_terminal(),
        reassure: !kind.is_terminal() && kind != StateKind::Running && elapsed_s > 10.0,
        offline_note,
    }
}

/// One game's launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameLaunch {
    /// Latest state.
    pub state: LaunchState,
    /// When it was entered.
    pub since: MonoInstant,
    /// Bytes at the previous update and when (for the rate).
    pub last_bytes: Option<(u64, MonoInstant)>,
    /// Bytes per second.
    pub rate: Option<u64>,
}

/// All launches the UI knows about.
#[derive(Clone, Debug, Default)]
pub struct LaunchVm {
    /// Current launches by game.
    pub games: HashMap<GameId, GameLaunch>,
    /// Problem panels by game.
    pub failures: HashMap<GameId, UserError>,
    /// Suggestions applied since the failure (the list shrinks, UF-9).
    pub tried: HashMap<GameId, Vec<RecoveryAction>>,
    /// Games whose session ended within the early-exit window (problem panel without error).
    pub ended_early: HashMap<GameId, EndReason>,
}

impl LaunchVm {
    /// Record a state.
    pub fn update(&mut self, game: GameId, state: LaunchState, now: MonoInstant) {
        if matches!(state, LaunchState::Validating) {
            self.failures.remove(&game);
            self.ended_early.remove(&game);
        }
        if let LaunchState::Failed(e) = &state {
            self.failures.insert(game, (**e).clone());
        }
        if let LaunchState::Finished {
            reason: reason @ (EndReason::Crashed { .. } | EndReason::FailedToStart { .. }),
        } = &state
        {
            self.ended_early.insert(game, *reason);
        }
        let prev = self.games.get(&game);
        let mut rate = prev.and_then(|p| p.rate);
        let mut last_bytes = prev.and_then(|p| p.last_bytes);
        if let LaunchState::Provisioning { bytes, .. } = &state {
            if let Some((b, t)) = last_bytes {
                let dt = now.saturating_duration_since(t).as_secs_f64();
                if dt > 0.2 && *bytes >= b {
                    rate = Some(((*bytes - b) as f64 / dt) as u64);
                }
            }
            last_bytes = Some((*bytes, now));
        }
        let since = match prev {
            Some(p) if p.state.kind() == state.kind() => p.since,
            _ => now,
        };
        self.games.insert(
            game,
            GameLaunch {
                state,
                since,
                last_bytes,
                rate,
            },
        );
    }

    /// The live launch of `game` (not terminal).
    pub fn active(&self, game: GameId) -> Option<&GameLaunch> {
        self.games
            .get(&game)
            .filter(|l| !l.state.kind().is_terminal())
    }

    /// Dismiss the problem panel.
    pub fn dismiss(&mut self, game: GameId) {
        self.failures.remove(&game);
        self.ended_early.remove(&game);
        self.tried.remove(&game);
    }

    /// Remember an applied suggestion.
    pub fn tried(&mut self, game: GameId, a: RecoveryAction) {
        self.tried.entry(game).or_default().push(a);
    }

    /// The suggestions to offer for a failure: the catalogue's actions (minus those tried),
    /// at most three, plus the fallbacks of UF-9 when nothing specific is known.
    pub fn suggestions(&self, game: GameId, err: Option<&UserError>) -> Vec<RecoveryAction> {
        let tried = self.tried.get(&game).cloned().unwrap_or_default();
        let base: Vec<RecoveryAction> = match err {
            Some(e) if !e.actions.is_empty() => e.actions.clone(),
            _ => vec![
                RecoveryAction::ChangeRuntime,
                RecoveryAction::SwitchPreset(BasePreset::Standard),
                RecoveryAction::ResetPrefix,
            ],
        };
        base.into_iter()
            .filter(|a| {
                !tried.contains(a)
                    && !matches!(
                        a,
                        RecoveryAction::ViewLog
                            | RecoveryAction::CopyDetails
                            | RecoveryAction::Dismiss
                            | RecoveryAction::Retry
                    )
            })
            .take(3)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use easyplay_core::ids::SessionId;

    fn every_state() -> Vec<LaunchState> {
        vec![
            LaunchState::Validating,
            LaunchState::ResolvingRuntime,
            LaunchState::Provisioning {
                phase: None,
                bytes: 1024,
            },
            LaunchState::Preparing { phase: None },
            LaunchState::AuditingRuntime,
            LaunchState::Planning,
            LaunchState::Spawning,
            LaunchState::Starting { phase: None },
            LaunchState::Running {
                session: SessionId::from_uuid(uuid::Uuid::from_u128(1)),
            },
            LaunchState::Failed(Box::new(crate::demo::launch_failure(
                GameId::from_uuid(uuid::Uuid::from_u128(1)),
                "X",
            ))),
            LaunchState::Cancelled,
            LaunchState::Finished {
                reason: EndReason::StoppedByUser,
            },
        ]
    }

    /// Every `LaunchState` has a defined presentation; cancel exists before `Starting`.
    #[test]
    fn every_state_is_presented() {
        let all = every_state();
        assert_eq!(all.len(), StateKind::ALL.len());
        for s in all {
            let p = present(&s, 0.0);
            assert!(!p.phase.is_empty(), "{s:?}");
            assert_eq!(p.cancellable, s.kind().cancellable());
            assert_eq!(p.done, s.kind().is_terminal());
        }
        assert!(
            present(
                &LaunchState::Provisioning {
                    phase: None,
                    bytes: 1
                },
                0.0
            )
            .offline_note
        );
        assert!(present(&LaunchState::Validating, 12.0).reassure);
    }

    #[test]
    fn failures_persist_until_the_next_launch() {
        let g = GameId::from_uuid(uuid::Uuid::from_u128(1));
        let t = MonoInstant::ZERO;
        let mut vm = LaunchVm::default();
        vm.update(g, every_state()[9].clone(), t);
        assert!(vm.failures.contains_key(&g));
        let s = vm.suggestions(g, vm.failures.get(&g));
        assert_eq!(s, vec![RecoveryAction::EditPermissions]);
        vm.tried(g, RecoveryAction::EditPermissions);
        assert!(vm.suggestions(g, vm.failures.get(&g)).is_empty());
        vm.update(g, LaunchState::Validating, t);
        assert!(!vm.failures.contains_key(&g));
        assert!(vm.active(g).is_some());
    }
}
