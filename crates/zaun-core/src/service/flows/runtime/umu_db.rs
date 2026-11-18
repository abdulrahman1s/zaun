//! The umu database over the runtime catalogue (UMU_INTEGRATION §8).
//!
//! Drafts match titles in the default build's database; Steam artwork looks up the game's
//! resolved build. It replaces the stubs of both with the real reader (`runtime::umu_db`).
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::fsx::Fs;
use crate::library::draft::UmuTitleMatch;
use crate::model::{Game, RuntimeRef};
use crate::runtime::{Resolution, RuntimeCatalog, RuntimeService, UmuDb, UmuDbCache};
use std::sync::Arc;

pub struct CatalogUmuDb {
    fs: Arc<dyn Fs>,
    runtimes: Arc<dyn RuntimeService>,
    cache: UmuDbCache,
    warned: AtomicBool,
}

impl CatalogUmuDb {
    /// A reader over `runtimes`.
    pub fn new(fs: Arc<dyn Fs>, runtimes: Arc<dyn RuntimeService>) -> Self {
        Self {
            fs,
            runtimes,
            cache: UmuDbCache::new(),
            warned: AtomicBool::new(false),
        }
    }

    /// The build directory whose database applies to `game` (default reference without a
    /// game); falls back to the newest build that ships fixes.
    fn build_dir(&self, game: Option<&Game>) -> Option<PathBuf> {
        let reference = game.map_or(RuntimeRef::Default, |g| g.runtime.clone());
        match self.runtimes.resolve(&reference, game) {
            Resolution::Ready(rt) => Some(rt.proton_path_host),
            _ => {
                let c: Arc<RuntimeCatalog> = self.runtimes.catalog();
                c.builds
                    .iter()
                    .rev()
                    .find(|b| b.has_fixes)
                    .map(|b| b.path.clone())
            }
        }
    }

    fn db(&self, game: Option<&Game>) -> Option<Arc<UmuDb>> {
        let dir = self.build_dir(game)?;
        let db = self.cache.get(&*self.fs, &dir);
        if db.is_none() && !self.warned.swap(true, Ordering::Relaxed) {
            tracing::debug!(build = %dir.display(), "no umu database in the build");
        }
        db
    }

    /// The Steam app ID of `game` from the database (artwork step 3.3).
    pub fn steam_app_id_of(&self, game: &Game) -> Option<u32> {
        self.db(Some(game))?
            .steam_app_id(game.umu_id.as_deref(), &game.name)
    }
}

impl std::fmt::Debug for CatalogUmuDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CatalogUmuDb").finish_non_exhaustive()
    }
}

impl UmuTitleMatch for CatalogUmuDb {
    fn exact_title(&self, name: &str) -> Option<String> {
        self.db(None)?.exact_title(name)
    }
}

impl crate::artwork::steam::UmuDbSteamId for CatalogUmuDb {
    fn steam_app_id(&self, game: &Game) -> Option<u32> {
        self.steam_app_id_of(game)
    }
}
