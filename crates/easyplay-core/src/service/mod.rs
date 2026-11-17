//! The application façade: one thread, one command loop, all state transitions.
//!
//! Layer 6. See MODULES §7 and ARCHITECTURE §5–6.
//!
//! | File | Content |
//! |---|---|
//! | [`command`] | [`Command`] and its payload types |
//! | [`event`] | [`Event`], [`Notice`], [`SetupContext`] |
//! | [`explainer`] | policy explanations for the UI thread |
//! | [`snapshot`] | [`SnapshotHub`], [`Swap`], [`ActiveSessions`] |
//! | [`app_service`] | [`AppService`], [`ServiceDeps`], [`ServiceHandle`], the loop |
//! | [`prompts`] | parked flows |
//! | [`session_watch`] | the session watcher thread (PROC-011) |
//! | [`flows`] | open, setup, launch, remove, runtime, artwork |

pub mod app_service;
pub mod command;
pub mod event;
pub mod explainer;
pub mod flows;
pub mod prompts;
pub mod recovery;
pub mod session_watch;
pub mod snapshot;

pub use app_service::{
    AppService, CommandSender, LaunchEnv, ServiceDeps, ServiceHandle, Waker, open_db,
};
pub use command::{
    AfterCreate, Command, DatabaseChoice, FolderRequest, GameEdit, InspectTarget, LaunchOrigin,
    LogFile, OpenOrigin, SteamCommand, StopMode,
};
pub use event::{Event, LogChunk, Notice, QueryResult, RecoveredHow, SetupContext, SteamDetail};
pub use snapshot::{
    ActiveSessions, Diagnostics, HostSnapshot, RecoveryReason, RecoveryState, RuntimesSnapshot,
    SnapshotHub, Swap,
};
