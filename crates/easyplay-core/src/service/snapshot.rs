//! Immutable snapshots shared by `Arc` between the AppService and the UI (ARCHITECTURE §6).

use std::sync::{Arc, Mutex, PoisonError};

use crate::config::Preferences;
use crate::host::HostFacts;
use crate::ids::{GameId, SessionId};
use crate::library::LibrarySnapshot;
use crate::model::Session;
use crate::runtime::RuntimeCatalog;

/// `Mutex<Arc<T>>`: `load` copies the pointer, `store` replaces it. The lock is held for a
/// pointer copy only, so the UI thread never waits on the service.
pub struct Swap<T> {
    inner: Mutex<Arc<T>>,
}

impl<T> Swap<T> {
    /// A cell holding `value`.
    pub fn new(value: T) -> Self {
        Self {
            inner: Mutex::new(Arc::new(value)),
        }
    }

    /// The current value.
    pub fn load(&self) -> Arc<T> {
        Arc::clone(&self.inner.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Replace the value.
    pub fn store(&self, value: Arc<T>) {
        *self.inner.lock().unwrap_or_else(PoisonError::into_inner) = value;
    }
}

impl<T: Default> Default for Swap<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

/// Open sessions as the GUI sees them (rows written by supervisors, polled by the session
/// watcher).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActiveSessions {
    /// Increments on every publish.
    pub revision: u64,
    /// Open sessions, oldest first.
    pub sessions: Vec<Session>,
}

impl ActiveSessions {
    /// The open session of a game (one per prefix).
    pub fn for_game(&self, game: GameId) -> Option<&Session> {
        self.sessions.iter().find(|s| s.game_id == Some(game))
    }

    /// A session by ID.
    pub fn get(&self, id: SessionId) -> Option<&Session> {
        self.sessions.iter().find(|s| s.id == id)
    }

    /// Whether nothing runs.
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

/// Host facts once probed (`None` until the first probe finished).
pub type HostSnapshot = Option<HostFacts>;

/// The one instance shared by the UI and the AppService.
pub struct SnapshotHub {
    /// All games, tags and per-game summary statistics.
    pub library: Swap<LibrarySnapshot>,
    /// Open sessions.
    pub sessions: Swap<ActiveSessions>,
    /// Compatibility layers and runtimes.
    pub runtimes: Swap<RuntimeCatalog>,
    /// Preferences.
    pub prefs: Swap<Preferences>,
    /// Host facts.
    pub host: Swap<HostSnapshot>,
    /// Desktop integration: `.exe` handler and shortcuts.
    pub integration: Swap<crate::desktop::IntegrationState>,
}

impl SnapshotHub {
    /// Empty snapshots; `prefs` as loaded.
    pub fn new(library: LibrarySnapshot, prefs: Preferences) -> Self {
        Self {
            library: Swap::new(library),
            sessions: Swap::default(),
            runtimes: Swap::default(),
            prefs: Swap::new(prefs),
            host: Swap::new(None),
            integration: Swap::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swap_replaces_without_mutating_old_values() {
        let s = Swap::new(1);
        let old = s.load();
        s.store(Arc::new(2));
        assert_eq!(*old, 1);
        assert_eq!(*s.load(), 2);
    }
}
