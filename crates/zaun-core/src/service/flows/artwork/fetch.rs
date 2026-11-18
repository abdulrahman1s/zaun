//! Run the Steam artwork chain on the job pool (ADR-0018): only with `artwork.online = "steam"`.
use super::fetched::ArtFetched;
use crate::artwork::http::HttpClient;
use crate::artwork::import::normalise;
use crate::artwork::steam::{
    self, AppIdFile, AttemptTracker, Resolution, ResolveDeps, cdn_candidates, fetch_slot, may_fetch,
};
use crate::artwork::{ImageCodec, ImageLimits};
use crate::config::schema::ArtworkPrefs;
use crate::model::{ArtOrigin, ArtSlot, ArtworkRef, Game};
use crate::service::flows::runtime::CatalogUmuDb;
use crate::service::prelude::*;
use crate::storage::ArtworkRepo;

/// What the job owns: everything the chain reads, so it can leave the service thread.
struct Chain {
    id: GameId,
    user_asked: bool,
    prefs: ArtworkPrefs,
    game: Game,
    existing: Vec<ArtworkRef>,
    http: Arc<HttpClient>,
    codec: Arc<dyn ImageCodec>,
    tracker: Arc<AttemptTracker>,
    app_id_file: Arc<dyn AppIdFile>,
    umu_db: CatalogUmuDb,
}

impl AppService {
    /// Run the Steam chain for `id` (after add: `user_asked = false`, one attempt per slot).
    pub(crate) fn fetch_online_artwork(&mut self, id: GameId, user_asked: bool) -> Result<()> {
        let prefs = self.prefs.artwork.clone();
        if !steam::online(&prefs) {
            ensure!(
                !user_asked,
                IntNotAvailable,
                detail = "online artwork is turned off"
            );
            return Ok(());
        }
        let (Some(http), Some(codec)) = (self.deps.http.clone(), self.deps.codec.clone()) else {
            return Ok(());
        };
        let chain = Chain {
            id,
            user_asked,
            prefs,
            game: self.lib.game(id)?,
            existing: self.lib.db_mut().artwork_of(id).unwrap_or_default(),
            http,
            codec,
            tracker: Arc::clone(&self.art_tracker),
            app_id_file: Arc::clone(&self.deps.app_id_file),
            umu_db: CatalogUmuDb::new(Arc::clone(&self.deps.fs), Arc::clone(&self.deps.runtimes)),
        };
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Background,
            JobLabel("steam-artwork"),
            move |_| post(Internal::Art(Box::new(chain.run()))),
        );
        Ok(())
    }
}

impl Chain {
    fn run(&self) -> ArtFetched {
        let deps = ResolveDeps {
            app_id_file: &*self.app_id_file,
            umu_db: &self.umu_db,
            http: Some(&self.http),
        };
        let mut out = ArtFetched::new(self.id, self.user_asked);
        let (app_id, source) = match steam::resolve_app_id(&self.prefs, &self.game, &deps) {
            Resolution::Found { app_id, source } => (app_id, source),
            Resolution::Candidates(_) | Resolution::NotFound => return out,
        };
        out.app_id = Some((app_id, source));
        for slot in [ArtSlot::Cover, ArtSlot::Hero, ArtSlot::Icon] {
            if !self.wants(slot, app_id) {
                continue;
            }
            match fetch_slot(&self.prefs, &self.http, &*self.codec, app_id, slot) {
                steam::FetchOutcome::Fetched { bytes, .. } => {
                    match normalise(&*self.codec, slot, &bytes, &ImageLimits::DEFAULT) {
                        Ok(n) => out.images.push((n, ArtOrigin::SteamCdn)),
                        Err(e) => tracing::info!(error = %e, "steam artwork rejected"),
                    }
                }
                steam::FetchOutcome::Failed(e) => {
                    out.error = Some(err!(ArtNoConnection, reason = e));
                    break;
                }
                steam::FetchOutcome::NotAvailable | steam::FetchOutcome::Offline => {}
            }
        }
        out
    }

    /// Whether `slot` has an image to fetch, may be replaced, and is not already tried.
    fn wants(&self, slot: ArtSlot, app_id: u32) -> bool {
        if cdn_candidates(app_id, slot).is_empty() {
            return false;
        }
        let current = self.existing.iter().find(|a| a.slot == slot);
        if !may_fetch(current, self.user_asked) {
            return false;
        }
        self.tracker.begin(self.id, slot) || self.user_asked
    }
}
