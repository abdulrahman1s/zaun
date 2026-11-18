//! Immutable snapshots shared by `Arc` between the AppService and the UI (ARCHITECTURE §6).

use std::sync::{Arc, Condvar, Mutex, PoisonError};

use crate::config::Preferences;
use crate::host::HostFacts;
use crate::ids::{GameId, SessionId};
use crate::library::LibrarySnapshot;
use crate::model::Session;
use crate::runtime::RuntimeCatalog;

/// `Mutex<Arc<T>>`: `load` copies the pointer, `store` replaces it. The lock is held for a
/// pointer copy only, so the UI thread never waits on the service. A watcher thread can
/// sleep until the value changes ([`Swap::wait_until`]) instead of polling it.
pub struct Swap<T> {
    inner: Mutex<Arc<T>>,
    changed: Condvar,
}

impl<T> Swap<T> {
    /// A cell holding `value`.
    pub fn new(value: T) -> Self {
        Self {
            inner: Mutex::new(Arc::new(value)),
            changed: Condvar::new(),
        }
    }

    /// The current value.
    pub fn load(&self) -> Arc<T> {
        Arc::clone(&self.inner.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Replace the value.
    pub fn store(&self, value: Arc<T>) {
        *self.inner.lock().unwrap_or_else(PoisonError::into_inner) = value;
        self.changed.notify_all();
    }

    /// Block until `done` holds for the current value and return that value. `done` is
    /// checked now, after every [`Swap::store`] and after every [`Swap::wake`]; it runs
    /// under the cell's lock, so it must be quick. For watcher threads, never the UI thread.
    pub fn wait_until(&self, mut done: impl FnMut(&Arc<T>) -> bool) -> Arc<T> {
        let guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let guard = self
            .changed
            .wait_while(guard, |current| !done(current))
            .unwrap_or_else(PoisonError::into_inner);
        Arc::clone(&guard)
    }

    /// Make waiters check their condition again: something it reads besides the value
    /// changed (a stop flag).
    pub fn wake(&self) {
        // Through the lock, so a waiter between its check and its sleep is not missed.
        drop(self.inner.lock().unwrap_or_else(PoisonError::into_inner));
        self.changed.notify_all();
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

/// Compatibility layers and runtimes as last scanned.
pub type RuntimesSnapshot = RuntimeCatalog;

/// Why the library is read-only (ERROR_HANDLING §5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryReason {
    /// The library file is damaged and salvage did not recover every game: the user
    /// chooses (PERSISTENCE §8). Until then an empty library is shown.
    Damaged {
        /// Backups that passed validation: (path, taken at, games), newest first.
        backups: Vec<(std::path::PathBuf, crate::clock::Timestamp, u64)>,
        /// Games salvage recovered (`None`: salvage failed).
        repaired_games: Option<u64>,
        /// Names of games salvage could not recover, where readable.
        missing_games: Vec<String>,
    },
    /// A schema update failed; the previous version was restored (`ZN-DB-013`).
    MigrationFailed {
        /// The migration step.
        step: u32,
    },
    /// The library was written by a newer Zaun (`ZN-DB-010`).
    NewerVersion,
}

/// Recovery mode: the library is shown read-only, launching is disabled with the reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryState {
    /// Why.
    pub reason: RecoveryReason,
    /// The user-facing error (title, message, actions).
    pub error: crate::error::UserError,
}

/// Diagnostics shown by the shell and the Doctor screen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diagnostics {
    /// The last full Doctor report (`Command::RunDoctor`, and once after start).
    pub doctor: Option<crate::host::DoctorReport>,
    /// A crash record not yet acknowledged (the next-start banner).
    pub crash: Option<crate::diagnostics::crash::CrashRecord>,
    /// Recovery mode.
    pub recovery: Option<RecoveryState>,
}

/// The one instance shared by the UI and the AppService.
pub struct SnapshotHub {
    /// All games, tags and per-game summary statistics.
    pub library: Swap<LibrarySnapshot>,
    /// Open sessions.
    pub sessions: Swap<ActiveSessions>,
    /// Compatibility layers and runtimes.
    pub runtimes: Swap<RuntimesSnapshot>,
    /// Preferences.
    pub prefs: Swap<Preferences>,
    /// Host facts.
    pub host: Swap<HostSnapshot>,
    /// Desktop integration: `.exe` handler and shortcuts.
    pub integration: Swap<crate::desktop::IntegrationState>,
    /// Doctor report, crash banner, recovery mode.
    pub diagnostics: Swap<Diagnostics>,
    /// Chat (ADR-0033).
    pub chat: Swap<crate::model::ChatSnapshot>,
    /// The picture of the stream the user watches (ADR-0036), outside the snapshot: frames
    /// come many times a second and only the latest counts.
    pub frames: crate::chat::media::FrameSlot,
    /// Whether the in-game overlay can be shown (ADR-0037): the hotkey's answer and
    /// Settings' note on desktops without a layer shell.
    pub overlay: Swap<crate::chat::overlay::OverlayStatus>,
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
            diagnostics: Swap::default(),
            chat: Swap::default(),
            frames: crate::chat::media::FrameSlot::new(),
            overlay: Swap::default(),
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

    #[test]
    fn waiters_sleep_until_a_store_or_a_wake() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let s = Arc::new(Swap::new(1));
        // Already true: no waiting.
        assert_eq!(*s.wait_until(|v| **v == 1), 1);
        let (s2, seen) = (Arc::clone(&s), s.load());
        let waiter = std::thread::spawn(move || *s2.wait_until(|v| !Arc::ptr_eq(v, &seen)));
        s.store(Arc::new(2));
        assert_eq!(waiter.join().unwrap(), 2);
        // A condition outside the value needs `wake`.
        let stop = Arc::new(AtomicBool::new(false));
        let (s2, stop2) = (Arc::clone(&s), Arc::clone(&stop));
        let waiter = std::thread::spawn(move || {
            s2.wait_until(|_| stop2.load(Ordering::SeqCst));
        });
        stop.store(true, Ordering::SeqCst);
        s.wake();
        waiter.join().unwrap();
    }
}
