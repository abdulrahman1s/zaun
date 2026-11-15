//! Artwork (LOGIC_SPECS §14, ADR-0018, ART-008 service part).
//!
//! Network and decoding run on the job pool; the AppService thread only stores the
//! normalised result (content-addressed file + row) and updates the thumbnail index. The
//! Steam chain runs after a game is added and when the user asks, and **only** with
//! `artwork.online = "steam"`; otherwise nothing is sent anywhere.

use std::sync::Arc;

use super::super::app_service::{AppService, Internal};
use super::super::event::Notice;
use super::super::event::QueryResult;
use crate::artwork::icon_extract::ICON_LIMITS;
use crate::artwork::import::{Normalised, normalise, store_and_commit};
use crate::artwork::steam::{
    self, Resolution, ResolveDeps, SteamAppIdSource, apply_app_id, cdn_candidates, fetch_slot,
    may_fetch,
};
use crate::artwork::{ArtSource, ImageLimits};
use crate::error::{Error, ErrorCode};
use crate::ids::{ExecutableId, GameId, QueryId};
use crate::jobs::{JobLabel, JobPriority};
use crate::model::{ArtOrigin, ArtSlot};
use crate::storage::{ArtworkRepo, GamePatch};

/// The artwork job's result.
pub(crate) struct ArtFetched {
    pub game: GameId,
    pub user_asked: bool,
    pub app_id: Option<(u32, SteamAppIdSource)>,
    pub images: Vec<(Normalised, ArtOrigin)>,
    pub error: Option<Error>,
}

impl AppService {
    /// Run the Steam chain for `id` (after add: `user_asked = false`, one attempt per slot).
    pub(crate) fn fetch_online_artwork(&mut self, id: GameId, user_asked: bool) {
        let prefs = self.prefs.artwork.clone();
        if !steam::online(&prefs) {
            if user_asked {
                self.fail(
                    Error::new(ErrorCode::IntNotAvailable)
                        .with("detail", "online artwork is turned off"),
                );
            }
            return;
        }
        let (Some(http), Some(codec)) = (self.deps.http.clone(), self.deps.codec.clone()) else {
            return;
        };
        let game = match self.lib.game(id) {
            Ok(g) => g,
            Err(e) => return self.fail(e),
        };
        let existing = self.lib.db_mut().artwork_of(id).unwrap_or_default();
        let app_id_file = Arc::clone(&self.deps.app_id_file);
        let tracker = Arc::clone(&self.art_tracker);
        let post = self.poster();
        let umu_db = super::runtime::CatalogUmuDb::new(
            Arc::clone(&self.deps.fs),
            Arc::clone(&self.deps.runtimes),
        );
        let _ = self.deps.jobs.submit(
            JobPriority::Background,
            JobLabel("steam-artwork"),
            move |_| {
                let deps = ResolveDeps {
                    app_id_file: &*app_id_file,
                    umu_db: &umu_db,
                    http: Some(&http),
                };
                let mut out = ArtFetched {
                    game: id,
                    user_asked,
                    app_id: None,
                    images: Vec::new(),
                    error: None,
                };
                let (app_id, source) = match steam::resolve_app_id(&prefs, &game, &deps) {
                    Resolution::Found { app_id, source } => (app_id, source),
                    Resolution::Candidates(_) | Resolution::NotFound => {
                        post(Internal::Art(Box::new(out)));
                        return;
                    }
                };
                out.app_id = Some((app_id, source));
                for slot in [ArtSlot::Cover, ArtSlot::Hero, ArtSlot::Icon] {
                    if cdn_candidates(app_id, slot).is_empty() {
                        continue;
                    }
                    let current = existing.iter().find(|a| a.slot == slot);
                    if !may_fetch(current, user_asked) {
                        continue;
                    }
                    if !tracker.begin(id, slot) && !user_asked {
                        continue;
                    }
                    match fetch_slot(&prefs, &http, &*codec, app_id, slot) {
                        steam::FetchOutcome::Fetched { bytes, .. } => {
                            match normalise(&*codec, slot, &bytes, &ImageLimits::DEFAULT) {
                                Ok(n) => out.images.push((n, ArtOrigin::SteamCdn)),
                                Err(e) => tracing::info!(error = %e, "steam artwork rejected"),
                            }
                        }
                        steam::FetchOutcome::Failed(e) => {
                            out.error = Some(
                                Error::new(ErrorCode::ArtNoConnection)
                                    .with("reason", e.to_string()),
                            );
                            break;
                        }
                        steam::FetchOutcome::NotAvailable | steam::FetchOutcome::Offline => {}
                    }
                }
                post(Internal::Art(Box::new(out)));
            },
        );
    }

    /// Import a user-chosen file (decode and normalise on the pool).
    pub(crate) fn import_artwork(&mut self, id: GameId, slot: ArtSlot, source: ArtSource) {
        match source {
            ArtSource::SteamCdn(_) => self.fetch_online_artwork(id, true),
            ArtSource::File(path) => {
                let Some(codec) = self.deps.codec.clone() else {
                    return self.fail(Error::not_available("artwork codec"));
                };
                let fs = Arc::clone(&self.deps.fs);
                let post = self.poster();
                let _ = self.deps.jobs.submit(
                    JobPriority::Interactive,
                    JobLabel("artwork-import"),
                    move |_| {
                        let mut out = ArtFetched {
                            game: id,
                            user_asked: true,
                            app_id: None,
                            images: Vec::new(),
                            error: None,
                        };
                        let limits = ImageLimits::DEFAULT;
                        match fs.read_follow(&path, limits.max_bytes) {
                            Ok(bytes) => match normalise(&*codec, slot, &bytes, &limits) {
                                Ok(n) => out.images.push((n, ArtOrigin::File)),
                                Err(e) => {
                                    out.error = Some(
                                        Error::new(ErrorCode::ArtUnreadable)
                                            .with("reason", e.to_string()),
                                    );
                                }
                            },
                            Err(e) => out.error = Some(e.into()),
                        }
                        post(Internal::Art(Box::new(out)));
                    },
                );
            }
            ArtSource::ExecutableIcon(exe) => self.import_program_icon(id, slot, exe),
            ArtSource::Generated => self.clear_artwork(id, slot),
        }
    }

    /// *Remove*: the slot falls back to the icon or the generated placeholder. The stored
    /// file stays until maintenance collects it.
    fn clear_artwork(&mut self, id: GameId, slot: ArtSlot) {
        let rows = self.lib.db_mut().artwork_of(id).unwrap_or_default();
        let Some(row) = rows.into_iter().find(|r| r.slot == slot) else {
            return;
        };
        if let Err(e) = self.lib.db_mut().delete_artwork(id, slot) {
            return self.fail(Error::new(e.code()).with_source(e));
        }
        self.deps.art_index.remove(&row.hash);
        self.index_game_art(id);
        if let Err(e) = self.lib.refresh_game(id) {
            tracing::debug!(error = %e, "refresh after artwork removal failed");
        }
        self.publish_library();
    }

    /// *Use program icon*: the largest icon embedded in one of the game's programs.
    fn import_program_icon(&mut self, id: GameId, slot: ArtSlot, exe: Option<ExecutableId>) {
        let Some(codec) = self.deps.codec.clone() else {
            return self.fail(Error::not_available("artwork codec"));
        };
        let path = match self.lib.game(id) {
            Ok(g) => g
                .executables
                .iter()
                .find(|e| Some(e.id) == exe)
                .or_else(|| g.primary())
                .map(|e| e.canonical_path.clone()),
            Err(e) => return self.fail(e),
        };
        let icons = Arc::clone(&self.deps.icons);
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("artwork-icon"),
            move |_| {
                let mut out = ArtFetched {
                    game: id,
                    user_asked: true,
                    app_id: None,
                    images: Vec::new(),
                    error: None,
                };
                let bytes = path.map(|p| icons.largest_icon(&p));
                match bytes {
                    Some(Ok(Some(b))) => match normalise(&*codec, slot, &b, &ICON_LIMITS) {
                        Ok(n) => out.images.push((n, ArtOrigin::ExecutableIcon)),
                        Err(e) => {
                            out.error = Some(
                                Error::new(ErrorCode::ArtUnreadable).with("reason", e.to_string()),
                            );
                        }
                    },
                    Some(Err(e)) => out.error = Some(e),
                    Some(Ok(None)) | None => {
                        out.error = Some(Error::new(ErrorCode::ArtNoIcon));
                    }
                }
                post(Internal::Art(Box::new(out)));
            },
        );
    }

    /// `FindSteamApp` (ADR-0024 §1): the ID the computer gives and store search results.
    pub(crate) fn find_steam_app(&mut self, id: GameId, term: Option<String>, reply_to: QueryId) {
        let game = match self.lib.game(id) {
            Ok(g) => g,
            Err(e) => {
                let describe = self.describer(Some(id));
                return self.answer_on_pool("find-steam-app", reply_to, move |_| {
                    QueryResult::Failed(Box::new(describe(&e)))
                });
            }
        };
        let prefs = self.prefs.artwork.clone();
        let http = self.deps.http.clone();
        let app_id_file = Arc::clone(&self.deps.app_id_file);
        let umu_db = super::runtime::CatalogUmuDb::new(
            Arc::clone(&self.deps.fs),
            Arc::clone(&self.deps.runtimes),
        );
        let describe = self.describer(Some(id));
        self.answer_on_pool("find-steam-app", reply_to, move |_| {
            let deps = ResolveDeps {
                app_id_file: &*app_id_file,
                umu_db: &umu_db,
                http: http.as_deref(),
            };
            let detected = match term {
                Some(_) => None,
                None => steam::local_app_id(&game, &deps),
            };
            let words = term.unwrap_or_else(|| game.name.clone());
            let candidates = match (&detected, http.as_deref()) {
                (None, Some(http)) => match steam::search(&prefs, http, &words) {
                    Ok(c) => steam::rank(&words, c),
                    Err(e) => {
                        let err =
                            Error::new(ErrorCode::ArtNoConnection).with("reason", e.to_string());
                        return QueryResult::Failed(Box::new(describe(&err)));
                    }
                },
                _ => Vec::new(),
            };
            QueryResult::SteamApps {
                detected,
                candidates,
            }
        });
    }

    pub(crate) fn on_art_fetched(&mut self, a: ArtFetched) {
        let id = a.game;
        if let Some((app_id, source)) = a.app_id
            && let Ok(mut game) = self.lib.game(id)
            && apply_app_id(&mut game, app_id, source)
        {
            let patch = GamePatch {
                steam_app_id: Some(game.steam_app_id),
                steam_app_id_is_user_set: Some(game.steam_app_id_is_user_set),
                ..GamePatch::default()
            };
            if let Err(e) = self.lib.update(id, patch) {
                tracing::info!(error = %e, "steam app id not stored");
            }
        }
        let now = self.deps.clock.now();
        let mut stored = 0;
        for (n, origin) in &a.images {
            // Re-check against the rows now: the user may have chosen a file meanwhile.
            let current = self
                .lib
                .db_mut()
                .artwork_of(id)
                .unwrap_or_default()
                .into_iter()
                .find(|r| r.slot == n.slot);
            if *origin == ArtOrigin::SteamCdn && !may_fetch(current.as_ref(), a.user_asked) {
                continue;
            }
            let fs = Arc::clone(&self.deps.fs);
            let paths = self.deps.paths.clone();
            match store_and_commit(&*fs, &paths, self.lib.db_mut(), id, n, *origin, now) {
                Ok(_) => stored += 1,
                Err(e) => tracing::warn!(error = %e, "artwork not stored"),
            }
        }
        if stored > 0 {
            self.index_game_art(id);
            if let Err(e) = self.lib.refresh_game(id) {
                tracing::debug!(error = %e, "refresh after artwork failed");
            }
            self.publish_library();
            if a.images.iter().any(|(_, o)| *o == ArtOrigin::SteamCdn) {
                self.notice(Notice::ArtworkFetched {
                    game: id,
                    slots: stored,
                });
            }
        }
        if !a.user_asked {
            return;
        }
        if let Some(e) = a.error {
            self.fail(e);
        } else if stored == 0 {
            // The window shows a spinner until the request ends one way or the other.
            self.fail(Error::new(ErrorCode::ArtNotFound));
        }
    }
}
