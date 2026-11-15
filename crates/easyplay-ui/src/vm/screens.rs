//! Smaller view-models: add game (SCREEN_SPECS §5), statistics (SCREEN_SPECS_2 §12),
//! settings (§13), onboarding (SCREEN_SPECS §1), artwork sheet (§14), quick launch (§7).

use std::collections::HashMap;
use std::path::PathBuf;

use easyplay_core::artwork::{SearchCandidate, SteamAppIdSource};
use easyplay_core::clock::MonoInstant;
use easyplay_core::error::UserError;
use easyplay_core::ids::{GameId, QueryId};
use easyplay_core::model::ArtSlot;
use easyplay_core::stats::{HistoryRow, LibraryOverview};

use super::AsyncValue;
use crate::bridge::FolderScan;
use crate::route::SettingsTab;

/// The add-game sheet.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AddGameVm {
    /// The folder being scanned.
    pub folder: PathBuf,
    /// The scan's query.
    pub query: Option<QueryId>,
    /// Results so far.
    pub scan: Option<FolderScan>,
    /// Checkbox mode ("Add several").
    pub multi: bool,
    /// Selected rows (indices into the visible list).
    pub chosen: Vec<usize>,
    /// Filtered-out files disclosed.
    pub show_filtered: bool,
}

impl AddGameVm {
    /// Rows to show: main candidates, then the filtered ones when disclosed.
    pub fn rows(&self) -> Vec<&crate::bridge::ProgramFound> {
        let Some(s) = &self.scan else {
            return Vec::new();
        };
        s.programs
            .iter()
            .filter(|p| !p.filtered || self.show_filtered)
            .collect()
    }

    /// New results: pre-select the first (most likely) row.
    pub fn set_scan(&mut self, scan: FolderScan) {
        if self.chosen.is_empty() && !scan.programs.is_empty() {
            self.chosen = vec![0];
        }
        self.scan = Some(scan);
    }

    /// Pick a row (radio) or toggle it (checkbox mode).
    pub fn choose(&mut self, i: usize) {
        if self.multi {
            if let Some(p) = self.chosen.iter().position(|c| *c == i) {
                self.chosen.remove(p);
            } else {
                self.chosen.push(i);
            }
        } else {
            self.chosen = vec![i];
        }
    }

    /// The chosen paths.
    pub fn chosen_paths(&self) -> Vec<PathBuf> {
        let rows = self.rows();
        let mut c = self.chosen.clone();
        c.sort_unstable();
        c.into_iter()
            .filter_map(|i| rows.get(i).map(|p| p.path.clone()))
            .collect()
    }
}

/// Statistics screen.
#[derive(Clone, Debug, Default)]
pub struct StatsVm {
    /// Range index (7 days, 30 days, year, all time).
    pub range: usize,
    /// Results per (range, library revision): returning shows them in the first frame.
    pub cache: HashMap<(usize, u64), LibraryOverview>,
    /// The pending overview query.
    pub pending: Option<(QueryId, usize, u64, MonoInstant)>,
    /// The last failure.
    pub failed: Option<UserError>,
    /// Recent sessions.
    pub recent: AsyncValue<Vec<HistoryRow>, UserError>,
    /// Its query.
    pub recent_q: Option<(QueryId, u64)>,
    /// Weekday distribution (wide layouts).
    pub weekdays: Option<[u64; 7]>,
    /// Its query.
    pub weekdays_q: Option<QueryId>,
    /// Chart as a table.
    pub table: bool,
    /// Focused bar.
    pub bar: Option<usize>,
    /// "Most played": show all.
    pub all_games: bool,
    /// Full history sheet: rows, more, query.
    pub history: Vec<HistoryRow>,
    /// More pages exist.
    pub history_more: bool,
    /// Next page.
    pub history_page: u32,
    /// Its query.
    pub history_q: Option<QueryId>,
    /// Filter by game.
    pub history_game: Option<GameId>,
}

/// Settings screen.
#[derive(Clone, Debug, Default)]
pub struct SettingsVm {
    /// The tab list's keyboard cursor.
    pub tab: SettingsTab,
    /// "Saved" ticks: key → when.
    pub saved: HashMap<String, MonoInstant>,
    /// A preference that needs a restart was changed.
    pub restart: bool,
    /// New global variable rows being typed.
    pub new_env: (String, String),
}

/// Onboarding steps (SCREEN_SPECS §1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OnboardingStep {
    /// Welcome.
    #[default]
    Welcome,
    /// System check.
    SystemCheck,
    /// Compatibility layer.
    Layer,
    /// Opening Windows programs (default handler) and artwork opt-in.
    Handler,
}

impl OnboardingStep {
    /// In order.
    pub const ALL: [OnboardingStep; 4] = [
        OnboardingStep::Welcome,
        OnboardingStep::SystemCheck,
        OnboardingStep::Layer,
        OnboardingStep::Handler,
    ];

    /// Index.
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }
}

/// Onboarding.
#[derive(Clone, Debug, Default)]
pub struct OnboardingVm {
    /// Step.
    pub step: OnboardingStep,
    /// Started by opening a program (reduced form: the check only).
    pub reduced: bool,
    /// The layer download was started.
    pub downloading: bool,
    /// "Use umu's runtime" was pressed; cleared when the runtime arrives.
    pub adopting: bool,
    /// The user chose "Later" for the download.
    pub later: bool,
    /// Steam artwork opt-in (ADR-0018), sent on finish.
    pub artwork_online: bool,
    /// When the check was (re)started (for "Still checking…").
    pub check_started: Option<MonoInstant>,
    /// The step announcement was made.
    pub announced: Option<OnboardingStep>,
}

/// What the artwork sheet found out about the game's Steam app (ADR-0024).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SteamApps {
    /// The app ID this computer gives, and from where.
    pub detected: Option<(u32, SteamAppIdSource)>,
    /// Store search results, closest name first.
    pub candidates: Vec<SearchCandidate>,
}

/// The artwork sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtworkVm {
    /// Game.
    pub game: GameId,
    /// Slots waiting for an import (spinner).
    pub busy: Vec<ArtSlot>,
    /// Inline errors per slot.
    pub errors: Vec<(ArtSlot, String)>,
    /// The search field: words, an app ID or a store link. Empty until the sheet fills it
    /// with the game's name.
    pub term: String,
    /// What was looked up for the sheet (asked once when it opens, then on *Search*).
    pub apps: AsyncValue<SteamApps>,
    /// The pending lookup.
    pub apps_q: Option<QueryId>,
    /// The app whose artwork is being downloaded.
    pub fetching: Option<u32>,
    /// Why the last download from Steam failed, shown under the results.
    pub fetch_error: Option<String>,
}

impl ArtworkVm {
    /// A sheet for `game`; `term` is the search field's first text (the game's name).
    pub fn new(game: GameId, term: Option<String>) -> Self {
        Self {
            game,
            busy: Vec::new(),
            errors: Vec::new(),
            term: term.unwrap_or_default(),
            apps: AsyncValue::Idle,
            apps_q: None,
            fetching: None,
            fetch_error: None,
        }
    }

    /// The field as an app ID: digits, or a store link (`store.steampowered.com/app/<id>`).
    pub fn typed_app_id(&self) -> Option<u32> {
        easyplay_core::artwork::parse_user_app_id(&self.term)
    }

    /// A request finished either way: spinners stop.
    pub fn settled(&mut self) {
        self.busy.clear();
        self.fetching = None;
    }
}

/// The quick-launch window (SCREEN_SPECS §7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickLaunchVm {
    /// The program opened.
    pub path: PathBuf,
    /// The game, once matched.
    pub game: Option<GameId>,
    /// When the game reached Running (auto-close 600 ms later).
    pub running_at: Option<MonoInstant>,
    /// When the launch was refused because the game is already running (auto-close 4 s
    /// later, SCREEN_SPECS §7).
    pub already_at: Option<MonoInstant>,
    /// The window switched to the full layout.
    pub expanded: bool,
    /// The window grew to fit the setup sheet.
    pub grown: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_game_selection_modes() {
        let mut vm = AddGameVm::default();
        vm.set_scan(crate::demo::folder_scan(std::path::Path::new(
            "/home/sam/Games/Celeste",
        )));
        assert_eq!(vm.chosen, vec![0]);
        assert_eq!(vm.rows().len(), 2);
        vm.choose(1);
        assert_eq!(vm.chosen, vec![1]);
        vm.multi = true;
        vm.choose(0);
        assert_eq!(vm.chosen_paths().len(), 2);
        vm.show_filtered = true;
        assert_eq!(vm.rows().len(), 4);
    }

    #[test]
    fn the_search_field_takes_numbers_and_store_links() {
        let mut a = ArtworkVm::new(GameId::from_uuid(uuid::Uuid::from_u128(1)), None);
        a.term = " 1145360 ".into();
        assert_eq!(a.typed_app_id(), Some(1_145_360));
        a.term = "https://store.steampowered.com/app/70/HalfLife/".into();
        assert_eq!(a.typed_app_id(), Some(70));
        a.term = "half life".into();
        assert_eq!(a.typed_app_id(), None);
    }
}
