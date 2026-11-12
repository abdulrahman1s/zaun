//! Collections, filters, search, sorting and grouping over a [`LibrarySnapshot`]
//! (GAME_LIBRARY §4–6). Pure; runs on the UI thread, results cached by
//! `(snapshot.revision, query)`.

use std::cmp::Ordering;
use std::sync::Arc;

use super::index::{LibrarySnapshot, SearchEntry, natural_cmp, normalize, played_recently};
use crate::ids::TagId;
use crate::model::{
    Activity, Availability, BasePreset, EntryKind, GameSummary, NetworkAccess, Preset,
};

/// A sidebar collection (the base filter).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Collection {
    /// `!hidden`.
    #[default]
    All,
    /// `favorite && !hidden`.
    Favorites,
    /// Last played within 30 days, top 30 by recency, `!hidden`.
    Recent,
    /// `session_count == 0 && kind == Game && !hidden`.
    NeverPlayed,
    /// `activity != Idle`.
    Running,
    /// `availability == Present && !hidden`.
    Installed,
    /// Missing, disconnected or unreadable.
    Missing,
    /// `kind == Installer && !hidden`.
    Installers,
    /// `hidden`.
    Hidden,
    /// Has the tag, `!hidden`.
    Tag(TagId),
}

/// Protection as a filter value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PresetFilter {
    /// Exactly *Offline*.
    Offline,
    /// Exactly *Online*.
    Online,
    /// Exactly *Standard*.
    Standard,
    /// Any custom policy.
    Custom,
}

impl PresetFilter {
    /// The filter value of a preset.
    pub fn of(p: Preset) -> Self {
        match p {
            Preset::Offline => PresetFilter::Offline,
            Preset::Online => PresetFilter::Online,
            Preset::Standard => PresetFilter::Standard,
            Preset::Custom { .. } => PresetFilter::Custom,
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "offline" => Some(PresetFilter::Offline),
            "online" => Some(PresetFilter::Online),
            "standard" => Some(PresetFilter::Standard),
            "custom" => Some(PresetFilter::Custom),
            _ => None,
        }
    }
}

/// Filter chips: AND across groups, OR within a group; an empty group does not filter.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Filters {
    /// Tags.
    pub tags: Vec<TagId>,
    /// Protection.
    pub presets: Vec<PresetFilter>,
    /// Network.
    pub network: Option<NetworkAccess>,
    /// Availability.
    pub availability: Vec<Availability>,
    /// Compatibility layer labels (exact).
    pub layers: Vec<Arc<str>>,
    /// Kinds.
    pub kinds: Vec<EntryKind>,
}

impl Filters {
    /// Whether any chip is active.
    pub fn is_active(&self) -> bool {
        !(self.tags.is_empty()
            && self.presets.is_empty()
            && self.network.is_none()
            && self.availability.is_empty()
            && self.layers.is_empty()
            && self.kinds.is_empty())
    }

    fn matches(&self, g: &GameSummary) -> bool {
        (self.tags.is_empty() || self.tags.iter().any(|t| g.tags.contains(t)))
            && (self.presets.is_empty() || self.presets.contains(&PresetFilter::of(g.preset)))
            && self.network.is_none_or(|n| n == g.network)
            && (self.availability.is_empty() || self.availability.contains(&g.availability))
            && (self.layers.is_empty() || self.layers.iter().any(|l| **l == *g.runtime_label))
            && (self.kinds.is_empty() || self.kinds.contains(&g.kind))
    }
}

/// Sort keys (§5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SortKey {
    /// `sort_name`, natural order.
    #[default]
    Name,
    /// Last played; never-played games last.
    LastPlayed,
    /// Total playtime.
    Playtime,
    /// Date added.
    DateAdded,
    /// Launches.
    Launches,
    /// Offline → Online → Standard → Custom.
    Protection,
    /// Compatibility-layer label.
    Layer,
}

impl SortKey {
    /// The default direction: `true` = descending (newest/most first).
    pub const fn default_descending(self) -> bool {
        matches!(
            self,
            SortKey::LastPlayed | SortKey::Playtime | SortKey::DateAdded | SortKey::Launches
        )
    }
}

/// Grouping (§5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Grouping {
    /// No groups.
    #[default]
    None,
    /// First letter of the sort name (`#` for anything else).
    FirstLetter,
    /// One group per tag; a game appears under each of its tags ("No tags" last).
    Tag,
    /// By protection preset.
    Protection,
}

/// What a group is about.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GroupKey {
    /// A letter (upper case) or `#`.
    Letter(char),
    /// A tag; `None` = games without tags.
    Tag(Option<TagId>),
    /// A protection value.
    Protection(PresetFilter),
}

/// A run of `items` under one header.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GroupSpan {
    /// What the group is.
    pub key: GroupKey,
    /// Header text (tag name, letter, preset name).
    pub label: Arc<str>,
    /// First index into [`QueryResult::items`].
    pub start: u32,
    /// Number of items.
    pub len: u32,
}

/// A library query.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct LibraryQuery {
    /// Base filter.
    pub collection: Collection,
    /// Search text with operators.
    pub text: String,
    /// Filter chips.
    pub filters: Filters,
    /// Sort key.
    pub sort: SortKey,
    /// Direction (see [`SortKey::default_descending`]).
    pub descending: bool,
    /// Grouping.
    pub group: Grouping,
    /// "Search all": ignore the collection while the text is non-empty (`Ctrl+Shift+F`).
    pub search_all: bool,
}

/// The result: indices into `snapshot.games` in display order (repeated under several tag
/// groups when grouping by tag).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QueryResult {
    /// Indices into the snapshot's games.
    pub items: Vec<u32>,
    /// Group headers (empty without grouping).
    pub groups: Vec<GroupSpan>,
    /// Snapshot revision the indices refer to.
    pub revision: u64,
}

// ---- Search text ---------------------------------------------------------------------------

/// `is:` flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IsFlag {
    /// `is:favorite`.
    Favorite,
    /// `is:running`.
    Running,
    /// `is:missing`.
    Missing,
    /// `is:hidden`.
    Hidden,
    /// `is:installer`.
    Installer,
    /// `is:offline`.
    Offline,
    /// `is:online`.
    Online,
    /// `is:never-played`.
    NeverPlayed,
}

impl IsFlag {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "favorite" | "favourite" | "fav" => IsFlag::Favorite,
            "running" => IsFlag::Running,
            "missing" => IsFlag::Missing,
            "hidden" => IsFlag::Hidden,
            "installer" => IsFlag::Installer,
            "offline" => IsFlag::Offline,
            "online" => IsFlag::Online,
            "never-played" | "neverplayed" | "unplayed" => IsFlag::NeverPlayed,
            _ => return None,
        })
    }

    fn matches(self, g: &GameSummary) -> bool {
        match self {
            IsFlag::Favorite => g.favorite,
            IsFlag::Running => g.activity != Activity::Idle,
            IsFlag::Missing => matches!(
                g.availability,
                Availability::Missing | Availability::DriveDisconnected | Availability::Unreadable
            ),
            IsFlag::Hidden => g.hidden,
            IsFlag::Installer => g.kind == EntryKind::Installer,
            IsFlag::Offline => g.network == NetworkAccess::None,
            IsFlag::Online => g.network == NetworkAccess::Full,
            IsFlag::NeverPlayed => g.session_count == 0,
        }
    }
}

/// One parsed term of the search text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TermKind {
    /// Free text: normalised tokens, each must match a field.
    Text(Vec<String>),
    /// `tag:<name>` (normalised; prefix match on the tag's words).
    Tag(String),
    /// `is:<flag>`.
    Is(IsFlag),
    /// `preset:<value>`.
    Preset(PresetFilter),
    /// `layer:<text>` (normalised substring of the label).
    Layer(String),
}

/// A term, possibly negated with a leading `-`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Term {
    /// Exclude matches instead of requiring them.
    pub negated: bool,
    /// What it matches.
    pub kind: TermKind,
}

/// Split the search text into terms. Quotes group words (`tag:"open world"`); unknown
/// operators are treated as text.
pub fn parse_search(text: &str) -> Vec<Term> {
    let mut raw: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    raw.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        raw.push(cur);
    }
    let mut terms = Vec::new();
    for r in raw {
        let (negated, body) = match r.strip_prefix('-') {
            Some(rest) if !rest.is_empty() => (true, rest),
            _ => (false, r.as_str()),
        };
        let lower = body.to_lowercase();
        let kind = if let Some(v) = lower.strip_prefix("tag:") {
            TermKind::Tag(normalize(v))
        } else if let Some(f) = lower.strip_prefix("is:").and_then(IsFlag::parse) {
            TermKind::Is(f)
        } else if let Some(p) = lower.strip_prefix("preset:").and_then(PresetFilter::parse) {
            TermKind::Preset(p)
        } else if let Some(v) = lower.strip_prefix("layer:") {
            TermKind::Layer(normalize(v))
        } else {
            let tokens: Vec<String> = normalize(body)
                .split(' ')
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
                .collect();
            if tokens.is_empty() {
                continue;
            }
            TermKind::Text(tokens)
        };
        terms.push(Term { negated, kind });
    }
    terms
}

/// Rank of one token against one game (0 = no match), §4 weights.
fn token_rank(snapshot: &LibrarySnapshot, g: &GameSummary, e: &SearchEntry, token: &str) -> u32 {
    let mut best = 0;
    if e.words.iter().any(|w| w.starts_with(token)) {
        best = 100;
    } else if token.chars().count() >= 2 && e.acronym.starts_with(token) {
        best = 80;
    } else if e.name.contains(token) {
        best = 60;
    }
    if best == 0
        && g.tags.iter().any(|t| {
            snapshot
                .search
                .tags
                .get(t)
                .is_some_and(|ws| ws.iter().any(|w| w.starts_with(token)))
        })
    {
        best = 40;
    }
    if best == 0 && e.exe.contains(token) {
        best = 20;
    }
    best
}

fn tag_matches(snapshot: &LibrarySnapshot, g: &GameSummary, name: &str) -> bool {
    if name.is_empty() {
        return !g.tags.is_empty();
    }
    let want: Vec<&str> = name.split(' ').collect();
    g.tags.iter().any(|t| {
        snapshot.search.tags.get(t).is_some_and(|ws| {
            // The words of the operator are a prefix of the tag's words, the last one as a
            // word prefix ("tag:open wo" matches "Open World").
            want.len() <= ws.len()
                && want.iter().enumerate().all(|(i, w)| {
                    if i + 1 == want.len() {
                        ws[i].starts_with(w)
                    } else {
                        &*ws[i] == *w
                    }
                })
        })
    })
}

/// Whether a term matches, with its rank for text terms.
fn term_rank(snapshot: &LibrarySnapshot, i: usize, t: &TermKind) -> Option<u32> {
    let g = &snapshot.games[i];
    let e = &snapshot.search.entries[i];
    match t {
        TermKind::Text(tokens) => {
            let mut sum = 0;
            for tok in tokens {
                let r = token_rank(snapshot, g, e, tok);
                if r == 0 {
                    return None;
                }
                sum += r;
            }
            Some(sum)
        }
        TermKind::Tag(name) => tag_matches(snapshot, g, name).then_some(0),
        TermKind::Is(f) => f.matches(g).then_some(0),
        TermKind::Preset(p) => (PresetFilter::of(g.preset) == *p).then_some(0),
        TermKind::Layer(text) => normalize(&g.runtime_label)
            .contains(text.as_str())
            .then_some(0),
    }
}

fn in_collection(snapshot: &LibrarySnapshot, g: &GameSummary, c: Collection) -> bool {
    match c {
        Collection::All => !g.hidden,
        Collection::Favorites => g.favorite && !g.hidden,
        Collection::Recent => !g.hidden && played_recently(g, snapshot.built_at),
        Collection::NeverPlayed => g.session_count == 0 && g.kind == EntryKind::Game && !g.hidden,
        Collection::Running => g.activity != Activity::Idle,
        Collection::Installed => g.availability == Availability::Present && !g.hidden,
        Collection::Missing => IsFlag::Missing.matches(g),
        Collection::Installers => g.kind == EntryKind::Installer && !g.hidden,
        Collection::Hidden => g.hidden,
        Collection::Tag(t) => g.tags.contains(&t) && !g.hidden,
    }
}

fn preset_rank(p: Preset) -> u8 {
    match p {
        Preset::Offline => 0,
        Preset::Online => 1,
        Preset::Standard => 2,
        Preset::Custom { base } => {
            3 + match base {
                BasePreset::Offline => 0,
                BasePreset::Online => 1,
                BasePreset::Standard => 2,
            }
        }
    }
}

fn tie(a: &GameSummary, b: &GameSummary) -> Ordering {
    natural_cmp(&a.sort_name, &b.sort_name).then_with(|| a.id.cmp(&b.id))
}

/// Order by `key` in direction `desc`; ties by `sort_name` ascending, then ID.
fn compare(a: &GameSummary, b: &GameSummary, key: SortKey, desc: bool) -> Ordering {
    let dir = |o: Ordering| if desc { o.reverse() } else { o };
    let primary = match key {
        SortKey::Name => dir(natural_cmp(&a.sort_name, &b.sort_name)),
        SortKey::LastPlayed => match (a.last_played_at, b.last_played_at) {
            (Some(x), Some(y)) => dir(x.cmp(&y)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        },
        SortKey::Playtime => dir(a.total_playtime_ms.cmp(&b.total_playtime_ms)),
        SortKey::DateAdded => dir(a.added_at.cmp(&b.added_at)),
        SortKey::Launches => dir(a.launch_count.cmp(&b.launch_count)),
        SortKey::Protection => dir(preset_rank(a.preset).cmp(&preset_rank(b.preset))),
        SortKey::Layer => dir(natural_cmp(
            &a.runtime_label.to_lowercase(),
            &b.runtime_label.to_lowercase(),
        )),
    };
    primary.then_with(|| tie(a, b))
}

fn letter_of(g: &GameSummary) -> char {
    normalize(&g.sort_name)
        .chars()
        .next()
        .filter(char::is_ascii_alphabetic)
        .map_or('#', |c| c.to_ascii_uppercase())
}

fn preset_label(p: PresetFilter) -> &'static str {
    match p {
        PresetFilter::Offline => "Offline",
        PresetFilter::Online => "Online",
        PresetFilter::Standard => "Standard",
        PresetFilter::Custom => "Custom",
    }
}

/// Answer a query.
pub fn run_query(snapshot: &LibrarySnapshot, q: &LibraryQuery) -> QueryResult {
    let terms = parse_search(&q.text);
    let use_collection = !(q.search_all && !terms.is_empty());
    // (index, rank)
    let mut hits: Vec<(u32, u32)> = Vec::new();
    'games: for (i, g) in snapshot.games.iter().enumerate() {
        if use_collection && !in_collection(snapshot, g, q.collection) {
            continue;
        }
        if !q.filters.matches(g) {
            continue;
        }
        let mut rank = 0;
        for t in &terms {
            match (term_rank(snapshot, i, &t.kind), t.negated) {
                (Some(r), false) => rank += r,
                (None, true) => {}
                _ => continue 'games,
            }
        }
        hits.push((i as u32, rank));
    }
    let games = &snapshot.games;
    let by_sort = |a: &(u32, u32), b: &(u32, u32)| {
        b.1.cmp(&a.1).then_with(|| {
            compare(
                &games[a.0 as usize],
                &games[b.0 as usize],
                q.sort,
                q.descending,
            )
        })
    };
    hits.sort_by(by_sort);
    if use_collection && q.collection == Collection::Recent {
        // Top 30 by recency, then shown in the active order.
        let mut recent = hits.clone();
        recent.sort_by(|a, b| {
            compare(
                &games[a.0 as usize],
                &games[b.0 as usize],
                SortKey::LastPlayed,
                true,
            )
        });
        recent.truncate(super::index::RECENT_MAX);
        let keep: std::collections::HashSet<u32> = recent.iter().map(|h| h.0).collect();
        hits.retain(|h| keep.contains(&h.0));
    }
    let (items, groups) = match q.group {
        Grouping::None => (hits.iter().map(|h| h.0).collect(), Vec::new()),
        Grouping::FirstLetter => group_by(&hits, |i| {
            let c = letter_of(&games[i as usize]);
            vec![(GroupKey::Letter(c), Arc::from(c.to_string()))]
        }),
        Grouping::Protection => group_by(&hits, |i| {
            let p = PresetFilter::of(games[i as usize].preset);
            vec![(GroupKey::Protection(p), Arc::from(preset_label(p)))]
        }),
        Grouping::Tag => group_by(&hits, |i| {
            let g = &games[i as usize];
            if g.tags.is_empty() {
                return vec![(GroupKey::Tag(None), Arc::from("No tags"))];
            }
            g.tags
                .iter()
                .filter_map(|t| snapshot.tag(*t))
                .map(|t| (GroupKey::Tag(Some(t.id)), t.name.clone()))
                .collect()
        }),
    };
    QueryResult {
        items,
        groups,
        revision: snapshot.revision,
    }
}

fn group_order(a: &(GroupKey, Arc<str>), b: &(GroupKey, Arc<str>)) -> Ordering {
    match (&a.0, &b.0) {
        (GroupKey::Letter(x), GroupKey::Letter(y)) => {
            // '#' after the letters.
            (*x == '#').cmp(&(*y == '#')).then(x.cmp(y))
        }
        (GroupKey::Protection(x), GroupKey::Protection(y)) => x.cmp(y),
        (GroupKey::Tag(x), GroupKey::Tag(y)) => x
            .is_none()
            .cmp(&y.is_none())
            .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()).then(x.cmp(y))),
        _ => Ordering::Equal,
    }
}

/// A group's key and label.
type GroupHeader = (GroupKey, Arc<str>);

/// Stable grouping: groups in their natural order, items keep their relative order.
fn group_by(
    hits: &[(u32, u32)],
    keys: impl Fn(u32) -> Vec<(GroupKey, Arc<str>)>,
) -> (Vec<u32>, Vec<GroupSpan>) {
    let mut buckets: Vec<(GroupHeader, Vec<u32>)> = Vec::new();
    for (i, _) in hits {
        for k in keys(*i) {
            match buckets.iter_mut().find(|(bk, _)| bk.0 == k.0) {
                Some((_, v)) => v.push(*i),
                None => buckets.push((k, vec![*i])),
            }
        }
    }
    buckets.sort_by(|a, b| group_order(&a.0, &b.0));
    let mut items = Vec::new();
    let mut groups = Vec::new();
    for ((key, label), v) in buckets {
        groups.push(GroupSpan {
            key,
            label,
            start: items.len() as u32,
            len: v.len() as u32,
        });
        items.extend(v);
    }
    (items, groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing() {
        let t = parse_search(r#"final -tag:"open world" is:favorite preset:custom layer:GE x"#);
        assert_eq!(t.len(), 6);
        assert_eq!(t[0].kind, TermKind::Text(vec!["final".into()]));
        assert!(t[1].negated);
        assert_eq!(t[1].kind, TermKind::Tag("open world".into()));
        assert_eq!(t[2].kind, TermKind::Is(IsFlag::Favorite));
        assert_eq!(t[3].kind, TermKind::Preset(PresetFilter::Custom));
        assert_eq!(t[4].kind, TermKind::Layer("ge".into()));
        assert_eq!(
            parse_search("is:bogus")[0].kind,
            TermKind::Text(vec!["is".into(), "bogus".into()])
        );
        assert!(parse_search("  - ").iter().all(|t| !t.negated));
    }
}
