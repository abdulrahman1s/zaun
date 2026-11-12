//! Dominant cover colours for the snapshot (`GameSummary::accent`, the game page's ambient
//! tint): read from the `epCl` header of an existing cover thumbnail, else computed from the
//! stored original. Runs on the job pool at start and after a game's artwork changes.

use std::sync::Arc;

use super::super::app_service::{AppService, Internal};
use super::FlowDone;
use crate::ids::GameId;
use crate::jobs::{JobLabel, JobPriority};
use crate::model::{ArtSlot, ArtworkRef};

impl AppService {
    /// Work out the accents of `games` (all games when `None`).
    pub(crate) fn refresh_accents(&mut self, games: Option<&[GameId]>) {
        let rows: Vec<ArtworkRef> = match games {
            None => match crate::storage::GameRepo::load_library(self.lib.db_mut()) {
                Ok(r) => r.artwork,
                Err(e) => return tracing::warn!(error = %e, "accents not computed"),
            },
            Some(ids) => {
                use crate::storage::ArtworkRepo;
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
        let mut covers: Vec<(GameId, Option<ArtworkRef>)> = rows
            .into_iter()
            .filter(|r| r.slot == ArtSlot::Cover)
            .map(|r| (r.game_id, Some(r)))
            .collect();
        if let Some(ids) = games {
            for id in ids {
                if !covers.iter().any(|(g, _)| g == id) {
                    covers.push((*id, None));
                }
            }
        }
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
                        crate::artwork::thumbs::stored_colour(&*fs, &paths.thumbs_dir, &r.hash)
                            .or_else(|| {
                                let codec = codec.as_ref()?;
                                let limits = crate::artwork::ImageLimits::DEFAULT;
                                let file = crate::artwork::store::original_path(&paths, &r);
                                let bytes = fs.read(&file, limits.max_bytes).ok()?;
                                let img = codec.decode(&bytes, &limits).ok()?;
                                Some(crate::artwork::import::dominant_colour(&img))
                            })
                    });
                    out.push((game, colour));
                }
                post(Internal::Flow(Box::new(FlowDone::Accents(out))));
            },
        );
    }
}
