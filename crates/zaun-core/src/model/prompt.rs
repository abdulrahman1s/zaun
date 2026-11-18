//! Prompts: decisions a parked flow needs from the user (LOGIC_SPECS §4).
//!
//! A flow raises a [`PromptRequest`] through `Event::Prompt` and is parked until
//! `Command::AnswerPrompt` delivers a [`PromptAnswer`]. No flow commits anything before its
//! prompt is answered, so a lost prompt aborts safely.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{Fingerprint, RuntimeRef, Tool};
use crate::ids::{ExecutableId, GameId, PromptId};

/// A question for the user.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptRequest {
    /// Identifier used to answer.
    pub id: PromptId,
    /// What is asked.
    pub kind: PromptKind,
}

/// A program found after an installer session (LOGIC_SPECS §13).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramCandidate {
    /// Path inside the game's prefix.
    pub path: PathBuf,
    /// Name to show (shortcut name or sanitised PE product name).
    pub label: String,
    /// Found through a Start Menu or Desktop shortcut.
    pub from_shortcut: bool,
    /// File size.
    pub size: u64,
}

/// The prompts of LOGIC_SPECS §4.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptKind {
    /// A known executable changed identity.
    ExeChanged {
        /// The game.
        game: GameId,
        /// The executable.
        exe: ExecutableId,
        /// Stored fingerprint.
        old: Fingerprint,
        /// Current fingerprint.
        new: Fingerprint,
    },
    /// An unknown executable inside a known game's folder.
    InsideKnownRoot {
        /// The game.
        game: GameId,
        /// The opened file.
        path: PathBuf,
    },
    /// A copy of a known executable elsewhere.
    CopyOfKnown {
        /// The game.
        game: GameId,
        /// The opened file.
        path: PathBuf,
    },
    /// A granted folder now resolves somewhere else.
    GrantTargetChanged {
        /// The game.
        game: GameId,
        /// The grant's chosen path.
        grant: PathBuf,
        /// The stored canonical path.
        old: PathBuf,
        /// The current canonical path.
        new: PathBuf,
    },
    /// The game's compatibility layer cannot be used.
    RuntimeUnavailable {
        /// The game.
        game: GameId,
        /// Its reference.
        reference: RuntimeRef,
    },
    /// A game fix needs the internet.
    FixNeedsNetwork {
        /// The game.
        game: GameId,
        /// Whether the prefix already ran untrusted code (warning text differs).
        prefix_tainted: bool,
    },
    /// A tool needs the internet.
    ToolNeedsNetwork {
        /// The game.
        game: GameId,
        /// The tool.
        tool: Tool,
        /// Whether the prefix already ran untrusted code.
        prefix_tainted: bool,
    },
    /// An installer finished; choose the installed program.
    InstallerFinished {
        /// The game.
        game: GameId,
        /// Candidates, best first (at most 20).
        programs: Vec<ProgramCandidate>,
    },
    /// A grant is unusually broad.
    ConfirmBroadGrant {
        /// The game, when it exists already.
        game: Option<GameId>,
        /// The path.
        path: PathBuf,
        /// Why it is broad (catalogue key, not prose).
        reason: String,
    },
    /// Removal of a running game.
    RemoveRunningGame {
        /// The game.
        game: GameId,
    },
}

impl PromptKind {
    /// The game the prompt is about.
    pub fn game(&self) -> Option<GameId> {
        match self {
            PromptKind::ExeChanged { game, .. }
            | PromptKind::InsideKnownRoot { game, .. }
            | PromptKind::CopyOfKnown { game, .. }
            | PromptKind::GrantTargetChanged { game, .. }
            | PromptKind::RuntimeUnavailable { game, .. }
            | PromptKind::FixNeedsNetwork { game, .. }
            | PromptKind::ToolNeedsNetwork { game, .. }
            | PromptKind::InstallerFinished { game, .. }
            | PromptKind::RemoveRunningGame { game } => Some(*game),
            PromptKind::ConfirmBroadGrant { game, .. } => *game,
        }
    }

    /// The default answer (the first choice in LOGIC_SPECS §4). For
    /// `GrantTargetChanged`, `ConfirmBroadGrant` and `RemoveRunningGame` this is `Cancel`.
    pub fn default_answer(&self) -> PromptAnswer {
        match self {
            PromptKind::ExeChanged { .. } => PromptAnswer::ItsAnUpdate,
            PromptKind::InsideKnownRoot { .. } => {
                PromptAnswer::RunWithGameSettings { remember: false }
            }
            PromptKind::CopyOfKnown { .. } => PromptAnswer::UseSameSettings,
            PromptKind::RuntimeUnavailable { .. } => PromptAnswer::UseDefaultRuntime,
            PromptKind::FixNeedsNetwork { .. } => PromptAnswer::AllowNetworkOnce,
            PromptKind::InstallerFinished { programs, .. } => match programs.first() {
                Some(p) => PromptAnswer::PickProgram {
                    path: p.path.clone(),
                },
                None => PromptAnswer::KeepAsInstaller,
            },
            PromptKind::GrantTargetChanged { .. }
            | PromptKind::ToolNeedsNetwork { .. }
            | PromptKind::ConfirmBroadGrant { .. }
            | PromptKind::RemoveRunningGame { .. } => PromptAnswer::Cancel,
        }
    }

    /// Whether `answer` is one of this prompt's choices.
    pub fn accepts(&self, answer: &PromptAnswer) -> bool {
        use PromptAnswer as A;
        if *answer == A::Cancel {
            return !matches!(self, PromptKind::FixNeedsNetwork { .. });
        }
        match self {
            PromptKind::ExeChanged { .. } => matches!(answer, A::ItsAnUpdate | A::SetUpAsNew),
            PromptKind::InsideKnownRoot { .. } => {
                matches!(answer, A::RunWithGameSettings { .. } | A::SetUpAsNew)
            }
            PromptKind::CopyOfKnown { .. } => matches!(answer, A::UseSameSettings | A::SetUpAsNew),
            PromptKind::GrantTargetChanged { .. } => matches!(answer, A::AllowNewLocation),
            PromptKind::RuntimeUnavailable { .. } => {
                matches!(answer, A::UseDefaultRuntime | A::ChooseRuntime(_))
            }
            PromptKind::FixNeedsNetwork { .. } => matches!(
                answer,
                A::AllowNetworkOnce | A::NotNow | A::TurnOffGameFixes
            ),
            PromptKind::ToolNeedsNetwork { .. } => matches!(answer, A::AllowNetworkOnce),
            PromptKind::InstallerFinished { programs, .. } => match answer {
                A::PickProgram { path } => programs.iter().any(|p| &p.path == path),
                A::KeepAsInstaller | A::RemoveGame => true,
                _ => false,
            },
            PromptKind::ConfirmBroadGrant { .. } => matches!(answer, A::AllowAnyway),
            PromptKind::RemoveRunningGame { .. } => matches!(answer, A::StopAndRemove),
        }
    }
}

/// The user's decision. Which answers a prompt accepts is [`PromptKind::accepts`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptAnswer {
    /// Cancel the flow.
    Cancel,
    /// `ExeChanged`: the file is an update of the game.
    ItsAnUpdate,
    /// `ExeChanged`, `InsideKnownRoot`, `CopyOfKnown`: set up as a separate game.
    SetUpAsNew,
    /// `InsideKnownRoot`: run with the game's settings.
    RunWithGameSettings {
        /// "Remember for this file".
        remember: bool,
    },
    /// `CopyOfKnown`: use the same settings.
    UseSameSettings,
    /// `GrantTargetChanged`: accept the new location.
    AllowNewLocation,
    /// `RuntimeUnavailable`: use the default.
    UseDefaultRuntime,
    /// `RuntimeUnavailable`: use this reference.
    ChooseRuntime(RuntimeRef),
    /// `FixNeedsNetwork`, `ToolNeedsNetwork`: allow the network for one run.
    AllowNetworkOnce,
    /// `FixNeedsNetwork`: not now.
    NotNow,
    /// `FixNeedsNetwork`: disable game fixes.
    TurnOffGameFixes,
    /// `InstallerFinished`: this program is the game.
    PickProgram {
        /// One of the candidates.
        path: PathBuf,
    },
    /// `InstallerFinished`: keep the entry as an installer.
    KeepAsInstaller,
    /// `InstallerFinished`: remove the entry.
    RemoveGame,
    /// `ConfirmBroadGrant`: allow anyway.
    AllowAnyway,
    /// `RemoveRunningGame`: stop, then remove.
    StopAndRemove,
}
