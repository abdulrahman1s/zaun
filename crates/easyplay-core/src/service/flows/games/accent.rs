//! Dominant cover colours for the snapshot (`GameSummary::accent`, the game page's ambient tint).
//!
//! Read from the `epCl` header of an existing cover thumbnail, else computed from the stored
//! original. Runs on the job pool at start and after a game's artwork changes.
use crate::artwork::{ImageLimits, import, store, thumbs};
use crate::model::{ArtSlot, ArtworkRef};
use crate::service::prelude::*;
use crate::storage::{ArtworkRepo, GameRepo};

type Cover = (GameId, Option<ArtworkRef>);

impl AppService {
    /// Work out the accents of `games` (all games when `None`). Infallible: it only logs.
    pub(crate) fn refresh_accents(&mut self, games: Option<&[GameId]>) {
        let covers = self.cover_rows(games);
        if covers.is_empty() {
            return;
        }
        let fs = Arc::clone(&self.deps.fs);
        let codec = self.deps.codec.clone();
        let paths = self.deps.paths.clone();
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Background,
            JobLabel("accents"),
            move |cancel| {
                let mut out = Vec::new();
                for (game, row) in covers {
                    if cancel.is_cancelled() {
                        break;
                    }
                    let colour = row.and_then(|r| {
                        thumbs::stored_colour(&*fs, &paths.thumbs_dir, &r.hash).or_else(|| {
                            let codec = codec.as_ref()?;
                            let limits = ImageLimits::DEFAULT;
                            let file = store::original_path(&paths, &r);
                            let bytes = fs.read(&file, limits.max_bytes).ok()?;
                            let img = codec.decode(&bytes, &limits).ok()?;
                            Some(import::dominant_colour(&img))
                        })
                    });
                    out.push((game, colour));
                }
                post(Internal::Flow(Box::new(FlowDone::Accents(out))));
            },
        );
    }

    /// The cover row of each game asked for; a game with none still gets an entry.
    fn cover_rows(&mut self, games: Option<&[GameId]>) -> Vec<Cover> {
        let rows: Vec<ArtworkRef> = match games {
            None => match GameRepo::load_library(self.lib.db_mut()) {
                Ok(r) => r.artwork,
                Err(e) => {
                    tracing::warn!(error = %e, "accents not computed");
                    return Vec::new();
                }
            },
            Some(ids) => {
                let mut out = Vec::new();
                for id in ids {
                    match self.lib.db_mut().artwork_of(*id) {
                        Ok(r) => out.extend(r),
                        Err(e) => tracing::debug!(error = %e, "artwork rows not read"),
                    }
                }
                out
            }
        };
        let mut covers: Vec<Cover> = rows
            .into_iter()
            .filter(|r| r.slot == ArtSlot::Cover)
            .map(|r| (r.game_id, Some(r)))
            .collect();
        for id in games.unwrap_or_default() {
            if !covers.iter().any(|(g, _)| g == id) {
                covers.push((*id, None));
            }
        }
        covers
    }
}
