//! `UiBridge`: the UI's only connection to the application service. `send(UiCommand)` and
//! snapshot accessors; the UI never blocks on it (`try_event` only). See MODULES §8 and
//! ARCHITECTURE §5.1.
//!
//! The seam is the [`Backend`] trait. Until the protocol types of SVC-001 exist in
//! `easyplay_core::service`, the UI defines the subset it needs here ([`UiCommand`],
//! [`UiEvent`], [`LibraryView`], [`HostStatus`]); the service adapter implements `Backend` for
//! its `ServiceHandle` by mapping these one-to-one onto `Command`/`Event` and the
//! `SnapshotHub`'s `LibrarySnapshot`. Two implementations ship with the crate:
//! [`crate::demo::DemoBackend`] (sample library, used when no service is connected) and
//! [`FakeBackend`] (records commands; tests publish snapshots and events).

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use easyplay_core::ids::{GameId, TagId};
use easyplay_core::model::{GameSummary, TagColor};

/// One tag as the sidebar shows it (GAME_LIBRARY §1.1 `TagInfo`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagInfo {
    /// Identifier.
    pub id: TagId,
    /// Display name.
    pub name: Arc<str>,
    /// Hue.
    pub color: TagColor,
}

/// The library snapshot subset the UI reads (GAME_LIBRARY §1.1 `LibrarySnapshot`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LibraryView {
    /// Increments on every publish; caches key on it.
    pub revision: u64,
    /// All games, sorted by `sort_name`.
    pub games: Arc<[GameSummary]>,
    /// All tags, sorted by name.
    pub tags: Arc<[TagInfo]>,
}

impl LibraryView {
    /// The game with `id`.
    pub fn game(&self, id: GameId) -> Option<&GameSummary> {
        self.games.iter().find(|g| g.id == id)
    }
}

/// The host-status row (SCREEN_SPECS §0).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HostStatus {
    /// Before the first host facts arrive.
    #[default]
    Unknown,
    /// All checks pass.
    Ready,
    /// A check needs the user (opens Doctor).
    NeedsAttention,
    /// Background jobs running for more than 3 s.
    Busy,
}

/// Commands the UI sends (subset of ARCHITECTURE §6 `Command`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiCommand {
    /// `Command::Launch { id, exe: None, origin: Library }`.
    Launch(GameId),
    /// `Command::StopSession` for the game's session, graceful.
    Stop(GameId),
    /// `Command::SetFavorite`.
    SetFavorite(GameId, bool),
    /// `Command::OpenPaths` from the add-game picker or a drop.
    OpenPaths(Vec<PathBuf>),
}

/// Events the UI drains (subset of ARCHITECTURE §6 `Event`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiEvent {
    /// A new library snapshot is available.
    LibraryChanged,
    /// Host status changed.
    HostChanged,
    /// Show a toast.
    Toast(String),
}

/// What the UI needs from the application service. Implementations must be cheap and
/// non-blocking: `send` queues, `try_event` never waits, snapshot accessors copy an `Arc`.
pub trait Backend: Send {
    /// Queue a command.
    fn send(&self, cmd: UiCommand);
    /// The next pending event, if any.
    fn try_event(&self) -> Option<UiEvent>;
    /// The current library snapshot.
    fn library(&self) -> Arc<LibraryView>;
    /// The current host status.
    fn host_status(&self) -> HostStatus;
    /// Install the repaint waker (`egui::Context::request_repaint`) for other threads.
    fn set_waker(&self, waker: Arc<dyn Fn() + Send + Sync>);
    /// Whether this is sample data (shows the preview banner).
    fn is_preview(&self) -> bool {
        false
    }
}

/// Maximum events drained per frame (ARCHITECTURE §5.1).
pub const MAX_EVENTS_PER_FRAME: usize = 256;

/// The UI side of the seam: caches the current snapshots for the frame.
pub struct UiBridge {
    backend: Box<dyn Backend>,
    library: Arc<LibraryView>,
    host: HostStatus,
}

impl UiBridge {
    /// Wrap a backend and take the first snapshots.
    pub fn new(backend: Box<dyn Backend>) -> Self {
        let library = backend.library();
        let host = backend.host_status();
        Self {
            backend,
            library,
            host,
        }
    }

    /// Send a command (never blocks).
    pub fn send(&self, cmd: UiCommand) {
        self.backend.send(cmd);
    }

    /// Drain up to [`MAX_EVENTS_PER_FRAME`] events, refreshing snapshots as announced.
    /// Returns the events that need view-model handling (toasts and the like).
    pub fn drain(&mut self) -> Vec<UiEvent> {
        let mut out = Vec::new();
        for _ in 0..MAX_EVENTS_PER_FRAME {
            let Some(ev) = self.backend.try_event() else {
                break;
            };
            match ev {
                UiEvent::LibraryChanged => self.library = self.backend.library(),
                UiEvent::HostChanged => self.host = self.backend.host_status(),
                other => out.push(other),
            }
        }
        out
    }

    /// The library snapshot of this frame.
    pub fn library(&self) -> &Arc<LibraryView> {
        &self.library
    }

    /// Host status of this frame.
    pub fn host_status(&self) -> HostStatus {
        self.host
    }

    /// Whether the backend serves sample data.
    pub fn is_preview(&self) -> bool {
        self.backend.is_preview()
    }

    /// Pass the repaint waker to the backend.
    pub fn set_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) {
        self.backend.set_waker(waker);
    }
}

#[derive(Default)]
struct FakeState {
    sent: Vec<UiCommand>,
    events: std::collections::VecDeque<UiEvent>,
    library: Arc<LibraryView>,
    host: HostStatus,
}

/// A test backend: records commands; tests publish snapshots and events.
#[derive(Clone, Default)]
pub struct FakeBackend {
    state: Arc<Mutex<FakeState>>,
}

impl FakeBackend {
    /// A backend serving `library`.
    pub fn new(library: LibraryView) -> Self {
        let f = Self::default();
        f.lock().library = Arc::new(library);
        f.lock().host = HostStatus::Ready;
        f
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Commands sent so far.
    pub fn sent(&self) -> Vec<UiCommand> {
        self.lock().sent.clone()
    }

    /// Publish a new library and announce it.
    pub fn publish_library(&self, mut library: LibraryView) {
        let mut s = self.lock();
        library.revision = s.library.revision + 1;
        s.library = Arc::new(library);
        s.events.push_back(UiEvent::LibraryChanged);
    }

    /// Queue an event.
    pub fn push_event(&self, ev: UiEvent) {
        self.lock().events.push_back(ev);
    }

    /// Set the host status and announce it.
    pub fn set_host(&self, host: HostStatus) {
        let mut s = self.lock();
        s.host = host;
        s.events.push_back(UiEvent::HostChanged);
    }
}

impl Backend for FakeBackend {
    fn send(&self, cmd: UiCommand) {
        self.lock().sent.push(cmd);
    }
    fn try_event(&self) -> Option<UiEvent> {
        self.lock().events.pop_front()
    }
    fn library(&self) -> Arc<LibraryView> {
        self.lock().library.clone()
    }
    fn host_status(&self) -> HostStatus {
        self.lock().host
    }
    fn set_waker(&self, _waker: Arc<dyn Fn() + Send + Sync>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_refreshes_snapshots_and_returns_other_events() {
        let fake = FakeBackend::new(LibraryView::default());
        let mut bridge = UiBridge::new(Box::new(fake.clone()));
        assert_eq!(bridge.library().revision, 0);
        fake.publish_library(LibraryView::default());
        fake.push_event(UiEvent::Toast("hi".into()));
        fake.set_host(HostStatus::NeedsAttention);
        let evs = bridge.drain();
        assert_eq!(evs, vec![UiEvent::Toast("hi".into())]);
        assert_eq!(bridge.library().revision, 1);
        assert_eq!(bridge.host_status(), HostStatus::NeedsAttention);
        bridge.send(UiCommand::OpenPaths(vec![]));
        assert_eq!(fake.sent(), vec![UiCommand::OpenPaths(vec![])]);
    }
}
