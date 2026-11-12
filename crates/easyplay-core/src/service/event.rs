//! What the AppService tells the UI (ARCHITECTURE §6). Snapshot changes are announced, not
//! carried: the new value is in the [`super::SnapshotHub`].
//!
//! User-visible text is not produced here. Errors arrive as `UserError` (written by
//! `error::catalog`); notices arrive as typed [`Notice`]s that the UI words.

use std::path::PathBuf;

use super::command::OpenOrigin;
use crate::error::UserError;
use crate::ids::{GameId, LaunchId, SessionId};
use crate::launch::LaunchState;
use crate::library::GameDraft;
use crate::model::{PromptRequest, SlrVariant};

/// The context of a setup dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupContext {
    /// Where the file came from.
    pub origin: OpenOrigin,
    /// Further drafts waiting behind this one.
    pub queued: usize,
}

/// A short, typed notice for a toast.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// A game was added.
    GameAdded {
        /// The game.
        game: GameId,
        /// Its name.
        name: String,
    },
    /// The opened file belongs to a game already in the library.
    AlreadyInLibrary {
        /// The game.
        game: GameId,
    },
    /// A moved game was found in its new location.
    FoundMoved {
        /// The game.
        game: GameId,
    },
    /// More than 64 paths were opened; the rest were ignored.
    OnlyFirstPaths {
        /// How many were processed.
        processed: usize,
    },
    /// A game was removed.
    GameRemoved {
        /// Its name.
        name: String,
        /// Where its save data was kept.
        saved_data: Option<PathBuf>,
    },
    /// The game's icon could not be stored; it keeps the generated cover.
    ArtworkKeptPlaceholder {
        /// The game.
        game: GameId,
    },
    /// Artwork was fetched from the Steam CDN.
    ArtworkFetched {
        /// The game.
        game: GameId,
        /// Slots stored.
        slots: usize,
    },
    /// A runtime variant was copied into the store.
    RuntimeAdopted {
        /// The variant.
        variant: SlrVariant,
        /// Reflinked (no extra space used).
        reflinked: bool,
    },
    /// The private Windows storage was reset.
    PrefixReset {
        /// The game.
        game: GameId,
    },
    /// A stop was requested.
    StopRequested {
        /// The session.
        session: SessionId,
    },
}

/// An announcement from the AppService.
#[derive(Clone, Debug)]
pub enum Event {
    /// A new library snapshot is in the hub.
    LibraryChanged,
    /// A new open-sessions snapshot is in the hub.
    SessionsChanged,
    /// A new runtime catalogue is in the hub.
    RuntimesChanged,
    /// New preferences are in the hub.
    PreferencesChanged,
    /// New host facts are in the hub.
    HostFactsChanged,
    /// A launch advanced.
    LaunchProgress {
        /// The game.
        game: GameId,
        /// The launch.
        launch: LaunchId,
        /// The state entered.
        state: LaunchState,
    },
    /// An unknown program needs the setup dialog.
    SetupRequested {
        /// The proposal.
        draft: Box<GameDraft>,
        /// Context.
        context: SetupContext,
    },
    /// A flow needs a decision.
    Prompt(PromptRequest),
    /// The UI should show this game's page (a known program was dropped or picked).
    ShowGame(GameId),
    /// A short notice.
    Notice(Notice),
    /// An error not attached to a launch.
    Failure(Box<UserError>),
    /// The service stopped (after `Shutdown`).
    QuitRequested,
}
