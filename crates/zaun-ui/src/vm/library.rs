//! Library view-model: the scope, search, filters, sort, grouping, layout, cursor and
//! multi-selection of the wall (GAME_LIBRARY §3–6, §9; the design's `inScope`,
//! `shownScopes`, `compare` and `visible`).
//!
//! Searching and the filters the core knows run through `library::query::run_query`; what
//! the design defines differently is applied here on top: the scopes (Recent is two weeks),
//! the protection filter (by level, a customised game counts under the level it started
//! from), the order (the sort key alone, never-played games last under "Recently played")
//! and the groups (one group per game, labels in alphabetical order, "No tag" last). The
//! result is cached by `(revision, query, grouping)` (PERFORMANCE §4): a keystroke updates
//! the wall in the same frame.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use zaun_core::ids::{GameId, TagId};
use zaun_core::library::index::natural_cmp;
use zaun_core::library::{
    Filters, Grouping, LibraryQuery, LibrarySnapshot, PresetFilter, SortKey, SourceFilter,
    run_query,
};
use zaun_core::model::{
    Activity, Availability, BasePreset, EntryKind, GameSummary, NetworkAccess, Preset,
};

pub use zaun_core::library::Collection;

use crate::strings;

/// The scopes of the filter line, in their order.
pub const FIXED: [Collection; 10] = [
    Collection::All,
    Collection::Favorites,
    Collection::Recent,
    Collection::NeverPlayed,
    Collection::Installed,
    Collection::Running,
    Collection::Missing,
    Collection::Installers,
    Collection::Steam,
    Collection::Hidden,
];

/// "Recent" holds the games played within this many days.
pub const RECENT_DAYS: u64 = 14;

/// Display helpers for scopes.
pub trait CollectionExt {
    /// Offered only while it holds something (Installed: only while something is missing).
    fn conditional(self) -> bool;
    /// The scope's word on the filter line.
    fn label(self) -> &'static str;
}

impl CollectionExt for Collection {
    fn conditional(self) -> bool {
        !matches!(
            self,
            Collection::All | Collection::Favorites | Collection::Recent | Collection::NeverPlayed
        )
    }

    fn label(self) -> &'static str {
        match self {
            Collection::All => strings::collection::ALL(),
            Collection::Favorites => strings::collection::FAVORITES(),
            Collection::Recent => strings::collection::RECENT(),
            Collection::NeverPlayed => strings::collection::NEVER_PLAYED(),
            Collection::Running => strings::collection::RUNNING(),
            Collection::Installed => strings::collection::INSTALLED(),
            Collection::Missing => strings::collection::MISSING(),
            Collection::Installers => strings::collection::INSTALLERS(),
            Collection::Hidden => strings::collection::HIDDEN(),
            Collection::Steam => strings::steam::SCOPE(),
            Collection::Tag(_) => strings::collection::ALL(),
        }
    }
}

/// The game's program cannot be reached (missing, on a drive that is not connected,
/// unreadable).
pub fn is_missing(g: &GameSummary) -> bool {
    matches!(
        g.availability,
        Availability::Missing | Availability::DriveDisconnected | Availability::Unreadable
    )
}

/// Played within [`RECENT_DAYS`] of the snapshot's time.
pub fn played_lately(g: &GameSummary, now: zaun_core::clock::Timestamp) -> bool {
    let window = Duration::from_secs(RECENT_DAYS * 24 * 3600);
    g.last_played_at
        .is_some_and(|t| t >= now.saturating_sub(window))
}

/// "Not played yet": never played here (the design's test), or, for a Steam game, no time
/// and no last-played date from Steam (ADR-0031).
pub fn not_played(g: &GameSummary) -> bool {
    if g.is_steam() {
        g.never_played()
    } else {
        g.last_played_at.is_none()
    }
}

/// Whether `g` belongs to scope `c` (the design's `inScope`). Hidden games are only in
/// Hidden.
pub fn in_scope(lib: &LibrarySnapshot, g: &GameSummary, c: Collection) -> bool {
    if c == Collection::Hidden {
        return g.hidden;
    }
    if g.hidden {
        return false;
    }
    match c {
        Collection::All => true,
        Collection::Favorites => g.favorite,
        Collection::Recent => played_lately(g, lib.built_at),
        Collection::NeverPlayed => not_played(g) && g.kind == EntryKind::Game,
        Collection::Installed => !is_missing(g) && g.kind == EntryKind::Game,
        Collection::Running => g.activity != Activity::Idle,
        Collection::Missing => is_missing(g),
        Collection::Installers => g.kind == EntryKind::Installer,
        Collection::Hidden => g.hidden,
        Collection::Steam => g.is_steam(),
        Collection::Tag(t) => g.tags.contains(&t),
    }
}

/// Games in scope `c`.
pub fn count(lib: &LibrarySnapshot, c: Collection) -> usize {
    lib.games.iter().filter(|g| in_scope(lib, g, c)).count()
}

/// Games that are not hidden: the size of the library the wall adapts to.
pub fn library_size(lib: &LibrarySnapshot) -> usize {
    lib.games.iter().filter(|g| !g.hidden).count()
}

/// The sort keys offered, in menu order: the design's four, then the app's others.
pub const SORTS: [SortKey; 7] = [
    SortKey::LastPlayed,
    SortKey::Name,
    SortKey::Playtime,
    SortKey::DateAdded,
    SortKey::Launches,
    SortKey::Protection,
    SortKey::Layer,
];

/// The design's four sort keys; the rest follow under a separator.
pub const DESIGN_SORTS: usize = 4;

/// Sort label.
pub fn sort_label(s: SortKey) -> &'static str {
    match s {
        SortKey::Name => strings::library::SORT_NAME(),
        SortKey::LastPlayed => strings::library::SORT_RECENT(),
        SortKey::Playtime => strings::library::SORT_PLAYTIME(),
        SortKey::DateAdded => strings::library::SORT_ADDED(),
        SortKey::Launches => strings::library::SORT_LAUNCHES(),
        SortKey::Protection => strings::library::SORT_PROTECTION(),
        SortKey::Layer => strings::library::SORT_LAYER(),
    }
}

/// The two orders of a sort key, as the sort menu says them: (descending, ascending).
pub fn order_labels(s: SortKey) -> (&'static str, &'static str) {
    use strings::library as l;
    match s {
        SortKey::LastPlayed => (l::ORDER_RECENT_FIRST(), l::ORDER_OLDEST_FIRST()),
        SortKey::Name | SortKey::Layer => (l::ORDER_Z_A(), l::ORDER_A_Z()),
        SortKey::Playtime => (l::ORDER_MOST_PLAYED(), l::ORDER_LEAST_PLAYED()),
        SortKey::DateAdded => (l::ORDER_NEWEST_FIRST(), l::ORDER_OLDEST_FIRST()),
        SortKey::Launches => (l::ORDER_MOST_LAUNCHED(), l::ORDER_LEAST_LAUNCHED()),
        SortKey::Protection => (l::ORDER_OPEN_FIRST(), l::ORDER_CLOSED_FIRST()),
    }
}

/// How the wall is grouped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Group {
    /// One wall.
    #[default]
    None,
    /// By protection level ("Custom" for a game whose switches differ from its level).
    Protection,
    /// By the game's first tag; "No tag" last.
    Tag,
    /// "Not played yet" and "Played".
    Played,
    /// By the first letter of the name (the app's own grouping, kept).
    Letter,
}

/// Groupings offered, in menu order.
pub const GROUPINGS: [Group; 5] = [
    Group::None,
    Group::Protection,
    Group::Tag,
    Group::Played,
    Group::Letter,
];

/// Grouping label.
pub fn grouping_label(g: Group) -> &'static str {
    match g {
        Group::None => strings::library::GROUP_NONE(),
        Group::Protection => strings::library::GROUP_PROTECTION(),
        Group::Tag => strings::library::GROUP_TAG(),
        Group::Played => strings::library::GROUP_PLAYED(),
        Group::Letter => strings::library::GROUP_LETTER(),
    }
}

/// Covers or list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ViewMode {
    /// The wall of covers.
    #[default]
    Grid,
    /// The list.
    List,
}

/// One active filter, as a word with × on the filter line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilterChip {
    /// A tag.
    Tag(TagId),
    /// A protection level (by the level a game started from; `Custom` is exact).
    Preset(PresetFilter),
    /// Internet.
    Network(NetworkAccess),
    /// Availability.
    Availability(Availability),
    /// Layer label (Zaun's own layers: a Steam game's Proton is Steam's choice).
    Layer(Arc<str>),
    /// Entry kind.
    Kind(EntryKind),
    /// Where the game comes from: Zaun or Steam (ADR-0031).
    Source(SourceFilter),
}

impl FilterChip {
    /// Whether `g` passes this one filter. A Steam game has no protection level and no
    /// internet setting of Zaun's (ADR-0031): those filters never match it.
    pub fn matches(&self, g: &GameSummary) -> bool {
        match self {
            FilterChip::Tag(t) => g.tags.contains(t),
            FilterChip::Preset(p) => !g.is_steam() && preset_matches(*p, g.preset),
            FilterChip::Network(n) => !g.is_steam() && g.network == *n,
            FilterChip::Availability(a) => g.availability == *a,
            FilterChip::Layer(l) => !g.is_steam() && **l == *g.runtime_label,
            FilterChip::Kind(k) => g.kind == *k,
            FilterChip::Source(s) => SourceFilter::of(g) == *s,
        }
    }
}

/// The words of the source filter.
pub fn source_label(s: SourceFilter) -> &'static str {
    match s {
        SourceFilter::Zaun => strings::steam::filter::ZAUN(),
        SourceFilter::Steam => strings::steam::filter::STEAM(),
    }
}

/// The filter menu's *Source* group (Zaun, Steam), offered while the library holds
/// Steam games (ADR-0031).
pub fn source_chips(lib: &LibrarySnapshot) -> Option<Vec<FilterChip>> {
    lib.games.iter().any(GameSummary::is_steam).then(|| {
        [SourceFilter::Zaun, SourceFilter::Steam]
            .map(FilterChip::Source)
            .to_vec()
    })
}

/// The protection filter: a level matches the games at that level, customised or not.
fn preset_matches(f: PresetFilter, p: Preset) -> bool {
    match f {
        PresetFilter::Offline => p.base() == BasePreset::Offline,
        PresetFilter::Online => p.base() == BasePreset::Online,
        PresetFilter::Standard => p.base() == BasePreset::Standard,
        PresetFilter::Custom => matches!(p, Preset::Custom { .. }),
    }
}

/// A run of the shown games under one heading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// The heading.
    pub label: Arc<str>,
    /// First index into [`Shelf::items`].
    pub start: u32,
    /// Number of games.
    pub len: u32,
}

/// What the wall shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shelf {
    /// Indices into the snapshot's games, in display order.
    pub items: Arc<[u32]>,
    /// Headings (empty without grouping).
    pub groups: Vec<Section>,
    /// Games in the scope before search and filters (the "17" of "13 of 17").
    pub pool: usize,
}

/// Library screen state.
#[derive(Clone, Debug)]
pub struct LibraryVm {
    /// The scope (a former tag collection becomes All plus that tag as a filter).
    pub collection: Collection,
    /// Search text (with operators, GAME_LIBRARY §4).
    pub query: String,
    /// Search across the whole library instead of the scope.
    pub search_all: bool,
    /// Filters.
    pub filters: Filters,
    /// Sort.
    pub sort: SortKey,
    /// Descending.
    pub descending: bool,
    /// Grouping.
    pub group: Group,
    /// Covers or list.
    pub view: ViewMode,
    /// Keyboard cursor / last opened game.
    pub selected: Option<GameId>,
    /// Multi-selection (the filter line carries its actions while non-empty).
    pub selection: Vec<GameId>,
    /// Range anchor for `Shift` selection: the last game toggled.
    pub anchor: Option<GameId>,
    /// The game whose menu is open, and where (the pointer, or the corner it hangs from).
    pub menu: Option<(GameId, egui::Pos2)>,
    /// The cover or row the open menu hangs from (opened from the keyboard).
    pub menu_anchor: Option<egui::Rect>,
    /// The game whose tag menu (`T`) hangs from its cover.
    pub tag_menu: Option<GameId>,
    /// A cover or row had keyboard focus last frame (single-key shortcuts apply to it).
    pub focused: bool,
    /// Screen rectangle of the cursor's cover or row (where the Menu key's menu hangs).
    pub cursor_rect: Option<egui::Rect>,
    /// The search field had keyboard focus last frame (the wall reflows quicker).
    pub typing: bool,
    cache: Option<(u64, LibraryQuery, Group, Arc<Shelf>)>,
    counts: Option<(u64, [usize; 10])>,
    /// Bumped whenever the shown order is recomputed.
    generation: u64,
    /// The shown order the cursor last followed, and its generation.
    order: (u64, Vec<GameId>),
}

impl Default for LibraryVm {
    fn default() -> Self {
        Self {
            collection: Collection::All,
            query: String::new(),
            search_all: false,
            filters: Filters::default(),
            sort: SortKey::LastPlayed,
            descending: true,
            group: Group::None,
            view: ViewMode::Grid,
            selected: None,
            selection: Vec::new(),
            anchor: None,
            menu: None,
            menu_anchor: None,
            tag_menu: None,
            focused: false,
            cursor_rect: None,
            typing: false,
            cache: None,
            counts: None,
            generation: 0,
            order: (0, Vec::new()),
        }
    }
}

/// The design's `compare`: by the key alone, never-played games last under "Recently
/// played", Steam games last under "Protection" in both orders (they have no level); ties
/// by name.
fn compare(a: &GameSummary, b: &GameSummary, key: SortKey, desc: bool) -> Ordering {
    let dir = |o: Ordering| if desc { o.reverse() } else { o };
    let rank = |p: Preset| match p {
        Preset::Offline => 0,
        Preset::Online => 1,
        Preset::Standard => 2,
        Preset::Custom { .. } => 3,
    };
    let primary = match key {
        SortKey::LastPlayed => match (a.last_played_at, b.last_played_at) {
            (Some(x), Some(y)) => dir(x.cmp(&y)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        },
        SortKey::Name => dir(natural_cmp(&a.sort_name, &b.sort_name)),
        SortKey::Playtime => dir(a.total_playtime_ms.cmp(&b.total_playtime_ms)),
        SortKey::DateAdded => dir(a.added_at.cmp(&b.added_at)),
        SortKey::Launches => dir(a.launch_count.cmp(&b.launch_count)),
        SortKey::Protection => match (a.is_steam(), b.is_steam()) {
            (false, false) => dir(rank(a.preset).cmp(&rank(b.preset))),
            (false, true) => Ordering::Less,
            (true, false) => Ordering::Greater,
            (true, true) => Ordering::Equal,
        },
        SortKey::Layer => dir(natural_cmp(
            &a.runtime_label.to_lowercase(),
            &b.runtime_label.to_lowercase(),
        )),
    };
    primary
        .then_with(|| natural_cmp(&a.sort_name, &b.sort_name))
        .then_with(|| a.id.cmp(&b.id))
}

/// The heading `g` goes under.
fn group_label(lib: &LibrarySnapshot, g: &GameSummary, by: Group) -> Arc<str> {
    match by {
        Group::None => Arc::from(""),
        Group::Protection if g.is_steam() => Arc::from(strings::steam::RUN_BY_STEAM()),
        Group::Protection => Arc::from(strings::shield::preset_name(g.preset)),
        Group::Tag => g
            .tags
            .first()
            .and_then(|t| lib.tag(*t))
            .map_or_else(|| Arc::from(strings::library::NO_TAG()), |t| t.name.clone()),
        Group::Played => Arc::from(if not_played(g) {
            strings::library::GROUP_NOT_PLAYED()
        } else {
            strings::library::GROUP_HAS_PLAYED()
        }),
        Group::Letter => {
            let c = g
                .sort_name
                .chars()
                .find(|c| !c.is_whitespace())
                .filter(char::is_ascii_alphabetic)
                .map_or('#', |c| c.to_ascii_uppercase());
            Arc::from(c.to_string())
        }
    }
}

/// Headings in the design's order: alphabetical; "No tag", "#" and "Run by Steam" last.
fn heading_order(a: &str, b: &str) -> Ordering {
    let last = |s: &str| {
        s == strings::library::NO_TAG() || s == "#" || s == strings::steam::RUN_BY_STEAM()
    };
    last(a)
        .cmp(&last(b))
        .then_with(|| natural_cmp(&a.to_lowercase(), &b.to_lowercase()))
}

/// Group `items` (already in order) under headings; each game once.
fn group_items(lib: &LibrarySnapshot, items: Vec<u32>, by: Group) -> (Vec<u32>, Vec<Section>) {
    if by == Group::None {
        return (items, Vec::new());
    }
    let mut buckets: Vec<(Arc<str>, Vec<u32>)> = Vec::new();
    for i in items {
        let label = group_label(lib, &lib.games[i as usize], by);
        match buckets.iter_mut().find(|(l, _)| *l == label) {
            Some((_, v)) => v.push(i),
            None => buckets.push((label, vec![i])),
        }
    }
    buckets.sort_by(|a, b| heading_order(&a.0, &b.0));
    let mut out = Vec::new();
    let mut groups = Vec::new();
    for (label, v) in buckets {
        groups.push(Section {
            label,
            start: out.len() as u32,
            len: v.len() as u32,
        });
        out.extend(v);
    }
    (out, groups)
}

impl LibraryVm {
    /// The query handed to the core: the scope and the protection filter are applied here,
    /// so the core sees All (or Hidden) and the other filters.
    pub fn query(&self) -> LibraryQuery {
        let mut filters = self.filters.clone();
        filters.presets.clear();
        LibraryQuery {
            collection: if self.collection == Collection::Hidden {
                Collection::Hidden
            } else {
                Collection::All
            },
            text: self.query.clone(),
            filters,
            sort: self.sort,
            descending: self.descending,
            group: Grouping::None,
            search_all: self.search_all,
        }
    }

    /// A former tag collection becomes All with that tag as a filter.
    fn normalise(&mut self) {
        if let Collection::Tag(t) = self.collection {
            self.collection = Collection::All;
            if !self.filters.tags.contains(&t) {
                self.filters.tags.push(t);
            }
        }
    }

    /// What the wall shows. Cached.
    pub fn result(&mut self, lib: &LibrarySnapshot) -> Arc<Shelf> {
        self.normalise();
        // The cache is keyed by everything the shelf depends on: the scope and the
        // protection filter too, which the core's query does not carry.
        let cache_key = LibraryQuery {
            collection: self.collection,
            filters: self.filters.clone(),
            ..self.query()
        };
        if let Some((rev, k, g, v)) = &self.cache
            && *rev == lib.revision
            && *k == cache_key
            && *g == self.group
        {
            return v.clone();
        }
        let core = run_query(lib, &self.query());
        let everywhere = self.search_all && !self.query.trim().is_empty();
        let scope = self.collection;
        let presets = &self.filters.presets;
        let mut items: Vec<u32> = core
            .items
            .into_iter()
            .filter(|i| {
                let g = &lib.games[*i as usize];
                (everywhere || in_scope(lib, g, scope))
                    && (presets.is_empty()
                        || (!g.is_steam() && presets.iter().any(|p| preset_matches(*p, g.preset))))
            })
            .collect();
        let (key, desc) = (self.sort, self.descending);
        items.sort_by(|a, b| compare(&lib.games[*a as usize], &lib.games[*b as usize], key, desc));
        let (items, groups) = group_items(lib, items, self.group);
        let shelf = Arc::new(Shelf {
            items: items.into(),
            groups,
            pool: count(lib, scope),
        });
        // A new snapshot that shows the same games under the same headings is not a new
        // order: the wall does not reflow for it.
        let same = self
            .cache
            .as_ref()
            .is_some_and(|(_, _, _, old)| old.items == shelf.items && old.groups == shelf.groups);
        if !same {
            self.generation += 1;
        }
        self.cache = Some((lib.revision, cache_key, self.group, shelf.clone()));
        shelf
    }

    /// A number that changes whenever the shown order is recomputed (the wall animates its
    /// covers to their new places when it does).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Games in scope `c`, counted once per snapshot.
    pub fn scope_count(&mut self, lib: &LibrarySnapshot, c: Collection) -> usize {
        let Some(i) = FIXED.iter().position(|x| *x == c) else {
            return count(lib, c);
        };
        match &self.counts {
            Some((rev, counts)) if *rev == lib.revision => counts[i],
            _ => {
                let counts = FIXED.map(|s| count(lib, s));
                self.counts = Some((lib.revision, counts));
                counts[i]
            }
        }
    }

    /// The scopes the filter line offers (the design's `shownScopes`): the first four
    /// always; Installed only while something is missing; the rest while they hold
    /// something, and whichever is current.
    pub fn shown_scopes(&mut self, lib: &LibrarySnapshot) -> Vec<Collection> {
        self.normalise();
        let missing = self.scope_count(lib, Collection::Missing);
        FIXED
            .into_iter()
            .filter(|c| {
                !c.conditional()
                    || *c == self.collection
                    || if *c == Collection::Installed {
                        missing > 0
                    } else {
                        self.scope_count(lib, *c) > 0
                    }
            })
            .collect()
    }

    /// LB / RB on a controller: the previous or next scope the filter line offers.
    pub fn cycle_scope(&mut self, lib: &LibrarySnapshot, forward: bool) {
        let all = self.shown_scopes(lib);
        let n = all.len().max(1);
        let i = all.iter().position(|c| *c == self.collection).unwrap_or(0);
        let j = if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        };
        if let Some(c) = all.get(j) {
            self.set_collection(*c);
        }
    }

    /// A search or a filter is on.
    pub fn refined(&self) -> bool {
        !self.query.trim().is_empty() || self.filters.is_active()
    }

    /// The wall of a small library (up to six games, scope All, nothing filtered, no
    /// groups, covers): fixed covers and the "Add game" tile.
    pub fn sparse(&self, size: usize) -> bool {
        self.view == ViewMode::Grid
            && size <= crate::theme::tokens::layout::SPARSE_UP_TO
            && self.collection == Collection::All
            && !self.refined()
            && self.group == Group::None
    }

    /// Focus following by game ID (ACCESSIBILITY §2.1, UX-002): after the shown order
    /// changed, keep the cursor on its game or move it to the nearest remaining one.
    /// Returns the new cursor when it moved to another game.
    pub fn follow(&mut self, lib: &LibrarySnapshot) -> Option<GameId> {
        let r = self.result(lib);
        if self.order.0 == self.generation {
            return None;
        }
        let ids: Vec<GameId> = r.items.iter().map(|i| lib.games[*i as usize].id).collect();
        let next = crate::input::focus::follow(self.selected, &self.order.1, &ids);
        self.order = (self.generation, ids);
        // A selection keeps only the games still in the library.
        self.selection.retain(|g| lib.by_id.contains_key(g));
        if next != self.selected && self.selected.is_some() {
            self.selected = next;
            return next;
        }
        None
    }

    /// Indices into `lib.games` of the games to show, in display order.
    pub fn visible(&mut self, lib: &LibrarySnapshot) -> Arc<[u32]> {
        self.result(lib).items.clone()
    }

    /// Games in the scope before search and filters (for "13 of 17").
    pub fn base_total(&mut self, lib: &LibrarySnapshot) -> usize {
        self.result(lib).pool
    }

    /// Choose a sort; picking the current one flips the direction (list headers). A new
    /// key starts in its own direction.
    pub fn set_sort(&mut self, key: SortKey, flip_if_same: bool) {
        if flip_if_same && self.sort == key {
            self.descending = !self.descending;
        } else if self.sort != key {
            self.sort = key;
            self.descending = key.default_descending();
        }
    }

    /// Switch scope: clears the selection.
    pub fn set_collection(&mut self, c: Collection) {
        if self.collection != c {
            self.collection = c;
            self.selection.clear();
            self.anchor = None;
        }
        self.normalise();
    }

    /// The active filters, in the order the line shows them: protection, then tags, then
    /// the app's other filters.
    pub fn chips(&self) -> Vec<FilterChip> {
        let f = &self.filters;
        let mut out: Vec<FilterChip> = f.presets.iter().map(|p| FilterChip::Preset(*p)).collect();
        out.extend(f.tags.iter().map(|t| FilterChip::Tag(*t)));
        out.extend(f.network.map(FilterChip::Network));
        out.extend(f.availability.iter().map(|a| FilterChip::Availability(*a)));
        out.extend(f.layers.iter().map(|l| FilterChip::Layer(l.clone())));
        out.extend(f.kinds.iter().map(|k| FilterChip::Kind(*k)));
        out.extend(f.sources.iter().map(|s| FilterChip::Source(*s)));
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
            FilterChip::Source(s) => flip(&mut f.sources, s),
        }
    }

    /// How many games of the scope pass `chip` (the counts of the filter menu).
    pub fn filter_count(&self, lib: &LibrarySnapshot, chip: &FilterChip) -> usize {
        lib.games
            .iter()
            .filter(|g| in_scope(lib, g, self.collection) && chip.matches(g))
            .count()
    }

    /// "Clear": the search and every filter.
    pub fn clear_refinements(&mut self) {
        self.query.clear();
        self.search_all = false;
        self.filters = Filters::default();
    }

    /// `Esc` in the library: clears the search, then the selection. Returns whether
    /// anything changed.
    pub fn escape(&mut self) -> bool {
        if !self.query.is_empty() {
            self.query.clear();
            self.search_all = false;
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

    /// `Ctrl+click`, `Space`: toggle one game; it becomes the anchor.
    pub fn toggle_select(&mut self, id: GameId) {
        if let Some(i) = self.selection.iter().position(|x| *x == id) {
            self.selection.remove(i);
        } else {
            self.selection.push(id);
        }
        self.anchor = Some(id);
        self.selected = Some(id);
    }

    /// A click on a cover or row as the design reads it (`libClick`): `Shift` takes the
    /// range from the last toggled game; `Shift`, `Ctrl` or an existing selection toggle.
    /// Returns `false` for a plain click with nothing selected (open or play instead).
    pub fn click(
        &mut self,
        lib: &LibrarySnapshot,
        visible: &[u32],
        id: GameId,
        shift: bool,
        ctrl: bool,
    ) -> bool {
        let pos = |g: GameId| visible.iter().position(|i| lib.games[*i as usize].id == g);
        if shift && let (Some(a), Some(b)) = (self.anchor.and_then(pos), pos(id)) {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            let mut seen: HashSet<GameId> = self.selection.iter().copied().collect();
            for i in &visible[lo..=hi] {
                let gid = lib.games[*i as usize].id;
                if seen.insert(gid) {
                    self.selection.push(gid);
                }
            }
            self.selected = Some(id);
            return true;
        }
        if shift || ctrl || !self.selection.is_empty() {
            self.toggle_select(id);
            return true;
        }
        false
    }

    /// `Shift+arrow`: select from the anchor (or the cursor) to `id` in visual order.
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

    /// `Ctrl+A`: everything shown.
    pub fn select_all(&mut self, lib: &LibrarySnapshot, visible: &[u32]) {
        self.selection = visible.iter().map(|i| lib.games[*i as usize].id).collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    fn name(lib: &LibrarySnapshot, i: u32) -> &str {
        &lib.games[i as usize].name
    }

    #[test]
    fn scopes_follow_the_design() {
        let lib = demo::library(demo::baseline_now());
        assert_eq!(count(&lib, Collection::All), 24);
        assert!(count(&lib, Collection::Favorites) > 0);
        assert!(count(&lib, Collection::Running) > 0);
        assert!(count(&lib, Collection::Missing) > 0);
        // Two weeks: played 12 days ago counts, 16 days ago does not.
        let recent = count(&lib, Collection::Recent);
        let mut vm = LibraryVm {
            collection: Collection::Recent,
            ..LibraryVm::default()
        };
        let v = vm.visible(&lib);
        assert_eq!(v.len(), recent);
        assert!(v.iter().any(|i| name(&lib, *i) == "Clockwork Garden"));
        assert!(!v.iter().any(|i| name(&lib, *i) == "Quiet Orbit"));
    }

    #[test]
    fn scopes_offered() {
        let lib = demo::library(demo::baseline_now());
        let mut vm = LibraryVm::default();
        let shown = vm.shown_scopes(&lib);
        assert_eq!(&shown[..4], &FIXED[..4]);
        // Something is missing: Installed is offered; nothing is hidden: Hidden is not.
        assert!(shown.contains(&Collection::Installed));
        assert!(!shown.contains(&Collection::Hidden));
        vm.set_collection(Collection::Hidden);
        assert!(vm.shown_scopes(&lib).contains(&Collection::Hidden));
    }

    #[test]
    fn a_new_scope_or_level_is_a_new_shelf() {
        let lib = demo::library(demo::baseline_now());
        let mut vm = LibraryVm::default();
        let all = vm.visible(&lib).len();
        vm.set_collection(Collection::Favorites);
        assert_eq!(vm.visible(&lib).len(), count(&lib, Collection::Favorites));
        vm.set_collection(Collection::All);
        assert_eq!(vm.visible(&lib).len(), all);
        vm.toggle_filter(FilterChip::Preset(PresetFilter::Standard));
        assert!(vm.visible(&lib).len() < all);
    }

    #[test]
    fn a_controller_cycles_through_the_offered_scopes() {
        let lib = demo::library(demo::baseline_now());
        let mut vm = LibraryVm::default();
        vm.cycle_scope(&lib, true);
        assert_eq!(vm.collection, Collection::Favorites);
        vm.cycle_scope(&lib, false);
        vm.cycle_scope(&lib, false);
        let last = vm.shown_scopes(&lib).last().copied();
        assert_eq!(Some(vm.collection), last);
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
        assert!(
            c.items
                .iter()
                .all(|i| name(&lib, *i).to_lowercase().contains("lantern"))
        );
        assert!(!c.items.is_empty());
        assert_eq!(c.pool, 24);
    }

    #[test]
    fn recently_played_puts_never_played_last_in_both_orders() {
        let lib = demo::library(demo::baseline_now());
        for desc in [true, false] {
            let mut vm = LibraryVm {
                descending: desc,
                ..LibraryVm::default()
            };
            let v = vm.visible(&lib);
            let played: Vec<bool> = v
                .iter()
                .map(|i| lib.games[*i as usize].last_played_at.is_some())
                .collect();
            let first_never = played.iter().position(|p| !p).unwrap_or(played.len());
            assert!(played[first_never..].iter().all(|p| !p));
        }
    }

    #[test]
    fn search_keeps_the_sort_order() {
        let lib = demo::library(demo::baseline_now());
        let mut vm = LibraryVm {
            query: "ar".into(),
            ..LibraryVm::default()
        };
        let v = vm.visible(&lib);
        let times: Vec<_> = v
            .iter()
            .filter_map(|i| lib.games[*i as usize].last_played_at)
            .collect();
        assert!(times.windows(2).all(|w| w[0] >= w[1]));
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
    fn protection_filter_counts_a_custom_game_under_its_level() {
        let lib = demo::library(demo::baseline_now());
        let mut vm = LibraryVm::default();
        vm.toggle_filter(FilterChip::Preset(PresetFilter::Offline));
        let v = vm.visible(&lib);
        assert!(v.iter().any(|i| name(&lib, *i) == "Quiet Orbit"));
        assert_eq!(
            v.len(),
            vm.filter_count(&lib, &FilterChip::Preset(PresetFilter::Offline))
        );
        assert!(vm.refined());
    }

    #[test]
    fn groups_once_each_alphabetical_no_tag_last() {
        let lib = demo::library(demo::baseline_now());
        let mut vm = LibraryVm {
            group: Group::Tag,
            ..LibraryVm::default()
        };
        let r = vm.result(&lib);
        assert_eq!(r.items.len(), 24);
        let labels: Vec<&str> = r.groups.iter().map(|g| &*g.label).collect();
        assert_eq!(labels.last(), Some(&strings::library::NO_TAG()));
        vm.group = Group::Protection;
        let r = vm.result(&lib);
        let labels: Vec<&str> = r.groups.iter().map(|g| &*g.label).collect();
        assert_eq!(labels, ["Custom", "Offline", "Online", "Standard"]);
        vm.group = Group::Played;
        let r = vm.result(&lib);
        assert_eq!(&*r.groups[0].label, strings::library::GROUP_NOT_PLAYED());
    }

    #[test]
    fn a_tag_collection_becomes_a_filter() {
        let lib = demo::library(demo::baseline_now());
        let mut vm = LibraryVm::default();
        vm.set_collection(Collection::Tag(TagId::new(1)));
        assert_eq!(vm.collection, Collection::All);
        assert_eq!(vm.filters.tags, vec![TagId::new(1)]);
        assert!(!vm.visible(&lib).is_empty());
    }

    #[test]
    fn clicks_select_as_the_design_does() {
        let lib = demo::library(demo::baseline_now());
        let mut vm = LibraryVm::default();
        let v = vm.visible(&lib);
        let id = |p: usize| lib.games[v[p] as usize].id;
        // A plain click with nothing selected opens.
        assert!(!vm.click(&lib, &v, id(0), false, false));
        // Ctrl toggles and anchors; a plain click then toggles too.
        assert!(vm.click(&lib, &v, id(2), false, true));
        assert!(vm.click(&lib, &v, id(4), false, false));
        assert_eq!(vm.selection, vec![id(2), id(4)]);
        // Shift takes the range from the last toggled.
        assert!(vm.click(&lib, &v, id(6), true, false));
        assert_eq!(vm.selection, vec![id(2), id(4), id(5), id(6)]);
        vm.select_all(&lib, &v);
        assert_eq!(vm.selection.len(), v.len());
    }

    #[test]
    fn escape_clears_search_then_selection() {
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
        assert!(vm.selection.is_empty());
        assert!(!vm.escape());
        // The filters stay: they have their own ×.
        assert!(vm.filters.is_active());
    }

    #[test]
    fn the_steam_scope_is_offered_while_it_holds_steam_games() {
        let now = demo::baseline_now();
        let lib = demo::library(now);
        let mut vm = LibraryVm::default();
        assert!(!vm.shown_scopes(&lib).contains(&Collection::Steam));
        // Counts are kept per revision: a new view over the other library.
        let mut vm = LibraryVm::default();
        let lib = demo::preview_library(now);
        let shown = vm.shown_scopes(&lib);
        assert!(shown.contains(&Collection::Steam));
        // After Installers, before Hidden.
        let at = |c| FIXED.iter().position(|x| *x == c);
        assert!(at(Collection::Installers) < at(Collection::Steam));
        assert!(at(Collection::Steam) < at(Collection::Hidden));
        assert_eq!(Collection::Steam.label(), strings::steam::SCOPE());
        vm.set_collection(Collection::Steam);
        let v = vm.visible(&lib);
        assert_eq!(v.len(), 3);
        assert!(v.iter().all(|i| lib.games[*i as usize].is_steam()));
        assert_eq!(vm.scope_count(&lib, Collection::Steam), 3);
    }

    #[test]
    fn protection_and_internet_filters_never_match_a_steam_game() {
        let lib = demo::preview_library(demo::baseline_now());
        let steam = demo::steam_samples(demo::baseline_now());
        for chip in [
            FilterChip::Preset(PresetFilter::Standard),
            FilterChip::Preset(PresetFilter::Offline),
            FilterChip::Preset(PresetFilter::Online),
            FilterChip::Network(NetworkAccess::Full),
            FilterChip::Network(NetworkAccess::None),
        ] {
            assert!(steam.iter().all(|g| !chip.matches(g)), "{chip:?}");
            let mut vm = LibraryVm::default();
            vm.toggle_filter(chip.clone());
            let v = vm.visible(&lib);
            assert!(v.iter().all(|i| !lib.games[*i as usize].is_steam()));
            assert_eq!(v.len(), vm.filter_count(&lib, &chip), "{chip:?}");
        }
    }

    #[test]
    fn steam_games_sort_and_group_last_under_run_by_steam() {
        let lib = demo::preview_library(demo::baseline_now());
        for desc in [true, false] {
            let mut vm = LibraryVm {
                sort: SortKey::Protection,
                descending: desc,
                ..LibraryVm::default()
            };
            let v = vm.visible(&lib);
            let steam: Vec<bool> = v
                .iter()
                .map(|i| lib.games[*i as usize].is_steam())
                .collect();
            let first = steam.iter().position(|x| *x).unwrap_or(steam.len());
            assert!(steam[first..].iter().all(|x| *x), "desc {desc}");
        }
        let mut vm = LibraryVm {
            group: Group::Protection,
            ..LibraryVm::default()
        };
        let r = vm.result(&lib);
        let labels: Vec<&str> = r.groups.iter().map(|g| &*g.label).collect();
        assert_eq!(labels.last(), Some(&strings::steam::RUN_BY_STEAM()));
        assert!(!labels.is_empty());
    }

    #[test]
    fn a_steam_game_with_time_but_no_date_is_not_unplayed() {
        let mut g = demo::steam_samples(demo::baseline_now())[0].clone();
        g.last_played_at = None;
        assert!(!not_played(&g));
        g.total_playtime_ms = 0;
        assert!(not_played(&g));
    }

    #[test]
    fn a_small_library_is_sparse_only_unrefined() {
        let mut vm = LibraryVm::default();
        assert!(vm.sparse(6));
        assert!(!vm.sparse(7));
        vm.query = "a".into();
        assert!(!vm.sparse(3));
        vm.query.clear();
        vm.view = ViewMode::List;
        assert!(!vm.sparse(3));
    }
}
