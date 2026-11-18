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

/// The watch stage's pictures (ADR-0036), the preview's test picture, the profile-picture
/// preparation the preview runs, and the send limit; here for the same reason.
pub use crate::chat::MAX_SEND as CHAT_MAX_SEND;
pub use crate::chat::media::pattern::test_pattern;
pub use crate::chat::media::{FrameSlot, VideoFrame};
/// Whether the in-game overlay can be shown (`SnapshotHub::overlay`, ADR-0037); here for the
/// UI, which may not name `chat` (ARCHITECTURE §3 rule 2).
pub use crate::chat::overlay::OverlayStatus;
pub use crate::chat::picture as chat_picture;
pub use app_service::{
    AppService, CommandSender, LaunchEnv, ServiceDeps, ServiceHandle, Waker, open_db,
};
pub use command::{
    AfterCreate, ChatCommand, ClipKind, Command, DatabaseChoice, FolderRequest, GameEdit,
    InspectTarget, LaunchOrigin, LogFile, OpenOrigin, OverlayAction, ShareHeight, SteamCommand,
    StopMode, UpdateCommand,
};
pub use event::{
    ChatPage, Event, LogChunk, Notice, QueryResult, RecoveredHow, SetupContext, SteamDetail,
};
pub use snapshot::{
    ActiveSessions, Diagnostics, HostSnapshot, RecoveryReason, RecoveryState, RuntimesSnapshot,
    SnapshotHub, Swap,
};
