//! The dialog queue (SCREEN_SPECS_2 §16, UI-027): confirmations, prompts (LOGIC_SPECS §4)
//! and errors. One dialog shows at a time; further ones wait. Errors say how many more are
//! queued ("1 more problem").

use std::collections::VecDeque;

use easyplay_core::error::UserError;
use easyplay_core::ids::{GameId, RuntimeName};
use easyplay_core::library::{RemoveOptions, ResetOptions};
use easyplay_core::model::{BasePreset, PromptRequest};

use crate::route::{DialogId, Route};

/// What happens after "Discard" in the unsaved-changes dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AfterDiscard {
    /// Go back.
    Back,
    /// Go to a route.
    Route(Route),
    /// Switch the game page to a tab.
    Tab(usize),
}

/// A confirmation of SCREEN_SPECS_2 §16.1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirm {
    /// Remove games (one dialog for a bulk removal).
    RemoveGames {
        /// Games.
        ids: Vec<GameId>,
        /// Options shown as checkboxes.
        options: RemoveOptions,
    },
    /// Reset private Windows storage.
    ResetPrefix {
        /// Game.
        id: GameId,
        /// Options.
        options: ResetOptions,
    },
    /// Unsaved changes.
    DiscardChanges(AfterDiscard),
    /// Switch to a wider preset from a menu.
    WidenProtection {
        /// Games.
        ids: Vec<GameId>,
        /// Preset.
        preset: BasePreset,
    },
    /// Remove a compatibility layer.
    RemoveLayer {
        /// Build.
        name: RuntimeName,
        /// Shown name.
        label: String,
        /// Size freed.
        size: Option<u64>,
        /// Games using it.
        used_by: u32,
    },
    /// Clear all play history.
    ClearStatistics,
    /// Clear one game's history.
    ResetGameStatistics(GameId),
    /// Turn on Developer Mode.
    DeveloperMode,
}

/// A queued dialog.
#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    /// A confirmation.
    Confirm(Confirm),
    /// A prompt from a parked flow.
    Prompt(PromptRequest),
    /// A failure not tied to a place on screen.
    Error(UserError),
}

impl Dialog {
    fn is_error(&self) -> bool {
        matches!(self, Dialog::Error(_))
    }
}

/// Per-dialog editable state (checkboxes, the selected program, details disclosure).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DialogForm {
    /// Details expanded.
    pub details: bool,
    /// "Remember for this file" and similar.
    pub remember: bool,
    /// Selected option index (installer programs, runtime choice).
    pub choice: usize,
}

/// The queue.
#[derive(Clone, Debug, Default)]
pub struct DialogQueue {
    items: VecDeque<(DialogId, Dialog)>,
    next: u32,
    /// State of the dialog on top.
    pub form: DialogForm,
}

impl DialogQueue {
    /// Queue a dialog; returns its ID. A dialog with the same content is not queued twice.
    pub fn push(&mut self, d: Dialog) -> DialogId {
        if let Some((id, _)) = self.items.iter().find(|(_, x)| *x == d) {
            return *id;
        }
        self.next += 1;
        let id = DialogId(self.next);
        self.items.push_back((id, d));
        id
    }

    /// The dialog to show.
    pub fn front(&self) -> Option<(DialogId, &Dialog)> {
        self.items.front().map(|(id, d)| (*id, d))
    }

    /// The dialog with `id`.
    pub fn get(&self, id: DialogId) -> Option<&Dialog> {
        self.items.iter().find(|(i, _)| *i == id).map(|(_, d)| d)
    }

    /// The dialog with `id`, mutably (checkbox options).
    pub fn get_mut(&mut self, id: DialogId) -> Option<&mut Dialog> {
        self.items
            .iter_mut()
            .find(|(i, _)| *i == id)
            .map(|(_, d)| d)
    }

    /// Close the dialog with `id`; returns the next one to show.
    pub fn close(&mut self, id: DialogId) -> Option<DialogId> {
        self.items.retain(|(i, _)| *i != id);
        self.form = DialogForm::default();
        self.front().map(|(i, _)| i)
    }

    /// How many more errors wait behind the one shown.
    pub fn more_errors(&self) -> usize {
        self.items
            .iter()
            .skip(1)
            .filter(|(_, d)| d.is_error())
            .count()
    }

    /// Whether anything is queued.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The prompts waiting for an answer.
    pub fn prompts(&self) -> impl Iterator<Item = &PromptRequest> {
        self.items.iter().filter_map(|(_, d)| match d {
            Dialog::Prompt(p) => Some(p),
            _ => None,
        })
    }

    /// Drop prompts about `game` (the game was removed).
    pub fn forget_game(&mut self, game: GameId) {
        self.items
            .retain(|(_, d)| !matches!(d, Dialog::Prompt(p) if p.kind.game() == Some(game)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(t: &str) -> UserError {
        let mut e = crate::demo::launch_failure(GameId::from_uuid(uuid::Uuid::from_u128(1)), "x");
        e.title = t.into();
        e
    }

    #[test]
    fn errors_queue_one_at_a_time() {
        let mut q = DialogQueue::default();
        let a = q.push(Dialog::Error(err("a")));
        let b = q.push(Dialog::Error(err("b")));
        assert_eq!(q.push(Dialog::Error(err("b"))), b);
        assert_eq!(q.front().map(|f| f.0), Some(a));
        assert_eq!(q.more_errors(), 1);
        assert_eq!(q.close(a), Some(b));
        assert_eq!(q.more_errors(), 0);
        assert_eq!(q.close(b), None);
        assert!(q.is_empty());
    }
}
