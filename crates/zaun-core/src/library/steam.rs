//! Steam games in the library (ADR-0031 §4, GAME_LIBRARY §15).
//!
//! The last scan (`steam::scan`) and the `steam_games` rows are merged into the snapshot as
//! summaries of origin [`GameOrigin::Steam`] with the ID [`GameId::steam`]. A scan replaces
//! every Steam summary at once: a game uninstalled in Steam disappears, its row stays, and
//! favourite, hidden, notes and tags return with it. Nothing here writes outside Zaun's
//! database.

use std::collections::HashMap;
use std::sync::Arc;

use super::index::{LibrarySnapshot, Overlays};
use super::limit;
use super::service::{Library, MAX_TAGS_PER_GAME, resolve_tags, storage};
use crate::clock::Timestamp;
use crate::error::{Error, ErrorCode, Result};
use crate::ids::{GameId, TagName};
use crate::model::{
    Activity, ArtKey, ArtSlot, Availability, EntryKind, GameOrigin, GameSummary, NetworkAccess,
    Preset, SaveLocationKind, SteamCompat, SteamFacts, SteamUpdate, SummaryFlags, sort_name_of,
    validate_notes,
};
use crate::steam::{SteamGame, SteamScan};
use crate::storage::{SteamOverlay, SteamPatch, SteamRepo};

/// The Steam part of the library: the last scan, `None` while the option is off.
#[derive(Clone, Debug, Default)]
pub struct SteamLayer {
    scan: Option<Arc<SteamScan>>,
}

/// The compatibility-layer label of a Steam game (the `layer:` search and list column).
pub fn layer_label(f: &SteamFacts) -> String {
    match &f.compat {
        SteamCompat::NotProton => "Steam (no Proton)".to_owned(),
        SteamCompat::Proton { tool: Some(t), .. } => format!("Steam: {t}"),
        SteamCompat::Proton { tool: None, .. } => "Steam: default Proton".to_owned(),
    }
}

/// `ZN-STM-002`: no Steam game with this app ID in the last scan.
pub(crate) fn not_listed(app_id: u32) -> Error {
    Error::new(ErrorCode::StmGameGone).with("app_id", app_id)
}

/// The summary of one Steam game.
pub fn steam_summary(
    g: &SteamGame,
    row: Option<&SteamOverlay>,
    overlays: &Overlays,
    now: Timestamp,
) -> GameSummary {
    let id = GameId::steam(g.facts.app_id);
    let art = |slot: ArtSlot| {
        g.art
            .iter()
            .find(|a| a.slot == slot)
            .map(|a| ArtKey(a.hash))
    };
    let install_dir: Arc<std::path::Path> = Arc::from(g.facts.install_dir.as_path());
    let availability = match overlays.availability.get(&id) {
        Some(a) => *a,
        None if g.present => Availability::Present,
        None => Availability::DriveDisconnected,
    };
    GameSummary {
        id,
        name: g.name.as_str().into(),
        sort_name: sort_name_of(&g.name).into(),
        cover: art(ArtSlot::Cover),
        icon: None,
        hero: art(ArtSlot::Hero),
        steam_app_id: Some(g.facts.app_id),
        accent: overlays.accent_of(id),
        favorite: row.is_some_and(|r| r.favorite),
        hidden: row.is_some_and(|r| r.hidden),
        tags: row
            .map(|r| r.tags.iter().copied().collect())
            .unwrap_or_default(),
        added_at: row.map_or(now, |r| r.first_seen_at),
        last_played_at: g.last_played,
        first_played_at: None,
        total_playtime_ms: g.facts.playtime_minutes.unwrap_or(0).saturating_mul(60_000),
        session_count: 0,
        launch_count: 0,
        // Placeholders: a Steam game has no preset (`GameSummary::protection`).
        preset: Preset::Standard,
        network: NetworkAccess::Full,
        runtime_label: layer_label(&g.facts).into(),
        primary_exe_path: install_dir.clone(),
        root: Some(install_dir),
        save_location: SaveLocationKind::Internal,
        kind: EntryKind::Game,
        availability,
        activity: overlays.activity_of(id),
        flags: SummaryFlags {
            suspected_tamper: false,
            needs_attention: g.facts.files_missing,
            has_warning: g.facts.update != SteamUpdate::UpToDate,
        },
        origin: GameOrigin::Steam(Arc::new(g.facts.clone())),
    }
}

/// Every summary of `scan` with its rows.
pub fn steam_summaries(
    scan: &SteamScan,
    rows: &[SteamOverlay],
    overlays: &Overlays,
    now: Timestamp,
) -> Vec<GameSummary> {
    let rows: HashMap<u32, &SteamOverlay> = rows.iter().map(|r| (r.app_id, r)).collect();
    scan.games
        .iter()
        .map(|g| steam_summary(g, rows.get(&g.facts.app_id).copied(), overlays, now))
        .collect()
}

impl SteamLayer {
    /// The last scan (`None` while the option is off).
    pub fn scan(&self) -> Option<&Arc<SteamScan>> {
        self.scan.as_ref()
    }
}

impl Library {
    /// The last Steam scan (`None` while the option is off).
    pub fn steam_scan(&self) -> Option<Arc<SteamScan>> {
        self.steam.scan().cloned()
    }

    /// A Steam game of the last scan.
    pub fn steam_game(&self, app_id: u32) -> Result<SteamGame> {
        self.steam
            .scan()
            .and_then(|s| s.game(app_id).cloned())
            .ok_or_else(|| not_listed(app_id))
    }

    /// Install a new scan (`None`: the option was turned off; every Steam game leaves the
    /// library, its row stays). Rows are created for games seen for the first time.
    pub fn set_steam(&mut self, scan: Option<Arc<SteamScan>>) -> Result<()> {
        if let Some(s) = &scan {
            let ids: Vec<u32> = s.games.iter().map(|g| g.facts.app_id).collect();
            let now = self.deps.clock.now();
            self.db.steam_seen(&ids, now).map_err(storage)?;
            // Availability is the scan's (the overlay is re-derived from it).
            let updates: Vec<(GameId, Availability)> = s
                .games
                .iter()
                .map(|g| {
                    let a = if g.present {
                        Availability::Present
                    } else {
                        Availability::DriveDisconnected
                    };
                    (GameId::steam(g.facts.app_id), a)
                })
                .collect();
            self.overlays.set_availability(&updates);
        }
        self.steam.scan = scan;
        self.refresh_steam()
    }

    /// Rebuild every Steam summary from the last scan and the rows.
    pub fn refresh_steam(&mut self) -> Result<()> {
        let now = self.deps.clock.now();
        let summaries = self.steam_summaries_now(now)?;
        let s = self.snapshot.with_steam(summaries, now);
        self.publish(s);
        Ok(())
    }

    pub(super) fn steam_summaries_now(&mut self, now: Timestamp) -> Result<Vec<GameSummary>> {
        let Some(scan) = self.steam.scan.clone() else {
            return Ok(Vec::new());
        };
        let rows = self.db.steam_overlays().map_err(storage)?;
        Ok(steam_summaries(&scan, &rows, &self.overlays, now))
    }

    /// The row of a Steam game (defaults when it has none yet).
    pub fn steam_row(&mut self, app_id: u32) -> Result<SteamOverlay> {
        let rows = self.db.steam_overlays().map_err(storage)?;
        Ok(rows
            .into_iter()
            .find(|r| r.app_id == app_id)
            .unwrap_or_else(|| SteamOverlay {
                app_id,
                first_seen_at: self.deps.clock.now(),
                updated_at: self.deps.clock.now(),
                favorite: false,
                hidden: false,
                notes: String::new(),
                tags: Vec::new(),
            }))
    }

    fn steam_patch(&mut self, app_id: u32, patch: &SteamPatch) -> Result<()> {
        self.steam_game(app_id)?;
        let now = self.deps.clock.now();
        self.db.steam_update(app_id, patch, now).map_err(storage)?;
        self.refresh_steam()
    }

    /// Favourite on or off.
    pub fn steam_set_favorite(&mut self, app_id: u32, favorite: bool) -> Result<()> {
        self.steam_patch(
            app_id,
            &SteamPatch {
                favorite: Some(favorite),
                ..SteamPatch::default()
            },
        )
    }

    /// Hidden on or off.
    pub fn steam_set_hidden(&mut self, app_id: u32, hidden: bool) -> Result<()> {
        self.steam_patch(
            app_id,
            &SteamPatch {
                hidden: Some(hidden),
                ..SteamPatch::default()
            },
        )
    }

    /// Notes (at most 4000 characters).
    pub fn steam_set_notes(&mut self, app_id: u32, notes: &str) -> Result<()> {
        validate_notes(notes)?;
        self.steam_patch(
            app_id,
            &SteamPatch {
                notes: Some(notes.to_owned()),
                ..SteamPatch::default()
            },
        )
    }

    /// Replace the tags (unknown names are created; at most 16).
    pub fn steam_set_tags(&mut self, app_id: u32, tags: &[TagName]) -> Result<()> {
        if tags.len() > MAX_TAGS_PER_GAME {
            return Err(limit("game.tags", MAX_TAGS_PER_GAME));
        }
        self.steam_game(app_id)?;
        let now = self.deps.clock.now();
        let tags = tags.to_vec();
        self.db.transaction(|tx| -> Result<()> {
            let ids = resolve_tags(tx, &tags, now)?;
            tx.steam_set_tags(app_id, &ids, now).map_err(storage)
        })?;
        self.refresh_tags()?;
        self.refresh_steam()
    }

    /// Mark a Steam game running or idle (no database write).
    pub fn steam_set_running(&mut self, running: &[(GameId, Activity)]) {
        for (id, a) in running {
            self.overlays.set_activity(*id, *a);
        }
        let s = self
            .snapshot
            .with_overlays(&self.overlays, self.deps.clock.now());
        self.publish(s);
    }
}

/// Refuse an edit that has no meaning for a Steam game (`ZN-STM-007`).
pub(crate) fn not_for_steam(what: &'static str) -> Error {
    Error::new(ErrorCode::StmNotForSteam).with("action", what)
}

/// Whether a snapshot lists `id` as a Steam game.
pub fn is_listed(snapshot: &LibrarySnapshot, id: GameId) -> bool {
    snapshot.get(id).is_some_and(GameSummary::is_steam)
}
