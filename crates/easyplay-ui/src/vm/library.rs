//! Library view-model: collection, query, filters, sort, grouping, view mode, cursor and
//! multi-selection (GAME_LIBRARY §3–6, §9). Searching, sorting and filtering are
//! `library::query::run_query` over the snapshot, cached by `(revision, query)`
//! (PERFORMANCE §4): a keystroke updates results in the same frame.

use std::collections::HashSet;
use std::sync::Arc;

use easyplay_core::ids::GameId;
use easyplay_core::library::{
    Filters, Grouping, LibraryQuery, LibrarySnapshot, PresetFilter, QueryResult, SortKey, run_query,
};
use easyplay_core::model::{Availability, EntryKind, NetworkAccess};

pub use easyplay_core::library::Collection;

use crate::strings;

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

/// Display helpers for collections.
pub trait CollectionExt {
    /// Shown only when non-empty.
    fn conditional(self) -> bool;
    /// Label for fixed collections (tags use their own name).
    fn label(self) -> &'static str;
}

impl CollectionExt for Collection {
    fn conditional(self) -> bool {
        matches!(
            self,
            Collection::Running | Collection::Missing | Collection::Installers | Collection::Hidden
        )
    }

    fn label(self) -> &'static str {
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
}

/// Count of games in a collection (sidebar), from the snapshot's totals.
pub fn count(lib: &LibrarySnapshot, c: Collection) -> usize {
    let t = &lib.totals;
    (match c {
        Collection::All => t.all,
        Collection::Favorites => t.favorites,
        Collection::Recent => t.recent,
        Collection::NeverPlayed => t.never_played,
        Collection::Running => t.running,
        Collection::Installed => t.installed,
        Collection::Missing => t.missing,
        Collection::Installers => t.installers,
        Collection::Hidden => t.hidden,
        Collection::Tag(id) => lib.tag(id).map_or(0, |t| t.count),
    }) as usize
}

/// The sort keys offered, in menu order (GAME_LIBRARY §5).
pub const SORTS: [SortKey; 7] = [
    SortKey::Name,
    SortKey::LastPlayed,
    SortKey::Playtime,
    SortKey::DateAdded,
    SortKey::Launches,
    SortKey::Protection,
    SortKey::Layer,
];

/// Sort label.
pub fn sort_label(s: SortKey) -> &'static str {
    match s {
        SortKey::Name => strings::library::SORT_NAME,
        SortKey::LastPlayed => strings::library::SORT_RECENT,
        SortKey::Playtime => strings::library::SORT_PLAYTIME,
        SortKey::DateAdded => strings::library::SORT_ADDED,
        SortKey::Launches => strings::library::SORT_LAUNCHES,
        SortKey::Protection => strings::library::SORT_PROTECTION,
        SortKey::Layer => strings::library::SORT_LAYER,
    }
}

/// Groupings offered.
pub const GROUPINGS: [Grouping; 4] = [
    Grouping::None,
    Grouping::FirstLetter,
    Grouping::Tag,
    Grouping::Protection,
];

/// Grouping label.
pub fn grouping_label(g: Grouping) -> &'static str {
    match g {
        Grouping::None => strings::library::GROUP_NONE,
        Grouping::FirstLetter => strings::library::GROUP_LETTER,
        Grouping::Tag => strings::library::GROUP_TAG,
        Grouping::Protection => strings::library::GROUP_PROTECTION,
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

/// Optional list columns (GAME_LIBRARY §3; name and Play are always shown).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Column {
    /// Protection preset.
    Protection,
    /// Last played.
    LastPlayed,
    /// Total playtime.
    Playtime,
    /// Tags.
    Tags,
    /// Compatibility layer.
    Layer,
    /// Date added.
    Added,
}

impl Column {
    /// Every optional column in display order.
    pub const ALL: [Column; 6] = [
        Column::Protection,
        Column::LastPlayed,
        Column::Playtime,
        Column::Tags,
        Column::Layer,
        Column::Added,
    ];

    /// Header label.
    pub fn label(self) -> &'static str {
        match self {
            Column::Protection => strings::library::COL_PROTECTION,
            Column::LastPlayed => strings::library::COL_LAST_PLAYED,
            Column::Playtime => strings::library::COL_PLAYTIME,
            Column::Tags => strings::library::COL_TAGS,
            Column::Layer => strings::library::COL_LAYER,
            Column::Added => strings::library::COL_ADDED,
        }
    }

    /// The sort a click on the header selects.
    pub fn sort(self) -> Option<SortKey> {
        match self {
            Column::Protection => Some(SortKey::Protection),
            Column::LastPlayed => Some(SortKey::LastPlayed),
            Column::Playtime => Some(SortKey::Playtime),
            Column::Layer => Some(SortKey::Layer),
            Column::Added => Some(SortKey::DateAdded),
            Column::Tags => None,
        }
    }

    /// Default width in points.
    pub fn default_width(self) -> f32 {
        match self {
            Column::Protection => 110.0,
            Column::LastPlayed | Column::Added => 120.0,
            Column::Playtime => 100.0,
            Column::Tags => 150.0,
            Column::Layer => 150.0,
        }
    }
}

/// One active filter, as a chip (SCREEN_SPECS §2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilterChip {
    /// A tag.
    Tag(easyplay_core::ids::TagId),
    /// A preset.
    Preset(PresetFilter),
    /// Internet.
    Network(NetworkAccess),
    /// Availability.
    Availability(Availability),
    /// Layer label.
    Layer(Arc<str>),
    /// Entry kind.
    Kind(EntryKind),
}

/// Library screen state.
#[derive(Clone, Debug)]
pub struct LibraryVm {
    /// Selected collection.
    pub collection: Collection,
    /// Search text (with operators, GAME_LIBRARY §4).
    pub query: String,
    /// Search across the whole library instead of the collection.
    pub search_all: bool,
    /// Filters.
    pub filters: Filters,
    /// Sort.
    pub sort: SortKey,
    /// Descending.
    pub descending: bool,
    /// Grouping.
    pub group: Grouping,
    /// View mode.
    pub view: ViewMode,
    /// Keyboard cursor / last opened game.
    pub selected: Option<GameId>,
    /// Multi-selection (selection bar shows while non-empty).
    pub selection: Vec<GameId>,
    /// Range anchor for `Shift` selection.
    pub anchor: Option<GameId>,
    /// Visible list columns.
    pub columns: Vec<Column>,
    /// Column widths (persisted through preferences once a key exists).
    pub widths: std::collections::HashMap<Column, f32>,
    /// The game whose context menu is open, and where.
    pub menu: Option<(GameId, egui::Pos2)>,
    /// A card or row had keyboard focus last frame (single-key shortcuts apply to it).
    pub focused: bool,
    /// Screen rectangle of the cursor's card or row (menu anchor for `Shift+F10`).
    pub cursor_rect: Option<egui::Rect>,
    cache: Option<(u64, LibraryQuery, Arc<QueryResult>)>,
}

impl Default for LibraryVm {
    fn default() -> Self {
        Self {
            collection: Collection::All,
            query: String::new(),
            search_all: false,
            filters: Filters::default(),
            sort: SortKey::Name,
            descending: false,
            group: Grouping::None,
            view: ViewMode::Grid,
            selected: None,
            selection: Vec::new(),
            anchor: None,
            columns: vec![
                Column::Protection,
                Column::LastPlayed,
                Column::Playtime,
                Column::Tags,
            ],
            widths: Default::default(),
            menu: None,
            focused: false,
            cursor_rect: None,
            cache: None,
        }
    }
}

impl LibraryVm {
    /// The current query.
    pub fn query(&self) -> LibraryQuery {
        LibraryQuery {
            collection: self.collection,
            text: self.query.clone(),
            filters: self.filters.clone(),
            sort: self.sort,
            descending: self.descending,
            group: self.group,
            search_all: self.search_all,
        }
    }

    /// The visible games (indices into `lib.games`) and group spans. Cached.
    pub fn result(&mut self, lib: &LibrarySnapshot) -> Arc<QueryResult> {
        let q = self.query();
        if let Some((rev, k, v)) = &self.cache
            && *rev == lib.revision
            && *k == q
        {
            return v.clone();
        }
        let r = Arc::new(run_query(lib, &q));
        self.cache = Some((lib.revision, q, r.clone()));
        r
    }

    /// Indices into `lib.games` of the games to show, in display order.
    pub fn visible(&mut self, lib: &LibrarySnapshot) -> Arc<[u32]> {
        self.result(lib).items.clone().into()
    }

    /// Total of the base collection (for "3 of 42").
    pub fn base_total(&self, lib: &LibrarySnapshot) -> usize {
        count(lib, self.collection)
    }

    /// Choose a sort; picking the current one flips the direction (list headers).
    pub fn set_sort(&mut self, key: SortKey, flip_if_same: bool) {
        if flip_if_same && self.sort == key {
            self.descending = !self.descending;
        } else {
            self.sort = key;
            self.descending = key.default_descending();
        }
    }

    /// Switch collection: clears the selection.
    pub fn set_collection(&mut self, c: Collection) {
        if self.collection != c {
            self.collection = c;
            self.selection.clear();
            self.anchor = None;
        }
    }

    /// The active filters as chips.
    pub fn chips(&self) -> Vec<FilterChip> {
        let f = &self.filters;
        let mut out: Vec<FilterChip> = f.tags.iter().map(|t| FilterChip::Tag(*t)).collect();
        out.extend(f.presets.iter().map(|p| FilterChip::Preset(*p)));
        out.extend(f.network.map(FilterChip::Network));
        out.extend(f.availability.iter().map(|a| FilterChip::Availability(*a)));
        out.extend(f.layers.iter().map(|l| FilterChip::Layer(l.clone())));
        out.extend(f.kinds.iter().map(|k| FilterChip::Kind(*k)));
        out
    }

    /// Whether `chip` is active.
    pub fn has_filter(&self, chip: &FilterChip) -> bool {
        self.chips().contains(chip)
    }

    /// Toggle a filter.
    pub fn toggle_filter(&mut self, chip: FilterChip) {
        fn flip<T: PartialEq>(v: &mut Vec<T>, x: T) {
            if let Some(i) = v.iter().position(|y| *y == x) {
                v.remove(i);
            } else {
                v.push(x);
            }
        }
        let f = &mut self.filters;
        match chip {
            FilterChip::Tag(t) => flip(&mut f.tags, t),
            FilterChip::Preset(p) => flip(&mut f.presets, p),
            FilterChip::Network(n) => {
                f.network = if f.network == Some(n) { None } else { Some(n) };
            }
            FilterChip::Availability(a) => flip(&mut f.availability, a),
            FilterChip::Layer(l) => flip(&mut f.layers, l),
            FilterChip::Kind(k) => flip(&mut f.kinds, k),
        }
    }

    /// `Esc` in the library: clears search, then filters, then selection. Returns whether
    /// anything changed.
    pub fn escape(&mut self) -> bool {
        if !self.query.is_empty() {
            self.query.clear();
            self.search_all = false;
            true
        } else if self.filters.is_active() {
            self.filters = Filters::default();
            true
        } else if !self.selection.is_empty() {
            self.selection.clear();
            self.anchor = None;
            true
        } else {
            false
        }
    }

    /// Whether `id` is in the multi-selection.
    pub fn is_selected(&self, id: GameId) -> bool {
        self.selection.contains(&id)
    }

    /// `Ctrl+click`: toggle one game.
    pub fn toggle_select(&mut self, id: GameId) {
        if let Some(i) = self.selection.iter().position(|x| *x == id) {
            self.selection.remove(i);
        } else {
            self.selection.push(id);
        }
        self.anchor = Some(id);
        self.selected = Some(id);
    }

    /// `Shift+click` / `Shift+arrow`: select from the anchor to `id` in visual order.
    pub fn select_range(&mut self, lib: &LibrarySnapshot, visible: &[u32], id: GameId) {
        let pos = |g: GameId| visible.iter().position(|i| lib.games[*i as usize].id == g);
        let anchor = self.anchor.or(self.selected).unwrap_or(id);
        let (Some(a), Some(b)) = (pos(anchor), pos(id)) else {
            self.toggle_select(id);
            return;
        };
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let mut seen: HashSet<GameId> = self.selection.iter().copied().collect();
        for i in &visible[lo..=hi] {
            let gid = lib.games[*i as usize].id;
            if seen.insert(gid) {
                self.selection.push(gid);
            }
        }
        self.anchor = Some(anchor);
        self.selected = Some(id);
    }

    /// `Ctrl+A`: everything visible.
    pub fn select_all(&mut self, lib: &LibrarySnapshot, visible: &[u32]) {
        self.selection = visible.iter().map(|i| lib.games[*i as usize].id).collect();
    }

    /// Width of a list column.
    pub fn width(&self, c: Column) -> f32 {
        self.widths
            .get(&c)
            .copied()
            .unwrap_or_else(|| c.default_width())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    #[test]
    fn collections_count_from_totals() {
        let lib = demo::library(demo::baseline_now());
        assert_eq!(count(&lib, Collection::All), 24);
        assert!(count(&lib, Collection::Favorites) > 0);
        assert!(count(&lib, Collection::Running) > 0);
        assert!(count(&lib, Collection::Missing) > 0);
    }

    #[test]
    fn results_are_cached_by_revision_and_query() {
        let lib = demo::library(demo::baseline_now());
        let mut vm = LibraryVm::default();
        let a = vm.result(&lib);
        let b = vm.result(&lib);
        assert!(Arc::ptr_eq(&a, &b));
        vm.query = "lantern".into();
        let c = vm.result(&lib);
        assert!(!Arc::ptr_eq(&a, &c));
        assert!(c.items.iter().all(|i| {
            lib.games[*i as usize]
                .name
                .to_lowercase()
                .contains("lantern")
        }));
        assert!(!c.items.is_empty());
    }

    #[test]
    fn search_operators_come_from_core() {
        let lib = demo::library(demo::baseline_now());
        let mut vm = LibraryVm {
            query: "is:favorite".into(),
            ..LibraryVm::default()
        };
        let r = vm.visible(&lib);
        assert_eq!(r.len(), count(&lib, Collection::Favorites));
    }

    #[test]
    fn playtime_sort_is_descending_by_default() {
        let lib = demo::library(demo::baseline_now());
        let mut vm = LibraryVm::default();
        vm.set_sort(SortKey::Playtime, false);
        assert!(vm.descending);
        let v = vm.visible(&lib);
        let times: Vec<u64> = v
            .iter()
            .map(|i| lib.games[*i as usize].total_playtime_ms)
            .collect();
        assert!(times.windows(2).all(|w| w[0] >= w[1]));
        vm.set_sort(SortKey::Playtime, true);
        assert!(!vm.descending);
    }

    #[test]
    fn range_selection_follows_visual_order_under_any_sort() {
        let lib = demo::library(demo::baseline_now());
        let mut vm = LibraryVm::default();
        vm.set_sort(SortKey::Playtime, false);
        let v = vm.visible(&lib);
        let id = |p: usize| lib.games[v[p] as usize].id;
        vm.toggle_select(id(2));
        vm.select_range(&lib, &v, id(5));
        assert_eq!(vm.selection, (2..=5).map(id).collect::<Vec<_>>());
        vm.select_all(&lib, &v);
        assert_eq!(vm.selection.len(), v.len());
    }

    #[test]
    fn escape_clears_search_then_filters_then_selection() {
        let mut vm = LibraryVm {
            query: "x".into(),
            ..LibraryVm::default()
        };
        vm.toggle_filter(FilterChip::Preset(PresetFilter::Offline));
        vm.selection
            .push(GameId::from_uuid(uuid::Uuid::from_u128(1)));
        assert!(vm.escape());
        assert!(vm.query.is_empty() && vm.filters.is_active());
        assert!(vm.escape());
        assert!(!vm.filters.is_active());
        assert!(vm.escape());
        assert!(vm.selection.is_empty());
        assert!(!vm.escape());
    }
}
