//! An artwork job finished: store its images, save the Steam app ID, tell the user (LOGIC_SPECS §14).
use crate::artwork::import::{Normalised, store_and_commit};
use crate::artwork::steam::{SteamAppIdSource, apply_app_id, may_fetch};
use crate::model::ArtOrigin;
use crate::service::prelude::*;
use crate::storage::{ArtworkRepo, GamePatch};

/// The artwork job's result.
pub(crate) struct ArtFetched {
    pub game: GameId,
    pub user_asked: bool,
    pub app_id: Option<(u32, SteamAppIdSource)>,
    pub images: Vec<(Normalised, ArtOrigin)>,
    pub error: Option<Error>,
}

impl ArtFetched {
    pub(super) fn new(game: GameId, user_asked: bool) -> Self {
        Self {
            game,
            user_asked,
            app_id: None,
            images: Vec::new(),
            error: None,
        }
    }
}

impl AppService {
    pub(crate) fn on_art_fetched(&mut self, a: ArtFetched) -> Result<()> {
        let id = a.game;
        if let Some((app_id, source)) = a.app_id {
            self.store_app_id(id, app_id, source);
        }
        let stored = self.store_images(&a);
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
            return Ok(());
        }
        if let Some(e) = a.error {
            return Err(e);
        }
        // The window shows a spinner until the request ends one way or the other.
        ensure!(stored > 0, ArtNotFound);
        Ok(())
    }

    fn store_app_id(&mut self, id: GameId, app_id: u32, source: SteamAppIdSource) {
        let Ok(mut game) = self.lib.game(id) else {
            return;
        };
        if !apply_app_id(&mut game, app_id, source) {
            return;
        }
        let patch = GamePatch {
            steam_app_id: Some(game.steam_app_id),
            steam_app_id_is_user_set: Some(game.steam_app_id_is_user_set),
            ..GamePatch::default()
        };
        if let Err(e) = self.lib.update(id, patch) {
            tracing::info!(error = %e, "steam app id not stored");
        }
    }

    /// Store each image; returns how many were kept.
    fn store_images(&mut self, a: &ArtFetched) -> usize {
        let now = self.deps.clock.now();
        let mut stored = 0;
        for (n, origin) in &a.images {
            // Re-check against the rows now: the user may have chosen a file meanwhile.
            let current = self
                .lib
                .db_mut()
                .artwork_of(a.game)
                .unwrap_or_default()
                .into_iter()
                .find(|r| r.slot == n.slot);
            if *origin == ArtOrigin::SteamCdn && !may_fetch(current.as_ref(), a.user_asked) {
                continue;
            }
            let fs = Arc::clone(&self.deps.fs);
            let paths = self.deps.paths.clone();
            match store_and_commit(&*fs, &paths, self.lib.db_mut(), a.game, n, *origin, now) {
                Ok(_) => stored += 1,
                Err(e) => tracing::warn!(error = %e, "artwork not stored"),
            }
        }
        stored
    }
}
