//! Steam app ID resolution and CDN artwork (ADR-0018 §3–7, tasks ART-007 / ART-008).
//!
//! Nothing here touches the network while `artwork.online = "off"`: every function that
//! could send a request checks the preference first and returns before using the client.
//! The local steps of the ID chain (stored ID, `steam_appid.txt`, umu-database) need no
//! network and always run.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Deserialize;

use super::codec::{ImageCodec, ImageLimits};
use super::http::{HttpClient, HttpError};
use super::import::{ImportDeps, ImportInput, Imported, import};
use crate::clock::Timestamp;
use crate::config::schema::{ArtworkOnline, ArtworkPrefs};
use crate::consts::{STEAM_CDN_HOST, STEAM_STORE_HOST};
use crate::error::Result;
use crate::fsx::Fs;
use crate::ids::GameId;
use crate::model::{ArtOrigin, ArtSlot, ArtworkRef, Game};
use crate::storage::ArtworkRepo;

/// Body cap of a store search response.
pub const SEARCH_MAX_BODY: u64 = 512 * 1024;
/// Candidates kept from a search.
pub const SEARCH_MAX_CANDIDATES: usize = 10;
/// Longest candidate name kept, in characters.
pub const CANDIDATE_NAME_MAX_CHARS: usize = 120;

/// Where an app ID came from (ADR-0018 §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SteamAppIdSource {
    /// Already stored on the game.
    Stored,
    /// A file in the game folder or beside its program (`steam_appid.txt`, an emulator's
    /// settings, the Steam library's app manifest).
    AppIdFile,
    /// The umu-database entry of the game (store `steam`).
    UmuDb,
    /// A single exact-name store search hit.
    NameSearch,
    /// Typed or pasted by the user.
    User,
}

/// Step 3.2: reads `steam_appid.txt` in a game folder (implemented by `identity`).
pub trait AppIdFile: Send + Sync {
    /// The app ID in `<game_root>/steam_appid.txt`, if valid.
    fn read_app_id(&self, game_root: &Path) -> Option<u32>;
}

impl<F: Fn(&Path) -> Option<u32> + Send + Sync> AppIdFile for F {
    fn read_app_id(&self, game_root: &Path) -> Option<u32> {
        self(game_root)
    }
}

/// Step 3.3: the Steam app ID of the umu-database entry matched for the game, when that
/// entry's store is `steam` (implemented over `runtime::umu_db`).
pub trait UmuDbSteamId: Send + Sync {
    /// The app ID, if the game has a `steam` entry.
    fn steam_app_id(&self, game: &Game) -> Option<u32>;
}

impl<F: Fn(&Game) -> Option<u32> + Send + Sync> UmuDbSteamId for F {
    fn steam_app_id(&self, game: &Game) -> Option<u32> {
        self(game)
    }
}

/// The real step 3.2: `identity::hints::find_steam_appid` through [`Fs`] (`steam_appid.txt`,
/// a Steam API replacement's settings, the Steam library's app manifest; regular files only).
#[derive(Clone)]
pub struct FsAppIdFile {
    fs: Arc<dyn Fs>,
}

impl FsAppIdFile {
    /// A reader over `fs`.
    pub fn new(fs: Arc<dyn Fs>) -> Self {
        Self { fs }
    }
}

impl std::fmt::Debug for FsAppIdFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FsAppIdFile")
    }
}

impl AppIdFile for FsAppIdFile {
    fn read_app_id(&self, game_root: &Path) -> Option<u32> {
        crate::identity::hints::find_steam_appid(&*self.fs, game_root)
    }
}

/// Step 3.3 without a database: never finds an ID (tests). The service uses
/// `service::flows::runtime::CatalogUmuDb` over `runtime::umu_db`.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoUmuDb;

impl UmuDbSteamId for NoUmuDb {
    fn steam_app_id(&self, _game: &Game) -> Option<u32> {
        None
    }
}

/// A store search result offered to the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchCandidate {
    /// App ID.
    pub app_id: u32,
    /// Sanitised display name.
    pub name: String,
}

/// The outcome of the ID chain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// An app ID was found.
    Found {
        /// App ID.
        app_id: u32,
        /// Which step found it.
        source: SteamAppIdSource,
    },
    /// The search was ambiguous: offer these in the artwork dialog; nothing is stored.
    Candidates(Vec<SearchCandidate>),
    /// Nothing found (or the search was not allowed / failed).
    NotFound,
}

/// Dependencies of [`resolve_app_id`].
pub struct ResolveDeps<'a> {
    /// `steam_appid.txt` reader.
    pub app_id_file: &'a dyn AppIdFile,
    /// umu-database lookup.
    pub umu_db: &'a dyn UmuDbSteamId,
    /// The client; `None` = no network at all.
    pub http: Option<&'a HttpClient>,
}

/// Whether the preferences allow any request.
pub fn online(prefs: &ArtworkPrefs) -> bool {
    prefs.online == ArtworkOnline::Steam
}

/// Steps 1–3 of the chain, which need no network: the stored ID, a file in the game folder
/// or beside its program, the umu-database.
pub fn local_app_id(game: &Game, deps: &ResolveDeps<'_>) -> Option<(u32, SteamAppIdSource)> {
    if let Some(id) = game.steam_app_id.filter(|id| *id != 0) {
        return Some((id, SteamAppIdSource::Stored));
    }
    let exe_dir = game
        .primary()
        .and_then(|e| e.canonical_path.parent())
        .filter(|d| Some(*d) != game.root.as_deref());
    if let Some(id) = game
        .root
        .as_deref()
        .into_iter()
        .chain(exe_dir)
        .find_map(|d| deps.app_id_file.read_app_id(d))
        .filter(|id| *id != 0)
    {
        return Some((id, SteamAppIdSource::AppIdFile));
    }
    deps.umu_db
        .steam_app_id(game)
        .filter(|id| *id != 0)
        .map(|id| (id, SteamAppIdSource::UmuDb))
}

/// Steps 1–4 of the chain (step 5 is [`parse_user_app_id`] in the artwork dialog).
pub fn resolve_app_id(prefs: &ArtworkPrefs, game: &Game, deps: &ResolveDeps<'_>) -> Resolution {
    if let Some((app_id, source)) = local_app_id(game, deps) {
        return Resolution::Found { app_id, source };
    }
    if !(online(prefs) && prefs.name_search) {
        return Resolution::NotFound;
    }
    let Some(http) = deps.http else {
        return Resolution::NotFound;
    };
    match search(prefs, http, &game.name) {
        Ok(c) => decide_search(&game.name, c),
        Err(e) => {
            tracing::info!(error = %e, "steam store search failed");
            Resolution::NotFound
        }
    }
}

/// Search the Steam store for `term` (ADR-0018 §3.4; the artwork sheet's search, ADR-0024
/// §1). Only with `artwork.online = "steam"` and `artwork.name_search`; the results are in
/// the store's order, at most [`SEARCH_MAX_CANDIDATES`].
pub fn search(
    prefs: &ArtworkPrefs,
    http: &HttpClient,
    term: &str,
) -> std::result::Result<Vec<SearchCandidate>, HttpError> {
    if !(online(prefs) && prefs.name_search) || term.trim().is_empty() {
        return Ok(Vec::new());
    }
    let r = http.get_capped(&search_url(term), SEARCH_MAX_BODY)?;
    if r.status != 200 {
        return Err(HttpError::Transport(format!("status {}", r.status)));
    }
    parse_search(&r.body).ok_or_else(|| HttpError::Transport("malformed search answer".into()))
}

/// `candidates` ordered by how closely their names match `name`: equal names first, then
/// names that contain it or are contained in it, then by the words they share; the store's
/// order breaks ties.
pub fn rank(name: &str, mut candidates: Vec<SearchCandidate>) -> Vec<SearchCandidate> {
    let want = normalise_name(name);
    let words: HashSet<String> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let score = |c: &SearchCandidate| {
        let have = normalise_name(&c.name);
        let tier = if have == want {
            0
        } else if !want.is_empty() && (have.contains(&want) || want.contains(&have)) {
            1
        } else {
            2
        };
        let shared = c
            .name
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| words.contains(&w.to_lowercase()))
            .count();
        (tier, std::cmp::Reverse(shared))
    };
    candidates.sort_by_key(score);
    candidates
}

/// A single candidate whose normalised name equals the game's is accepted; anything else
/// (none, or several equal names, or only near matches) becomes a candidate list.
pub fn decide_search(game_name: &str, candidates: Vec<SearchCandidate>) -> Resolution {
    let want = normalise_name(game_name);
    let exact: Vec<&SearchCandidate> = candidates
        .iter()
        .filter(|c| !want.is_empty() && normalise_name(&c.name) == want)
        .collect();
    match exact.as_slice() {
        [one] => Resolution::Found {
            app_id: one.app_id,
            source: SteamAppIdSource::NameSearch,
        },
        _ if candidates.is_empty() => Resolution::NotFound,
        _ => Resolution::Candidates(rank(game_name, candidates)),
    }
}

/// Record a found ID on the game. A user-set ID is never replaced automatically; an ID from
/// [`SteamAppIdSource::User`] always replaces and becomes user-set. Returns whether the game
/// changed.
pub fn apply_app_id(game: &mut Game, app_id: u32, source: SteamAppIdSource) -> bool {
    if app_id == 0 {
        return false;
    }
    match source {
        SteamAppIdSource::User => {
            let changed = game.steam_app_id != Some(app_id) || !game.steam_app_id_is_user_set;
            game.steam_app_id = Some(app_id);
            game.steam_app_id_is_user_set = true;
            changed
        }
        SteamAppIdSource::Stored => false,
        _ if game.steam_app_id_is_user_set => false,
        _ => {
            let changed = game.steam_app_id != Some(app_id);
            game.steam_app_id = Some(app_id);
            changed
        }
    }
}

/// Step 5: an app ID typed by the user, or a pasted store URL
/// (`[https://][www.]store.steampowered.com/app/<id>[/…]`).
pub fn parse_user_app_id(text: &str) -> Option<u32> {
    let t = text.trim();
    let digits = |s: &str| -> Option<u32> {
        (!s.is_empty() && s.len() <= 10 && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse::<u32>().ok())
            .flatten()
            .filter(|id| *id != 0)
    };
    if let Some(id) = digits(t) {
        return Some(id);
    }
    let lower = t.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(&lower);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let rest = rest.strip_prefix(STEAM_STORE_HOST)?.strip_prefix("/app/")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    digits(&rest[..end])
}

/// Lower-case alphanumerics only; trademark signs and punctuation dropped; `&` = `and`.
pub fn normalise_name(name: &str) -> String {
    name.replace('&', " and ")
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The store search address for a display name (ADR-0018 §3.4).
pub fn search_url(name: &str) -> String {
    format!(
        "https://{STEAM_STORE_HOST}/api/storesearch/?term={}&l=english&cc=US",
        percent_encode(name.trim())
    )
}

#[derive(Deserialize)]
struct SearchBody {
    #[serde(default)]
    items: Vec<serde_json::Value>,
}

fn sanitise(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_control() && !matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
        .take(CANDIDATE_NAME_MAX_CHARS)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Parse a store search body: only numeric IDs and short sanitised names of `app` items are
/// kept, at most [`SEARCH_MAX_CANDIDATES`]. `None` for malformed JSON.
pub fn parse_search(body: &[u8]) -> Option<Vec<SearchCandidate>> {
    if body.len() as u64 > SEARCH_MAX_BODY {
        return None;
    }
    let parsed: SearchBody = serde_json::from_slice(body).ok()?;
    Some(
        parsed
            .items
            .into_iter()
            .filter(|i| i.get("type").is_none_or(|k| k == "app"))
            .filter_map(|i| {
                let app_id = u32::try_from(i.get("id")?.as_u64()?)
                    .ok()
                    .filter(|v| *v != 0)?;
                let name = sanitise(i.get("name")?.as_str()?);
                (!name.is_empty()).then_some(SearchCandidate { app_id, name })
            })
            .take(SEARCH_MAX_CANDIDATES)
            .collect(),
    )
}

/// The CDN addresses tried for a slot, in order (ADR-0018 §4). Icons: none.
pub fn cdn_candidates(app_id: u32, slot: ArtSlot) -> Vec<String> {
    let files: &[&str] = match slot {
        ArtSlot::Cover => &["library_600x900_2x.jpg", "library_600x900.jpg"],
        ArtSlot::Hero => &["library_hero.jpg", "header.jpg"],
        ArtSlot::Icon => &[],
    };
    files
        .iter()
        .map(|f| format!("https://{STEAM_CDN_HOST}/steam/apps/{app_id}/{f}"))
        .collect()
}

/// The result of fetching one slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchOutcome {
    /// An image that passed the codec's probe.
    Fetched {
        /// Body.
        bytes: Vec<u8>,
        /// Address that served it.
        url: String,
    },
    /// Every address answered 404/410 or served something that is not an image.
    NotAvailable,
    /// `artwork.online = "off"`: nothing was sent.
    Offline,
    /// A transport failure (timeout, refused redirect, oversize…).
    Failed(HttpError),
}

/// Try the addresses of `slot` in order: a 404 (or a non-image body) moves to the next;
/// any other failure stops. Only bodies that pass [`ImageCodec::probe`] are returned.
pub fn fetch_slot(
    prefs: &ArtworkPrefs,
    http: &HttpClient,
    codec: &dyn ImageCodec,
    app_id: u32,
    slot: ArtSlot,
) -> FetchOutcome {
    if !online(prefs) {
        return FetchOutcome::Offline;
    }
    let limits = ImageLimits {
        max_bytes: super::http::MAX_BODY,
        ..ImageLimits::DEFAULT
    };
    let mut last_err = None;
    for url in cdn_candidates(app_id, slot) {
        match http.get(&url) {
            Ok(r) if r.status == 200 => match codec.probe(&r.body, &limits) {
                Ok(_) => {
                    return FetchOutcome::Fetched {
                        bytes: r.body,
                        url: r.url.to_string(),
                    };
                }
                Err(e) => tracing::info!(%url, error = %e, "steam artwork is not an image"),
            },
            Ok(r) if matches!(r.status, 404 | 410) => {}
            Ok(r) => {
                tracing::info!(%url, status = r.status, "steam artwork unavailable");
                last_err = Some(HttpError::Transport(format!("status {}", r.status)));
            }
            Err(e) => return FetchOutcome::Failed(e),
        }
    }
    last_err.map_or(FetchOutcome::NotAvailable, FetchOutcome::Failed)
}

/// Remembers which `(game, slot)` pairs were tried in this process ("one attempt per game
/// and slot unless the user asks"). The service persists the set if it wants it to survive
/// restarts; a successful fetch needs no record (the stored row is the record).
#[derive(Default)]
pub struct AttemptTracker {
    tried: Mutex<HashSet<(GameId, ArtSlot)>>,
}

impl AttemptTracker {
    /// Empty.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an attempt; `false` if it had already been tried.
    pub fn begin(&self, game: GameId, slot: ArtSlot) -> bool {
        self.tried.lock().insert((game, slot))
    }

    /// Forget a game's attempts ("Reset artwork" re-runs the chain).
    pub fn reset(&self, game: GameId) {
        self.tried.lock().retain(|(g, _)| *g != game);
    }

    /// Whether `(game, slot)` was tried.
    pub fn was_tried(&self, game: GameId, slot: ArtSlot) -> bool {
        self.tried.lock().contains(&(game, slot))
    }
}

/// Whether an automatic fetch may replace `existing`: only empty slots and generated or
/// executable-icon art. A user-chosen file always wins; Steam art is never re-fetched.
/// With `user_asked`, everything but a user file may be replaced.
pub fn may_fetch(existing: Option<&ArtworkRef>, user_asked: bool) -> bool {
    match existing.map(|a| a.source) {
        None | Some(ArtOrigin::Generated | ArtOrigin::ExecutableIcon) => true,
        Some(ArtOrigin::SteamCdn) => user_asked,
        Some(ArtOrigin::File) => false,
    }
}

/// What happened to one slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlotOutcome {
    /// Stored with `source = steam_cdn`.
    Imported(Imported),
    /// Not attempted: offline, the slot has no CDN art (icons), the slot holds art that wins,
    /// or it was already tried.
    Skipped,
    /// The CDN has nothing for it (fall back to icon and placeholder, no error toast).
    NotAvailable,
    /// The request failed; the previous artwork stays.
    Failed(HttpError),
}

/// Fetch and import one slot for `game` with a known `app_id` (ADR-0018 §4–7). Blocking:
/// run on the job pool at `Background` priority.
#[allow(clippy::too_many_arguments)]
pub fn fetch_and_import<R: ArtworkRepo + ?Sized>(
    prefs: &ArtworkPrefs,
    http: &HttpClient,
    deps: &ImportDeps<'_>,
    repo: &mut R,
    tracker: &AttemptTracker,
    game: GameId,
    app_id: u32,
    slot: ArtSlot,
    user_asked: bool,
    now: Timestamp,
) -> Result<SlotOutcome> {
    if !online(prefs) || cdn_candidates(app_id, slot).is_empty() {
        return Ok(SlotOutcome::Skipped);
    }
    let existing = repo
        .artwork_of(game)
        .map_err(super::store::storage_err)?
        .into_iter()
        .find(|a| a.slot == slot);
    if !may_fetch(existing.as_ref(), user_asked) {
        return Ok(SlotOutcome::Skipped);
    }
    let first = tracker.begin(game, slot);
    if !first && !user_asked {
        return Ok(SlotOutcome::Skipped);
    }
    match fetch_slot(prefs, http, deps.codec, app_id, slot) {
        FetchOutcome::Fetched { bytes, .. } => {
            let imported = import(
                deps,
                repo,
                game,
                slot,
                ImportInput::Bytes {
                    bytes: &bytes,
                    origin: ArtOrigin::SteamCdn,
                },
                now,
            )?;
            Ok(SlotOutcome::Imported(imported))
        }
        FetchOutcome::NotAvailable => Ok(SlotOutcome::NotAvailable),
        FetchOutcome::Offline => Ok(SlotOutcome::Skipped),
        FetchOutcome::Failed(e) => Ok(SlotOutcome::Failed(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_app_id_parser() {
        let ok = [
            ("70", 70),
            ("  1145360 \n", 1_145_360),
            (
                "https://store.steampowered.com/app/2246340/Monster_Hunter_Wilds/",
                2_246_340,
            ),
            ("store.steampowered.com/app/70", 70),
            ("http://www.store.steampowered.com/app/70/?l=german", 70),
            ("HTTPS://STORE.STEAMPOWERED.COM/APP/70/HALF_LIFE/", 70),
            ("https://store.steampowered.com/app/70#reviews", 70),
        ];
        for (t, id) in ok {
            assert_eq!(parse_user_app_id(t), Some(id), "{t}");
        }
        for t in [
            "",
            "0",
            "abc",
            "-70",
            "99999999999",
            "4294967296",
            "https://store.steampowered.com/sub/70/",
            "https://store.steampowered.com.evil.example/app/70/",
            "https://evil.example/store.steampowered.com/app/70",
            "https://store.steampowered.com/app/",
            "https://store.steampowered.com/app/7x0/",
        ] {
            assert_eq!(parse_user_app_id(t), None, "{t}");
        }
    }

    #[test]
    fn name_normalisation() {
        assert_eq!(normalise_name("Half-Life™"), "halflife");
        assert_eq!(normalise_name("HALF-LIFE"), "halflife");
        assert_eq!(
            normalise_name("Ori & the Blind Forest"),
            normalise_name("Ori and the Blind Forest")
        );
        assert_eq!(normalise_name("Hades II"), "hadesii");
    }

    #[test]
    fn search_parsing_is_defensive() {
        let body = br#"{"total":3,"items":[
            {"type":"app","name":"Half-Life","id":70,"price":{"final":999}},
            {"type":"app","name":"Half-Life 2\u0007\u202e","id":220},
            {"type":"sub","name":"Bundle","id":1},
            {"type":"app","name":"Bad id","id":-5},
            {"type":"app","name":"Huge id","id":99999999999},
            {"type":"app","id":12},
            {"type":"app","name":"   ","id":13}
        ]}"#;
        let c = parse_search(body).unwrap();
        assert_eq!(
            c,
            vec![
                SearchCandidate {
                    app_id: 70,
                    name: "Half-Life".into()
                },
                SearchCandidate {
                    app_id: 220,
                    name: "Half-Life 2".into()
                },
            ]
        );
        assert_eq!(parse_search(b"<html>"), None);
        assert_eq!(parse_search(br#"{"total":0}"#), Some(vec![]));
        let long = format!(r#"{{"items":[{{"name":"{}","id":5}}]}}"#, "x".repeat(500));
        assert_eq!(
            parse_search(long.as_bytes()).unwrap()[0]
                .name
                .chars()
                .count(),
            120
        );
    }

    #[test]
    fn search_decision() {
        let c = |id, n: &str| SearchCandidate {
            app_id: id,
            name: n.into(),
        };
        assert_eq!(
            decide_search("Half-Life", vec![c(70, "Half-Life"), c(220, "Half-Life 2")]),
            Resolution::Found {
                app_id: 70,
                source: SteamAppIdSource::NameSearch
            }
        );
        let two = vec![c(1, "Doom"), c(2, "DOOM")];
        assert_eq!(
            decide_search("Doom", two.clone()),
            Resolution::Candidates(two)
        );
        let near = vec![c(3, "Portal 2")];
        assert_eq!(
            decide_search("Portal", near.clone()),
            Resolution::Candidates(near)
        );
        assert_eq!(decide_search("Portal", vec![]), Resolution::NotFound);
    }

    #[test]
    fn candidates_are_ranked_by_their_names() {
        let c = |id, n: &str| SearchCandidate {
            app_id: id,
            name: n.into(),
        };
        let ranked = rank(
            "Grand Theft Auto San Andreas",
            vec![
                c(1, "Grand Theft Auto V"),
                c(2, "Grand Theft Auto: San Andreas – The Definitive Edition"),
                c(3, "San Andreas Tractor Simulator"),
                c(4, "Grand Theft Auto: San Andreas"),
            ],
        );
        let ids: Vec<u32> = ranked.iter().map(|c| c.app_id).collect();
        assert_eq!(ids, vec![4, 2, 1, 3]);
    }

    #[test]
    fn search_url_encodes_the_name() {
        assert_eq!(
            search_url(" Ori & the Blind Forest "),
            "https://store.steampowered.com/api/storesearch/?term=Ori%20%26%20the%20Blind%20Forest&l=english&cc=US"
        );
    }

    #[test]
    fn cdn_addresses() {
        assert_eq!(
            cdn_candidates(70, ArtSlot::Cover),
            vec![
                "https://cdn.cloudflare.steamstatic.com/steam/apps/70/library_600x900_2x.jpg",
                "https://cdn.cloudflare.steamstatic.com/steam/apps/70/library_600x900.jpg",
            ]
        );
        assert_eq!(cdn_candidates(70, ArtSlot::Hero).len(), 2);
        assert!(cdn_candidates(70, ArtSlot::Icon).is_empty());
    }

    #[test]
    fn replacement_rules() {
        use crate::clock::Timestamp;
        let row = |source| ArtworkRef {
            game_id: GameId::from_uuid(uuid::Uuid::from_u128(1)),
            slot: ArtSlot::Cover,
            file: "cover-0000000000000000.png".into(),
            hash: [0; 32],
            width: 1,
            height: 1,
            source,
            updated_at: Timestamp::from_millis(0),
        };
        assert!(may_fetch(None, false));
        assert!(may_fetch(Some(&row(ArtOrigin::Generated)), false));
        assert!(may_fetch(Some(&row(ArtOrigin::ExecutableIcon)), false));
        assert!(!may_fetch(Some(&row(ArtOrigin::SteamCdn)), false));
        assert!(may_fetch(Some(&row(ArtOrigin::SteamCdn)), true));
        assert!(!may_fetch(Some(&row(ArtOrigin::File)), false));
        assert!(!may_fetch(Some(&row(ArtOrigin::File)), true));
    }
}
