//! The library snapshot (GAME_LIBRARY §1.1): construction from repository rows, incremental
//! update of one game, activity and availability overlays (no database writes), the search
//! keys of §4, and the matcher index of GAME_IDENTITY §4 ([`ExeIndex`]).
//!
//! Every snapshot is immutable and published as an `Arc`; an update produces a new value
//! that shares every unchanged summary's `Arc<str>`/`Arc<Path>` with the previous one.
//! `build(rows)` and any sequence of [`LibrarySnapshot::with_game`] /
//! [`LibrarySnapshot::without_game`] reaching the same rows produce equal content
//! ([`LibrarySnapshot::same_content`]).

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use smallvec::SmallVec;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::clock::Timestamp;
use crate::identity::matcher::{IndexedExe, LibraryIndex};
use crate::ids::{ExecutableId, GameId, RuntimeName, TagId};
use crate::model::{
    Activity, ArtKey, ArtSlot, ArtworkRef, Availability, Channel, EntryKind, ExeRole, GameFlags,
    GameStats, GameSummary, NetworkAccess, Preset, RuntimeRef, SaveDataLocation, SaveLocationKind,
    Sha256, SummaryFlags, Tag, TagColor,
};
use crate::storage::{GameRecord, LibraryRows};

/// "Recently played" window (GAME_LIBRARY §6).
pub const RECENT_WINDOW: Duration = Duration::from_secs(30 * 24 * 3600);
/// "Recently played" size cap.
pub const RECENT_MAX: usize = 30;

// ---- Normalisation (§4) -------------------------------------------------------------------

/// Search normalisation of GAME_LIBRARY §4: NFKD, combining marks removed, lower case, `&` →
/// `and`, every run of non-alphanumeric characters → one space, trimmed.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    let push = |out: &mut String, c: char, pending: &mut bool| {
        if *pending && !out.is_empty() {
            out.push(' ');
        }
        *pending = false;
        out.push(c);
    };
    for c in s.nfkd().filter(|c| !is_combining_mark(*c)) {
        if c == '&' {
            pending_space = true;
            for a in "and".chars() {
                push(&mut out, a, &mut pending_space);
            }
            pending_space = true;
        } else if c.is_alphanumeric() {
            for l in c.to_lowercase() {
                push(&mut out, l, &mut pending_space);
            }
        } else {
            pending_space = true;
        }
    }
    out
}

const ROMAN: [&str; 9] = ["ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x"];

/// The other spelling of a numeral word: `vii` ↔ `7` for 2…10.
pub fn numeral_alternate(word: &str) -> Option<&'static str> {
    const DIGITS: [&str; 9] = ["2", "3", "4", "5", "6", "7", "8", "9", "10"];
    if let Some(i) = ROMAN.iter().position(|r| *r == word) {
        return Some(DIGITS[i]);
    }
    DIGITS.iter().position(|d| *d == word).map(|i| ROMAN[i])
}

/// Natural, locale-independent order: digit runs compare numerically ("Game 2" < "Game 10").
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.as_bytes(), b.as_bytes());
    loop {
        match (x.first(), y.first()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(p), Some(q)) if p.is_ascii_digit() && q.is_ascii_digit() => {
                let nx = x.iter().take_while(|c| c.is_ascii_digit()).count();
                let ny = y.iter().take_while(|c| c.is_ascii_digit()).count();
                let (dx, dy) = (&x[..nx], &y[..ny]);
                let tx = trim_zeros(dx);
                let ty = trim_zeros(dy);
                let o = tx.len().cmp(&ty.len()).then_with(|| tx.cmp(ty));
                if o != Ordering::Equal {
                    return o;
                }
                x = &x[nx..];
                y = &y[ny..];
            }
            (Some(_), Some(_)) => {
                // Compare one UTF-8 character at a time (byte order of UTF-8 is code-point
                // order, so comparing the encoded bytes of one character is enough).
                let lx = utf8_len(x[0]);
                let ly = utf8_len(y[0]);
                let o = x[..lx.min(x.len())].cmp(&y[..ly.min(y.len())]);
                if o != Ordering::Equal {
                    return o;
                }
                x = &x[lx.min(x.len())..];
                y = &y[ly.min(y.len())..];
            }
        }
    }
}

fn trim_zeros(d: &[u8]) -> &[u8] {
    let n = d.iter().take_while(|c| **c == b'0').count();
    &d[n.min(d.len().saturating_sub(1))..]
}

fn utf8_len(b: u8) -> usize {
    match b {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

/// Precomputed search keys of one game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchEntry {
    /// Normalised name.
    pub name: Box<str>,
    /// Words of the normalised name followed by numeral alternates.
    pub words: Box<[Box<str>]>,
    /// First letters of the name's words.
    pub acronym: Box<str>,
    /// Normalised primary executable file name.
    pub exe: Box<str>,
}

impl SearchEntry {
    /// Keys for a name and executable path.
    pub fn new(name: &str, exe: &Path) -> Self {
        let name = normalize(name);
        let mut words: Vec<Box<str>> = name
            .split(' ')
            .filter(|w| !w.is_empty())
            .map(Into::into)
            .collect();
        let acronym: String = words.iter().filter_map(|w| w.chars().next()).collect();
        let alternates: Vec<Box<str>> = words
            .iter()
            .filter_map(|w| numeral_alternate(w))
            .map(Into::into)
            .collect();
        words.extend(alternates);
        let exe = exe
            .file_name()
            .map(|n| normalize(&n.to_string_lossy()))
            .unwrap_or_default();
        Self {
            name: name.into(),
            words: words.into(),
            acronym: acronym.into(),
            exe: exe.into(),
        }
    }
}

/// The precomputed search keys of a snapshot (`entries[i]` belongs to `games[i]`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchIndex {
    /// Per game, parallel to [`LibrarySnapshot::games`]; shared between snapshots so an
    /// incremental update copies pointers, not strings (PERFORMANCE §1).
    pub entries: Vec<Arc<SearchEntry>>,
    /// Normalised words of each tag name.
    pub tags: HashMap<TagId, Box<[Box<str>]>>,
}

// ---- Snapshot ------------------------------------------------------------------------------

/// A tag as the sidebar and editors show it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagInfo {
    /// Identifier.
    pub id: TagId,
    /// Name.
    pub name: Arc<str>,
    /// Colour.
    pub color: TagColor,
    /// Games carrying it that are not hidden (the sidebar count).
    pub count: u32,
}

/// Counts per collection (GAME_LIBRARY §6) and the library's total playtime.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LibraryTotals {
    /// *All games*.
    pub all: u32,
    /// *Favourites*.
    pub favorites: u32,
    /// *Recently played*.
    pub recent: u32,
    /// *Never played*.
    pub never_played: u32,
    /// *Running*.
    pub running: u32,
    /// *Installed*.
    pub installed: u32,
    /// *Missing*.
    pub missing: u32,
    /// *Installers*.
    pub installers: u32,
    /// *Hidden*.
    pub hidden: u32,
    /// Σ total playtime over every game Zaun runs (hidden included): Zaun's own
    /// recorded sessions only.
    pub total_playtime_ms: u64,
    /// Σ Steam's play time over the Steam games listed (hidden included), shown apart: it
    /// comes from Steam, not from Zaun's sessions (STATISTICS §11).
    pub steam_playtime_ms: u64,
}

/// The only library data the UI has (GAME_LIBRARY §1.1).
#[derive(Clone, Debug)]
pub struct LibrarySnapshot {
    /// Increments on every publish.
    pub revision: u64,
    /// The "now" the time-dependent parts (recently played) were computed for.
    pub built_at: Timestamp,
    /// Sorted by `sort_name` in natural order, then ID.
    pub games: Vec<GameSummary>,
    /// Index into `games`.
    pub by_id: HashMap<GameId, u32>,
    /// Tags by name.
    pub tags: Vec<TagInfo>,
    /// Counts.
    pub totals: LibraryTotals,
    /// Search keys.
    pub search: SearchIndex,
}

/// Availability and activity, kept outside the database (LOGIC_SPECS §9, GAME_LIBRARY §1.1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Overlays {
    /// Last known availability per game (absent = `Unknown`).
    pub availability: HashMap<GameId, Availability>,
    /// Current activity per game (absent = `Idle`).
    pub activity: HashMap<GameId, Activity>,
    /// Dominant cover colour per game, from the thumbnail headers (absent = unknown).
    pub accent: HashMap<GameId, [u8; 3]>,
}

impl Overlays {
    /// Availability of a game.
    pub fn availability_of(&self, id: GameId) -> Availability {
        self.availability.get(&id).copied().unwrap_or_default()
    }

    /// Activity of a game.
    pub fn activity_of(&self, id: GameId) -> Activity {
        self.activity.get(&id).copied().unwrap_or_default()
    }

    /// Record availabilities.
    pub fn set_availability(&mut self, updates: &[(GameId, Availability)]) {
        for (id, a) in updates {
            self.availability.insert(*id, *a);
        }
    }

    /// Dominant cover colour of a game.
    pub fn accent_of(&self, id: GameId) -> Option<[u8; 3]> {
        self.accent.get(&id).copied()
    }

    /// Record dominant cover colours (`None` forgets one).
    pub fn set_accents(&mut self, updates: &[(GameId, Option<[u8; 3]>)]) {
        for (id, c) in updates {
            match c {
                Some(c) => {
                    self.accent.insert(*id, *c);
                }
                None => {
                    self.accent.remove(id);
                }
            }
        }
    }

    /// Record an activity (`Idle` removes the entry).
    pub fn set_activity(&mut self, id: GameId, activity: Activity) {
        if activity == Activity::Idle {
            self.activity.remove(&id);
        } else {
            self.activity.insert(id, activity);
        }
    }
}

/// What the index needs to know about compatibility layers (implemented over `runtime` by
/// the AppService; [`DefaultRuntimeInfo`] labels from the stored names alone).
pub trait RuntimeInfo: Send + Sync {
    /// The label of a game's compatibility layer, e.g. "GE-Proton 10-25" or
    /// "Default (UMU-Proton 10.0-4)".
    fn label(&self, runtime: &RuntimeRef, last_resolved: Option<&RuntimeName>) -> Arc<str>;
    /// Whether switching a prefix prepared with `prepared` to `new` goes back to an older
    /// build (CONFIGURATION §3 rule 8). Unknown → `false`.
    fn is_downgrade(&self, prepared: &RuntimeName, new: &RuntimeRef) -> bool {
        let _ = (prepared, new);
        false
    }
}

/// Labels from the reference and the last resolution only.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultRuntimeInfo;

impl RuntimeInfo for DefaultRuntimeInfo {
    fn label(&self, runtime: &RuntimeRef, last: Option<&RuntimeName>) -> Arc<str> {
        match (runtime, last) {
            (RuntimeRef::Default, Some(n)) => format!("Default ({})", n.as_str()).into(),
            (RuntimeRef::Default, None) => "Default".into(),
            (RuntimeRef::Channel(_), Some(n)) => n.as_str().into(),
            (RuntimeRef::Channel(Channel::UmuProton), None) => "UMU-Proton (newest)".into(),
            (RuntimeRef::Channel(Channel::GeProton), None) => "GE-Proton (newest)".into(),
            (RuntimeRef::Pinned(n), _) => n.as_str().into(),
            (RuntimeRef::External(p), _) => format!(
                "Custom: {}",
                p.file_name().map_or_else(
                    || p.display().to_string(),
                    |n| n.to_string_lossy().into_owned()
                )
            )
            .into(),
        }
    }
}

/// One game's rows, as joined by [`build`].
#[derive(Clone, Copy, Debug)]
pub struct GameRows<'a> {
    /// The game.
    pub record: &'a GameRecord,
    /// Its tags.
    pub tags: &'a [TagId],
    /// Its artwork rows.
    pub artwork: &'a [ArtworkRef],
    /// Its aggregates.
    pub stats: Option<&'a GameStats>,
}

fn art_key(art: &[ArtworkRef], slot: ArtSlot) -> Option<ArtKey> {
    art.iter().find(|a| a.slot == slot).map(|a| ArtKey(a.hash))
}

/// The summary of one game; `previous` lends its shared strings when they are unchanged.
pub fn summarize(
    rows: GameRows<'_>,
    runtime: &dyn RuntimeInfo,
    overlays: &Overlays,
    previous: Option<&GameSummary>,
) -> GameSummary {
    let r = rows.record;
    let share = |new: &str, old: Option<&Arc<str>>| -> Arc<str> {
        match old {
            Some(o) if &**o == new => o.clone(),
            _ => new.into(),
        }
    };
    let primary = r
        .executables
        .iter()
        .find(|e| e.role == ExeRole::Primary)
        .or_else(|| r.executables.first());
    let exe_path = primary.map_or_else(PathBuf::new, |e| e.path.clone());
    let primary_exe_path: Arc<Path> = match previous {
        Some(p) if *p.primary_exe_path == *exe_path => p.primary_exe_path.clone(),
        _ => exe_path.into(),
    };
    let root = match (previous.and_then(|p| p.root.as_ref()), &r.root) {
        (Some(old), Some(new)) if **old == **new => Some(old.clone()),
        (_, Some(new)) => Some(Arc::from(new.as_path())),
        (_, None) => None,
    };
    let (preset, network, save_location) = match &r.policy {
        Ok(p) => (
            p.preset,
            p.network,
            match p.save_data {
                SaveDataLocation::Internal => SaveLocationKind::Internal,
                SaveDataLocation::External { .. } => SaveLocationKind::External,
            },
        ),
        Err(_) => (
            Preset::Offline,
            NetworkAccess::None,
            SaveLocationKind::Internal,
        ),
    };
    let mut tags: SmallVec<[TagId; 4]> = rows.tags.iter().copied().collect();
    tags.sort_unstable();
    tags.dedup();
    let label = runtime.label(&r.runtime, r.runtime_last_resolved.as_ref());
    let runtime_label = share(&label, previous.map(|p| &p.runtime_label));
    let stats = rows.stats;
    GameSummary {
        id: r.id,
        name: share(&r.name, previous.map(|p| &p.name)),
        sort_name: share(&r.sort_name, previous.map(|p| &p.sort_name)),
        cover: art_key(rows.artwork, ArtSlot::Cover),
        icon: art_key(rows.artwork, ArtSlot::Icon),
        hero: art_key(rows.artwork, ArtSlot::Hero),
        steam_app_id: r.steam_app_id,
        accent: overlays.accent_of(r.id),
        favorite: r.favorite,
        hidden: r.hidden,
        tags,
        added_at: r.added_at,
        last_played_at: stats.and_then(|s| s.last_played_at),
        first_played_at: stats.and_then(|s| s.first_played_at),
        total_playtime_ms: stats.map_or(0, |s| s.total_ms),
        session_count: stats.map_or(0, |s| s.sessions),
        launch_count: stats.map_or(0, |s| s.sessions.saturating_add(s.failed_launches)),
        preset,
        network,
        runtime_label,
        primary_exe_path,
        root,
        save_location,
        kind: super::entry_kind(r.flags),
        availability: overlays.availability_of(r.id),
        activity: overlays.activity_of(r.id),
        flags: SummaryFlags {
            suspected_tamper: r.flags.contains(GameFlags::SUSPECTED_TAMPER),
            needs_attention: !r.is_readable() || primary.is_none(),
            has_warning: false,
        },
        origin: crate::model::GameOrigin::Zaun,
    }
}

fn base_order(a: &GameSummary, b: &GameSummary) -> Ordering {
    natural_cmp(&a.sort_name, &b.sort_name).then_with(|| a.id.cmp(&b.id))
}

fn is_missing(a: Availability) -> bool {
    matches!(
        a,
        Availability::Missing | Availability::DriveDisconnected | Availability::Unreadable
    )
}

/// Whether a summary counts as recently played at `now`.
pub fn played_recently(g: &GameSummary, now: Timestamp) -> bool {
    g.last_played_at
        .is_some_and(|t| t >= now.saturating_sub(RECENT_WINDOW))
}

fn totals_of(games: &[GameSummary], now: Timestamp) -> LibraryTotals {
    let mut t = LibraryTotals::default();
    let mut recent = 0usize;
    for g in games {
        if g.is_steam() {
            t.steam_playtime_ms = t.steam_playtime_ms.saturating_add(g.total_playtime_ms);
        } else {
            t.total_playtime_ms = t.total_playtime_ms.saturating_add(g.total_playtime_ms);
        }
        if g.activity != Activity::Idle {
            t.running += 1;
        }
        if is_missing(g.availability) {
            t.missing += 1;
        }
        if g.hidden {
            t.hidden += 1;
            continue;
        }
        t.all += 1;
        t.favorites += u32::from(g.favorite);
        t.never_played += u32::from(g.never_played() && g.kind == EntryKind::Game);
        t.installed += u32::from(g.availability == Availability::Present);
        t.installers += u32::from(g.kind == EntryKind::Installer);
        recent += usize::from(played_recently(g, now));
    }
    t.recent = recent.min(RECENT_MAX) as u32;
    t
}

fn tag_infos(base: &[TagInfo], games: &[GameSummary]) -> Vec<TagInfo> {
    let mut counts: HashMap<TagId, u32> = HashMap::new();
    for g in games.iter().filter(|g| !g.hidden) {
        for t in &g.tags {
            *counts.entry(*t).or_insert(0) += 1;
        }
    }
    let mut tags: Vec<TagInfo> = base
        .iter()
        .map(|t| TagInfo {
            count: counts.get(&t.id).copied().unwrap_or(0),
            ..t.clone()
        })
        .collect();
    tags.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    tags
}

fn tag_words(tags: &[TagInfo]) -> HashMap<TagId, Box<[Box<str>]>> {
    tags.iter()
        .map(|t| {
            let words: Vec<Box<str>> = normalize(&t.name)
                .split(' ')
                .filter(|w| !w.is_empty())
                .map(Into::into)
                .collect();
            (t.id, words.into())
        })
        .collect()
}

/// Tag rows as [`TagInfo`] (counts filled in by the snapshot).
pub fn tag_rows(tags: &[Tag]) -> Vec<TagInfo> {
    tags.iter()
        .map(|t| TagInfo {
            id: t.id,
            name: t.name.as_str().into(),
            color: t.color,
            count: 0,
        })
        .collect()
}

impl LibrarySnapshot {
    /// An empty library.
    pub fn empty(now: Timestamp) -> Self {
        Self {
            revision: 0,
            built_at: now,
            games: Vec::new(),
            by_id: HashMap::new(),
            tags: Vec::new(),
            totals: LibraryTotals::default(),
            search: SearchIndex::default(),
        }
    }

    fn assemble(
        revision: u64,
        now: Timestamp,
        mut pairs: Vec<(GameSummary, Arc<SearchEntry>)>,
        tags: &[TagInfo],
    ) -> Self {
        pairs.sort_by(|a, b| base_order(&a.0, &b.0));
        let (games, entries): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
        Self::finish(revision, now, games, entries, tags)
    }

    fn finish(
        revision: u64,
        now: Timestamp,
        games: Vec<GameSummary>,
        entries: Vec<Arc<SearchEntry>>,
        tags: &[TagInfo],
    ) -> Self {
        let by_id = games
            .iter()
            .enumerate()
            .map(|(i, g)| (g.id, i as u32))
            .collect();
        Self::finish_with(revision, now, games, entries, tags, by_id)
    }

    fn finish_with(
        revision: u64,
        now: Timestamp,
        games: Vec<GameSummary>,
        entries: Vec<Arc<SearchEntry>>,
        tags: &[TagInfo],
        by_id: HashMap<GameId, u32>,
    ) -> Self {
        debug_assert_eq!(by_id.len(), games.len());
        let tags = tag_infos(tags, &games);
        Self {
            revision,
            built_at: now,
            totals: totals_of(&games, now),
            search: SearchIndex {
                entries,
                tags: tag_words(&tags),
            },
            tags,
            by_id,
            games,
        }
    }

    /// A summary by ID.
    pub fn get(&self, id: GameId) -> Option<&GameSummary> {
        self.by_id.get(&id).map(|i| &self.games[*i as usize])
    }

    /// A tag by ID.
    pub fn tag(&self, id: TagId) -> Option<&TagInfo> {
        self.tags.iter().find(|t| t.id == id)
    }

    /// Everything but the revision is equal.
    pub fn same_content(&self, other: &LibrarySnapshot) -> bool {
        self.built_at == other.built_at
            && self.games == other.games
            && self.by_id == other.by_id
            && self.tags == other.tags
            && self.totals == other.totals
            && self.search == other.search
    }

    fn pairs(&self) -> Vec<(GameSummary, Arc<SearchEntry>)> {
        self.games
            .iter()
            .cloned()
            .zip(self.search.entries.iter().cloned())
            .collect()
    }

    fn base_tags(&self) -> Vec<TagInfo> {
        self.tags.clone()
    }

    /// Insert or replace one game (incremental update: other summaries are shared).
    pub fn with_game(&self, summary: GameSummary, now: Timestamp) -> Self {
        let entry = match self.get(summary.id) {
            Some(old)
                if old.name == summary.name && old.primary_exe_path == summary.primary_exe_path =>
            {
                self.search.entries[self.by_id[&summary.id] as usize].clone()
            }
            _ => Arc::new(SearchEntry::new(&summary.name, &summary.primary_exe_path)),
        };
        let mut games = self.games.clone();
        let mut entries = self.search.entries.clone();
        // The index map is patched rather than rebuilt: shifting positions is cheaper than
        // hashing every ID again (PERFORMANCE §1, incremental update ≤ 0.5 ms).
        let mut by_id = self.by_id.clone();
        if let Some(i) = by_id.remove(&summary.id) {
            games.remove(i as usize);
            entries.remove(i as usize);
            by_id.values_mut().filter(|v| **v > i).for_each(|v| *v -= 1);
        }
        let at = games.partition_point(|g| base_order(g, &summary) == Ordering::Less);
        by_id
            .values_mut()
            .filter(|v| **v >= at as u32)
            .for_each(|v| *v += 1);
        by_id.insert(summary.id, at as u32);
        games.insert(at, summary);
        entries.insert(at, entry);
        Self::finish_with(
            self.revision + 1,
            now,
            games,
            entries,
            &self.base_tags(),
            by_id,
        )
    }

    /// Remove one game.
    pub fn without_game(&self, id: GameId, now: Timestamp) -> Self {
        let mut games = self.games.clone();
        let mut entries = self.search.entries.clone();
        if let Some(i) = self.by_id.get(&id) {
            games.remove(*i as usize);
            entries.remove(*i as usize);
        }
        Self::finish(self.revision + 1, now, games, entries, &self.base_tags())
    }

    /// Replace the tag list (after a tag was created, renamed, recoloured or deleted).
    pub fn with_tags(&self, tags: Vec<TagInfo>, now: Timestamp) -> Self {
        let pairs = self.pairs();
        Self::assemble(self.revision + 1, now, pairs, &tags)
    }

    /// Replace every Steam game (ADR-0031) with `steam` in one rebuild; the other games and
    /// their search keys are shared.
    pub fn with_steam(&self, steam: Vec<GameSummary>, now: Timestamp) -> Self {
        let mut pairs: Vec<(GameSummary, Arc<SearchEntry>)> = self
            .pairs()
            .into_iter()
            .filter(|(g, _)| !g.is_steam())
            .collect();
        pairs.extend(steam.into_iter().map(|g| {
            let e = Arc::new(SearchEntry::new(&g.name, &g.primary_exe_path));
            (g, e)
        }));
        Self::assemble(self.revision + 1, now, pairs, &self.base_tags())
    }

    /// The Steam games of this snapshot.
    pub fn steam_games(&self) -> impl Iterator<Item = &GameSummary> {
        self.games.iter().filter(|g| g.is_steam())
    }

    /// Apply new overlays to every game (no database access).
    pub fn with_overlays(&self, overlays: &Overlays, now: Timestamp) -> Self {
        let games: Vec<GameSummary> = self
            .games
            .iter()
            .map(|g| GameSummary {
                availability: overlays.availability_of(g.id),
                activity: overlays.activity_of(g.id),
                accent: overlays.accent_of(g.id),
                ..g.clone()
            })
            .collect();
        Self::finish(
            self.revision + 1,
            now,
            games,
            self.search.entries.clone(),
            &self.base_tags(),
        )
    }
}

/// Build a snapshot from repository rows (start-up and after bulk changes).
pub fn build(
    rows: &LibraryRows,
    runtime: &dyn RuntimeInfo,
    overlays: &Overlays,
    revision: u64,
    now: Timestamp,
) -> LibrarySnapshot {
    let mut tags: HashMap<GameId, Vec<TagId>> = HashMap::new();
    for (g, t) in &rows.game_tags {
        tags.entry(*g).or_default().push(*t);
    }
    let mut art: HashMap<GameId, Vec<ArtworkRef>> = HashMap::new();
    for a in &rows.artwork {
        art.entry(a.game_id).or_default().push(a.clone());
    }
    let stats: HashMap<GameId, &GameStats> = rows.stats.iter().map(|s| (s.game_id, s)).collect();
    let pairs = rows
        .games
        .iter()
        .map(|r| {
            let s = summarize(
                GameRows {
                    record: r,
                    tags: tags.get(&r.id).map_or(&[], Vec::as_slice),
                    artwork: art.get(&r.id).map_or(&[], Vec::as_slice),
                    stats: stats.get(&r.id).copied(),
                },
                runtime,
                overlays,
                None,
            );
            let e = Arc::new(SearchEntry::new(&s.name, &s.primary_exe_path));
            (s, e)
        })
        .collect();
    LibrarySnapshot::assemble(revision, now, pairs, &tag_rows(&rows.tags))
}

// ---- Matcher index (GAME_IDENTITY §4, ADR-0008) ------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct GameEntry {
    exes: Vec<IndexedExe>,
    root: Option<PathBuf>,
    save_dir: Option<PathBuf>,
}

/// The executables of the library by canonical path and quick hash, and the game roots:
/// what `identity::matcher::resolve` and the cross-field checks need.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExeIndex {
    games: HashMap<GameId, GameEntry>,
    by_path: HashMap<PathBuf, IndexedExe>,
    by_hash: HashMap<Sha256, Vec<IndexedExe>>,
}

impl ExeIndex {
    /// Build from the library rows.
    pub fn from_rows(rows: &LibraryRows) -> Self {
        let played: HashMap<GameId, Option<Timestamp>> = rows
            .stats
            .iter()
            .map(|s| (s.game_id, s.last_played_at))
            .collect();
        let mut idx = Self::default();
        for r in &rows.games {
            idx.upsert_game(r, played.get(&r.id).copied().flatten());
        }
        idx
    }

    /// Insert or replace one game.
    pub fn upsert_game(&mut self, r: &GameRecord, last_played_at: Option<Timestamp>) {
        self.remove_game(r.id);
        let exes: Vec<IndexedExe> = r
            .executables
            .iter()
            .map(|e| IndexedExe {
                game: r.id,
                exe: e.id,
                canonical_path: e.canonical_path.clone(),
                mount_point: e.mount_point.clone(),
                fingerprint: e.fingerprint,
                pe: e.pe.clone(),
                last_played_at,
            })
            .collect();
        for e in &exes {
            self.by_path.insert(e.canonical_path.clone(), e.clone());
            self.by_hash
                .entry(e.fingerprint.quick_hash)
                .or_default()
                .push(e.clone());
        }
        let save_dir = match &r.policy {
            Ok(p) => match &p.save_data {
                SaveDataLocation::External { dir } => Some(dir.clone()),
                SaveDataLocation::Internal => None,
            },
            Err(_) => None,
        };
        self.games.insert(
            r.id,
            GameEntry {
                exes,
                root: r.root.clone(),
                save_dir,
            },
        );
    }

    /// Remove one game.
    pub fn remove_game(&mut self, id: GameId) {
        let Some(entry) = self.games.remove(&id) else {
            return;
        };
        for e in entry.exes {
            if self
                .by_path
                .get(&e.canonical_path)
                .is_some_and(|x| x.game == id)
            {
                self.by_path.remove(&e.canonical_path);
            }
            if let Some(v) = self.by_hash.get_mut(&e.fingerprint.quick_hash) {
                v.retain(|x| x.game != id);
                if v.is_empty() {
                    self.by_hash.remove(&e.fingerprint.quick_hash);
                }
            }
        }
    }

    /// The game owning an executable.
    pub fn game_of(&self, exe: ExecutableId) -> Option<GameId> {
        self.games
            .iter()
            .find(|(_, g)| g.exes.iter().any(|e| e.exe == exe))
            .map(|(id, _)| *id)
    }

    /// Roots of every game except `except`.
    pub fn other_roots(&self, except: Option<GameId>) -> Vec<PathBuf> {
        self.games
            .iter()
            .filter(|(id, _)| Some(**id) != except)
            .filter_map(|(_, g)| g.root.clone())
            .collect()
    }

    /// External save directories of every game except `except`.
    pub fn other_save_dirs(&self, except: Option<GameId>) -> Vec<PathBuf> {
        self.games
            .iter()
            .filter(|(id, _)| Some(**id) != except)
            .filter_map(|(_, g)| g.save_dir.clone())
            .collect()
    }

    /// Number of indexed games.
    pub fn len(&self) -> usize {
        self.games.len()
    }

    /// Whether no game is indexed.
    pub fn is_empty(&self) -> bool {
        self.games.is_empty()
    }
}

impl LibraryIndex for ExeIndex {
    fn by_path(&self, path: &Path) -> Option<IndexedExe> {
        self.by_path.get(path).cloned()
    }

    fn by_hash(&self, hash: &Sha256) -> Vec<IndexedExe> {
        self.by_hash.get(hash).cloned().unwrap_or_default()
    }

    fn roots(&self) -> Vec<(PathBuf, GameId)> {
        let mut v: Vec<(PathBuf, GameId)> = self
            .games
            .iter()
            .filter_map(|(id, g)| g.root.clone().map(|r| (r, *id)))
            .collect();
        v.sort();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalisation() {
        assert_eq!(normalize("Pokémon: Rock & Roll!"), "pokemon rock and roll");
        assert_eq!(normalize("  ＦＵＬＬ  width "), "full width");
        assert_eq!(normalize("Half-Life²"), "half life2");
        assert_eq!(normalize("---"), "");
    }

    #[test]
    fn natural_order() {
        assert_eq!(natural_cmp("game 2", "game 10"), Ordering::Less);
        assert_eq!(natural_cmp("game 010", "game 10"), Ordering::Equal);
        assert_eq!(natural_cmp("a", "b"), Ordering::Less);
        assert_eq!(natural_cmp("abc", "ab"), Ordering::Greater);
        assert_eq!(natural_cmp("é", "e"), Ordering::Greater);
    }

    #[test]
    fn numerals_both_ways() {
        let e = SearchEntry::new("Final Fantasy VII", Path::new("/g/ff7.exe"));
        assert!(e.words.iter().any(|w| &**w == "7"));
        assert_eq!(&*e.acronym, "ffv");
        let e = SearchEntry::new("Hitman 3", Path::new("/g/h.exe"));
        assert!(e.words.iter().any(|w| &**w == "iii"));
        assert_eq!(&*e.exe, "h exe");
    }
}
