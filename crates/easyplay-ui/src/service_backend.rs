//! [`Backend`] over the real application service: maps [`UiCommand`] onto
//! `service::Command`, `service::Event` onto [`UiEvent`], and the hub's `LibrarySnapshot`
//! onto [`LibraryView`] (cached per revision). Thin on purpose: the seam grows with the
//! screens (setup dialog, prompts, launch progress).
//!
//! Until the setup dialog and the prompt dialogs exist, two stand-ins apply:
//!
//! - `SetupRequested` confirms the proposed draft as is (the setup defaults of LOGIC_SPECS §2:
//!   Offline preset, the detected game folder) and adds the game; a program opened from the
//!   file manager or the command line is launched afterwards ("Play").
//! - a `Prompt` is answered with *Cancel*, which is always safe (no flow commits before its
//!   prompt is answered), and a toast says what was asked.

use std::collections::VecDeque;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, PoisonError};

use easyplay_core::host::HostFacts;
use easyplay_core::launch::LaunchState;
use easyplay_core::model::{PromptAnswer, PromptKind};
use easyplay_core::service::{
    AfterCreate, Command, Event, LaunchOrigin, Notice, OpenOrigin, ServiceHandle, StopMode,
};

use crate::bridge::{Backend, HostStatus, LibraryView, TagInfo, UiCommand, UiEvent};

/// Toast wording for service notices (moves to `strings.rs` with the screens).
mod text {
    use easyplay_core::model::{PromptKind, SlrVariant};

    pub fn added(name: &str) -> String {
        format!("Added {name}")
    }
    pub const ALREADY: &str = "Already in your library";
    pub const FOUND_MOVED: &str = "Found the game in its new location";
    pub fn only_first(n: usize) -> String {
        format!("Only the first {n} files were opened")
    }
    pub fn removed(name: &str) -> String {
        format!("Removed {name}")
    }
    pub const KEPT_PLACEHOLDER: &str = "The program's icon could not be used";
    pub const ARTWORK: &str = "Artwork updated";
    pub fn adopted(v: SlrVariant) -> String {
        format!("Steam Linux Runtime ({}) copied into EasyPlay", v.as_str())
    }
    pub const RESET: &str = "Private Windows storage reset";
    pub const STOPPING: &str = "Stopping…";
    pub fn prompt(kind: &PromptKind) -> &'static str {
        match kind {
            PromptKind::ExeChanged { .. } => {
                "The program file changed since it was added. Nothing was started."
            }
            PromptKind::InsideKnownRoot { .. } | PromptKind::CopyOfKnown { .. } => {
                "This program belongs to a game in your library. Nothing was added."
            }
            PromptKind::RuntimeUnavailable { .. } => {
                "The compatibility layer of this game is no longer installed."
            }
            PromptKind::RemoveRunningGame { .. } => "Stop the game before removing it.",
            _ => "EasyPlay needs a decision that this version cannot ask yet.",
        }
    }
}

struct State {
    events: Receiver<Event>,
    pending: VecDeque<UiEvent>,
    view: Arc<LibraryView>,
}

/// The production backend.
pub struct ServiceBackend {
    handle: ServiceHandle,
    state: Mutex<State>,
}

impl ServiceBackend {
    /// Wrap a running service and its event receiver.
    pub fn new(handle: ServiceHandle, events: Receiver<Event>) -> Self {
        Self {
            handle,
            state: Mutex::new(State {
                events,
                pending: VecDeque::new(),
                view: Arc::new(LibraryView::default()),
            }),
        }
    }

    /// The service handle (commands the bridge does not cover yet).
    pub fn handle(&self) -> &ServiceHandle {
        &self.handle
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Map one service event; `None` when the UI has nothing to do.
    fn map(&self, ev: Event) -> Option<UiEvent> {
        match ev {
            Event::LibraryChanged => Some(UiEvent::LibraryChanged),
            Event::HostFactsChanged => Some(UiEvent::HostChanged),
            Event::SessionsChanged
            | Event::RuntimesChanged
            | Event::PreferencesChanged
            | Event::ShowGame(_)
            | Event::QuitRequested => None,
            Event::SetupRequested { draft, context } => {
                let then = if context.origin.launches_known() {
                    AfterCreate::Launch
                } else {
                    AfterCreate::Nothing
                };
                self.handle.send(Command::CreateGame { draft, then });
                None
            }
            Event::Prompt(p) => {
                let toast = text::prompt(&p.kind).to_owned();
                let answer = match p.kind {
                    PromptKind::FixNeedsNetwork { .. } => PromptAnswer::NotNow,
                    _ => PromptAnswer::Cancel,
                };
                self.handle.send(Command::AnswerPrompt {
                    prompt: p.id,
                    answer,
                });
                Some(UiEvent::Toast(toast))
            }
            Event::Notice(n) => Some(UiEvent::Toast(match n {
                Notice::GameAdded { name, .. } => text::added(&name),
                Notice::AlreadyInLibrary { .. } => text::ALREADY.into(),
                Notice::FoundMoved { .. } => text::FOUND_MOVED.into(),
                Notice::OnlyFirstPaths { processed } => text::only_first(processed),
                Notice::GameRemoved { name, .. } => text::removed(&name),
                Notice::ArtworkKeptPlaceholder { .. } => text::KEPT_PLACEHOLDER.into(),
                Notice::ArtworkFetched { .. } => text::ARTWORK.into(),
                Notice::RuntimeAdopted { variant, .. } => text::adopted(variant),
                Notice::PrefixReset { .. } => text::RESET.into(),
                Notice::StopRequested { .. } => text::STOPPING.into(),
            })),
            Event::Failure(u) => Some(UiEvent::Toast(format!("{}. {}", u.title, u.message))),
            Event::LaunchProgress { state, .. } => match state {
                LaunchState::Failed(u) => {
                    Some(UiEvent::Toast(format!("{}. {}", u.title, u.message)))
                }
                _ => None,
            },
        }
    }
}

/// The host-status row from the facts (no I/O).
pub fn host_status(facts: Option<&HostFacts>) -> HostStatus {
    match facts {
        None => HostStatus::Unknown,
        Some(f) if f.bwrap.is_none() || f.umu.is_none() || !f.problems.is_empty() => {
            HostStatus::NeedsAttention
        }
        Some(_) => HostStatus::Ready,
    }
}

impl Backend for ServiceBackend {
    fn send(&self, cmd: UiCommand) {
        let cmd = match cmd {
            UiCommand::Launch(id) => Command::Launch {
                id,
                exe: None,
                origin: LaunchOrigin::Library,
            },
            UiCommand::Stop(id) => {
                let sessions = self.handle.hub.sessions.load();
                let Some(s) = sessions.for_game(id) else {
                    return;
                };
                Command::StopSession {
                    session: s.id,
                    mode: StopMode::Graceful,
                }
            }
            UiCommand::SetFavorite(id, favorite) => Command::SetFavorite { id, favorite },
            UiCommand::OpenPaths(paths) => Command::OpenPaths {
                paths,
                origin: OpenOrigin::Picker,
            },
        };
        self.handle.send(cmd);
    }

    fn try_event(&self) -> Option<UiEvent> {
        loop {
            let ev = {
                let mut s = self.lock();
                if let Some(e) = s.pending.pop_front() {
                    return Some(e);
                }
                s.events.try_recv().ok()?
            };
            if let Some(ui) = self.map(ev) {
                return Some(ui);
            }
        }
    }

    fn library(&self) -> Arc<LibraryView> {
        let snap = self.handle.hub.library.load();
        let mut s = self.lock();
        if s.view.revision != snap.revision || s.view.games.len() != snap.games.len() {
            s.view = Arc::new(LibraryView {
                revision: snap.revision,
                games: snap.games.clone().into(),
                tags: snap
                    .tags
                    .iter()
                    .map(|t| TagInfo {
                        id: t.id,
                        name: Arc::clone(&t.name),
                        color: t.color,
                    })
                    .collect(),
            });
        }
        Arc::clone(&s.view)
    }

    fn host_status(&self) -> HostStatus {
        host_status(self.handle.hub.host.load().as_ref().as_ref())
    }

    fn set_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) {
        self.handle.set_waker(waker);
    }
}
