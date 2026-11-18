//! What flow jobs send back (`FlowDone`) and what the service keeps between messages (`FlowState`).

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use super::diagnostics::maintenance;
use super::{chat, steam, updates};
use crate::error::Result;
use crate::ids::{GameId, LaunchId, QueryId, SessionId};
use crate::library::prefix_ops::ResetReport;
use crate::library::{RemoveFiles, RemoveJob};
use crate::model::Availability;
use crate::service::event::QueryResult;
use crate::storage::ReadDb;

/// Completions of flow jobs, re-entering the loop as `Internal::Flow`.
pub(crate) enum FlowDone {
    Removed {
        job: RemoveJob,
        files: RemoveFiles,
    },
    Reset {
        game: GameId,
        result: Result<ResetReport>,
    },
    /// A tool session was handed to its supervisor (or failed).
    Tool {
        game: GameId,
        result: Result<SessionId>,
    },
    Query {
        id: QueryId,
        result: QueryResult,
    },
    /// Dominant cover colours worked out.
    Accents(Vec<(GameId, Option<[u8; 3]>)>),
    /// Whether each game's main program is there.
    Availability(Vec<(GameId, Availability)>),
    Maintenance(maintenance::MaintenanceReport),
    /// ADR-0031.
    Steam(steam::SteamDone),
    /// The engine reported, or a restart is due (ADR-0033).
    Chat(chat::ChatDone),
    /// A web service (GIFs, link check) finished.
    ChatServices(chat::ServicesDone),
    /// The in-game overlay reported, or one of its timers is due (ADR-0037).
    Overlay(chat::OverlayDone),
    /// A check, a download or the daily timer (ADR-0040).
    Updates(updates::UpdatesDone),
}

#[derive(Default)]
pub(crate) struct FlowState {
    /// Launches cancelled before their hand-off.
    pub cancelled: HashSet<LaunchId>,
    /// Read-only connection for statistics queries on the job pool.
    pub stats_db: Arc<Mutex<Option<ReadDb>>>,
    pub steam: steam::SteamState,
    pub chat: chat::ChatState,
    pub services: chat::ServicesState,
    pub overlay: chat::OverlayState,
    pub updates: updates::UpdatesState,
}
