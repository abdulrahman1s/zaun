//! `FindSteamApp` (ADR-0024 §1): the ID the computer gives and store search results, answered through its query.
use crate::artwork::steam::{self, ResolveDeps};
use crate::service::flows::runtime::CatalogUmuDb;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn find_steam_app(
        &mut self,
        id: GameId,
        term: Option<String>,
        reply_to: QueryId,
    ) -> Result<()> {
        let describe = self.describer(Some(id));
        let game = match self.lib.game(id) {
            Ok(g) => g,
            Err(e) => {
                self.answer_on_pool("find-steam-app", reply_to, move |_| {
                    QueryResult::Failed(Box::new(describe(&e)))
                });
                return Ok(());
            }
        };
        let prefs = self.prefs.artwork.clone();
        let http = self.deps.http.clone();
        let app_id_file = Arc::clone(&self.deps.app_id_file);
        let umu_db = CatalogUmuDb::new(Arc::clone(&self.deps.fs), Arc::clone(&self.deps.runtimes));
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
                        let err = err!(ArtNoConnection, reason = e);
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
        Ok(())
    }
}
