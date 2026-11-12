//! Library view-model: collection, query, sort, view mode, selection, and the cached list of
//! visible games (GAME_LIBRARY §3–6). Searching, sorting and filtering run in memory over the
//! snapshot and are cached by `(revision, collection, query, sort)` (PERFORMANCE §4).
//!
//! Foundation scope (UI-008): collections, a plain case-insensitive name search and the four
//! sorts. Search operators, filter chips and grouping arrive with UI-011.

use std::sync::Arc;
use std::time::Duration;

use easyplay_core::clock::Timestamp;
use easyplay_core::ids::{GameId, TagId};
use easyplay_core::model::{Activity, Availability, EntryKind, GameSummary};

use crate::bridge::LibraryView;
use crate::strings;

/// "Recently played" window (GAME_LIBRARY §6).
pub const RECENT_WINDOW: Duration = Duration::from_secs(30 * 24 * 3600);
/// "Recently played" size cap.
pub const RECENT_MAX: usize = 30;

/// A sidebar collection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Collection {
    /// `!hidden`.
    #[default]
    All,
    /// `favorite && !hidden`.
    Favorites,
    /// Played within 30 days, top 30.
    Recent,
    /// Never played games.
    NeverPlayed,
    /// `activity != Idle`.
    Running,
    /// `availability == Present`.
    Installed,
    /// Missing, disconnected or unreadable.
    Missing,
    /// Installers.
    Installers,
    /// Hidden games.
    Hidden,
    /// Games with a tag.
    Tag(TagId),
}

impl Collection {
    /// The fixed collections in sidebar order.
    pub const FIXED: [Collection; 9] = [
        Collection::All,
        Collection::Favorites,
        Collection::Recent,
        Collection::NeverPlayed,
        Collection::Running,
        Collection::Installed,
        Collection::Missing,
        Collection::Installers,
        Collection::Hidden,
    ];

    /// Shown only when non-empty.
    pub const fn conditional(self) -> bool {
        matches!(
            self,
            Collection::Running | Collection::Missing | Collection::Installers | Collection::Hidden
        )
    }

    /// Label for fixed collections (tags use their own name).
    pub fn label(self) -> &'static str {
        match self {
            Collection::All => strings::collection::ALL,
            Collection::Favorites => strings::collection::FAVORITES,
            Collection::Recent => strings::collection::RECENT,
            Collection::NeverPlayed => strings::collection::NEVER_PLAYED,
            Collection::Running => strings::collection::RUNNING,
            Collection::Installed => strings::collection::INSTALLED,
            Collection::Missing => strings::collection::MISSING,
            Collection::Installers => strings::collection::INSTALLERS,
            Collection::Hidden => strings::collection::HIDDEN,
            Collection::Tag(_) => strings::nav::TAGS,
        }
    }

    /// Whether `g` belongs (Recent's top-30 cap is applied after sorting).
    pub fn contains(self, g: &GameSummary, now: Timestamp) -> bool {
        match self {
            Collection::All => !g.hidden,
            Collection::Favorites => g.favorite && !g.hidden,
            Collection::Recent => {
                !g.hidden
                    && g.last_played_at
                        .and_then(|t| now.since(t))
                        .is_some_and(|d| d <= RECENT_WINDOW)
            }
            Collection::NeverPlayed => {
                g.session_count == 0 && g.kind == EntryKind::Game && !g.hidden
            }
            Collection::Running => g.activity != Activity::Idle,
            Collection::Installed => g.availability == Availability::Present && !g.hidden,
            Collection::Missing => matches!(
                g.availability,
                Availability::Missing | Availability::DriveDisconnected | Availability::Unreadable
            ),
            Collection::Installers => g.kind == EntryKind::Installer && !g.hidden,
            Collection::Hidden => g.hidden,
            Collection::Tag(t) => !g.hidden && g.tags.contains(&t),
        }
    }
}

/// Sort keys (GAME_LIBRARY §5, subset).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Sort {
    /// By `sort_name`.
    #[default]
    Name,
    /// Most recently played first.
    RecentlyPlayed,
    /// Most playtime first.
    Playtime,
    /// Newest first.
    Added,
}

impl Sort {
    /// In menu order.
    pub const ALL: [Sort; 4] = [
        Sort::Name,
        Sort::RecentlyPlayed,
        Sort::Playtime,
        Sort::Added,
    ];

    /// Label.
    pub fn label(self) -> &'static str {
        match self {
            Sort::Name => strings::library::SORT_NAME,
            Sort::RecentlyPlayed => strings::library::SORT_RECENT,
            Sort::Playtime => strings::library::SORT_PLAYTIME,
            Sort::Added => strings::library::SORT_ADDED,
        }
    }
}

/// Grid or list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ViewMode {
    /// Cover grid.
    #[default]
    Grid,
    /// Table rows.
    List,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CacheKey {
    revision: u64,
    collection: Collection,
    query: String,
    sort: Sort,
}

/// Library screen state.
#[derive(Clone, Debug, Default)]
pub struct LibraryVm {
    /// Selected collection.
    pub collection: Collection,
    /// Search text.
    pub query: String,
    /// Sort.
    pub sort: Sort,
    /// View mode.
    pub view: ViewMode,
    /// Keyboard cursor / last opened game.
    pub selected: Option<GameId>,
    cache: Option<(CacheKey, Arc<[u32]>)>,
}

impl LibraryVm {
    /// Indices into `lib.games` of the games to show, in display order. Cached.
    pub fn visible(&mut self, lib: &LibraryView, now: Timestamp) -> Arc<[u32]> {
        let key = CacheKey {
            revision: lib.revision,
            collection: self.collection,
            query: self.query.clone(),
            sort: self.sort,
        };
        if let Some((k, v)) = &self.cache
            && *k == key
        {
            return v.clone();
        }
        let q = self.query.trim().to_lowercase();
        let mut idx: Vec<u32> = lib
            .games
            .iter()
            .enumerate()
            .filter(|(_, g)| self.collection.contains(g, now))
            .filter(|(_, g)| q.is_empty() || g.name.to_lowercase().contains(&q))
            .map(|(i, _)| i as u32)
            .collect();
        let g = |i: &u32| &lib.games[*i as usize];
        let sort = if self.collection == Collection::Recent {
            Sort::RecentlyPlayed
        } else {
            self.sort
        };
        match sort {
            Sort::Name => idx.sort_by(|a, b| g(a).sort_name.cmp(&g(b).sort_name)),
            Sort::RecentlyPlayed => idx.sort_by(|a, b| {
                g(b).last_played_at
                    .cmp(&g(a).last_played_at)
                    .then(g(a).sort_name.cmp(&g(b).sort_name))
            }),
            Sort::Playtime => idx.sort_by(|a, b| {
                g(b).total_playtime_ms
                    .cmp(&g(a).total_playtime_ms)
                    .then(g(a).sort_name.cmp(&g(b).sort_name))
            }),
            Sort::Added => idx.sort_by_key(|i| std::cmp::Reverse(g(i).added_at)),
        }
        if self.collection == Collection::Recent {
            idx.truncate(RECENT_MAX);
        }
        let v: Arc<[u32]> = idx.into();
        self.cache = Some((key, v.clone()));
        v
    }

    /// Count of games in a collection (sidebar).
    pub fn count(lib: &LibraryView, c: Collection, now: Timestamp) -> usize {
        let n = lib.games.iter().filter(|g| c.contains(g, now)).count();
        if c == Collection::Recent {
            n.min(RECENT_MAX)
        } else {
            n
        }
    }

    /// Total of the base collection (for "3 of 42").
    pub fn base_total(&self, lib: &LibraryView, now: Timestamp) -> usize {
        Self::count(lib, self.collection, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    #[test]
    fn collections_filter_and_count() {
        let now = demo::baseline_now();
        let lib = demo::library(now);
        let all = LibraryVm::count(&lib, Collection::All, now);
        assert!(all > 10);
        assert!(LibraryVm::count(&lib, Collection::Favorites, now) > 0);
        assert!(LibraryVm::count(&lib, Collection::Running, now) > 0);
        assert!(LibraryVm::count(&lib, Collection::Missing, now) > 0);
        assert!(LibraryVm::count(&lib, Collection::Recent, now) <= RECENT_MAX);
    }

    #[test]
    fn search_and_sort_are_cached_by_key() {
        let now = demo::baseline_now();
        let lib = demo::library(now);
        let mut vm = LibraryVm::default();
        let a = vm.visible(&lib, now);
        let b = vm.visible(&lib, now);
        assert!(Arc::ptr_eq(&a, &b));
        let names: Vec<_> = a
            .iter()
            .map(|i| lib.games[*i as usize].sort_name.clone())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);

        vm.query = "LANTERN".into();
        let c = vm.visible(&lib, now);
        assert!(!Arc::ptr_eq(&a, &c));
        assert!(c.iter().all(|i| {
            lib.games[*i as usize]
                .name
                .to_lowercase()
                .contains("lantern")
        }));
        assert!(!c.is_empty());
    }

    #[test]
    fn playtime_sort_is_descending() {
        let now = demo::baseline_now();
        let lib = demo::library(now);
        let mut vm = LibraryVm {
            sort: Sort::Playtime,
            ..LibraryVm::default()
        };
        let v = vm.visible(&lib, now);
        let times: Vec<u64> = v
            .iter()
            .map(|i| lib.games[*i as usize].total_playtime_ms)
            .collect();
        assert!(times.windows(2).all(|w| w[0] >= w[1]));
    }
}
