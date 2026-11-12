//! Parked flows (LOGIC_SPECS §4, SVC-006): a flow that needs a decision raises
//! `Event::Prompt` and leaves its continuation here, keyed by `PromptId`. The AppService
//! keeps serving other commands; `AnswerPrompt` resumes the flow. No flow commits anything
//! before its prompt is answered, so dropping a parked flow (shutdown, a stale answer) is
//! always safe.

use std::collections::HashMap;
use std::path::PathBuf;

use super::command::{LaunchOrigin, OpenOrigin};
use crate::clock::Clock;
use crate::ids::{ExecutableId, GameId, PromptId};
use crate::model::{Fingerprint, PeInfo, PromptAnswer, PromptKind, PromptRequest};

/// What to do once the answer arrives.
#[derive(Clone, Debug, PartialEq)]
pub enum Continuation {
    /// LOGIC_SPECS §1: a known program changed (`ExeChanged`) — update or set up anew.
    OpenChanged {
        /// The opened path (canonical).
        path: PathBuf,
        /// The game.
        game: GameId,
        /// The executable.
        exe: ExecutableId,
        /// The fresh fingerprint.
        fingerprint: Fingerprint,
        /// The fresh PE information.
        pe: Box<PeInfo>,
        /// Origin of the open.
        origin: OpenOrigin,
    },
    /// LOGIC_SPECS §1: `InsideKnownRoot` / `CopyOfKnown` — add as an alternate or set up.
    OpenAttach {
        /// The opened path.
        path: PathBuf,
        /// The game.
        game: GameId,
        /// Origin.
        origin: OpenOrigin,
    },
    /// LOGIC_SPECS §3: the executable changed since it was added; "It's an update"
    /// refreshes the fingerprint and launches again.
    LaunchExeChanged {
        /// The game.
        game: GameId,
        /// The executable.
        exe: Option<ExecutableId>,
        /// Origin.
        origin: LaunchOrigin,
    },
    /// LOGIC_SPECS §3: the compatibility layer is unavailable.
    LaunchRuntime {
        /// The game.
        game: GameId,
        /// The executable.
        exe: Option<ExecutableId>,
        /// Origin.
        origin: LaunchOrigin,
    },
    /// LOGIC_SPECS §11: remove a running game.
    RemoveRunning {
        /// The game.
        game: GameId,
        /// Dialog options.
        options: crate::library::RemoveOptions,
    },
    /// No follow-up besides the answer itself (informational prompts).
    Nothing,
}

/// One parked flow.
#[derive(Clone, Debug, PartialEq)]
pub struct Parked {
    /// The question.
    pub kind: PromptKind,
    /// The continuation.
    pub then: Continuation,
}

/// The parked flows.
#[derive(Debug, Default)]
pub struct Prompts {
    parked: HashMap<PromptId, Parked>,
}

impl Prompts {
    /// Park a flow; returns the request to send with `Event::Prompt`.
    pub fn park(
        &mut self,
        clock: &dyn Clock,
        kind: PromptKind,
        then: Continuation,
    ) -> PromptRequest {
        let id = PromptId::generate(clock);
        self.parked.insert(
            id,
            Parked {
                kind: kind.clone(),
                then,
            },
        );
        PromptRequest { id, kind }
    }

    /// Take the flow parked under `id` if `answer` is one of its choices. A stale ID or an
    /// answer the prompt does not offer returns `None` and leaves the flow parked (stale) or
    /// removes nothing.
    pub fn take(&mut self, id: PromptId, answer: &PromptAnswer) -> Option<Parked> {
        let ok = self.parked.get(&id).is_some_and(|p| p.kind.accepts(answer));
        if ok { self.parked.remove(&id) } else { None }
    }

    /// Whether a flow for `game` waits for an answer.
    pub fn has_game(&self, game: GameId) -> bool {
        self.parked.values().any(|p| p.kind.game() == Some(game))
    }

    /// Parked flows.
    pub fn len(&self) -> usize {
        self.parked.len()
    }

    /// Whether nothing is parked.
    pub fn is_empty(&self) -> bool {
        self.parked.is_empty()
    }

    /// Drop every parked flow (shutdown). Returns how many were cancelled.
    pub fn cancel_all(&mut self) -> usize {
        let n = self.parked.len();
        self.parked.clear();
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::SystemClock;

    #[test]
    fn park_take_and_reject_foreign_answers() {
        let clock = SystemClock::new();
        let game: GameId = "0192f0c2-1c3a-7d4e-8f00-000000000001".parse().unwrap();
        let mut p = Prompts::default();
        let req = p.park(
            &clock,
            PromptKind::RemoveRunningGame { game },
            Continuation::Nothing,
        );
        assert!(p.has_game(game));
        assert!(p.take(req.id, &PromptAnswer::ItsAnUpdate).is_none());
        assert_eq!(p.len(), 1);
        assert!(p.take(req.id, &PromptAnswer::StopAndRemove).is_some());
        assert!(p.is_empty());
        assert!(p.take(req.id, &PromptAnswer::Cancel).is_none());
        p.park(
            &clock,
            PromptKind::RemoveRunningGame { game },
            Continuation::Nothing,
        );
        assert_eq!(p.cancel_all(), 1);
    }
}
