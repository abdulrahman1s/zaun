//! The application façade: one thread, one command loop, all state transitions (MODULES §7, ARCHITECTURE §5–6).
//!
//! | File | Job |
//! |---|---|
//! | `command`, `event` | what goes in (`Command`) and what comes out (`Event`, `Notice`) |
//! | `app_service` | the struct, the loop and the plumbing (`emit`, `fail`, `publish_*`) |
//! | `dispatch` | the routes table: command, job completion or prompt answer to its flow |
//! | `flows/` | one folder or file per user action |
//! | `prompts` | parked flows waiting for an answer |
//! | `snapshot`, `session_watch` | published state; the session watcher thread (PROC-011) |
//! | `recovery`, `headless`, `explainer` | damaged-library mode; no-thread use; policy explanations |

pub mod app_service;
pub mod command;
mod dispatch;
pub mod event;
pub mod explainer;
pub mod flows;
mod headless;
pub(crate) mod prelude;
pub mod prompts;
pub mod recovery;
pub mod session_watch;
pub mod snapshot;

pub use app_service::{
    AppService, CommandSender, LaunchEnv, ServiceDeps, ServiceHandle, Waker, open_db,
};
pub use command::{
    AfterCreate, ChatCommand, ClipKind, Command, DatabaseChoice, FolderRequest, GameEdit,
    InspectTarget, LaunchOrigin, LogFile, OpenOrigin, SteamCommand, StopMode,
};
pub use event::{
    ChatPage, Event, LogChunk, Notice, QueryResult, RecoveredHow, SetupContext, SteamDetail,
};
pub use snapshot::{
    ActiveSessions, Diagnostics, HostSnapshot, RecoveryReason, RecoveryState, RuntimesSnapshot,
    SnapshotHub, Swap,
};
