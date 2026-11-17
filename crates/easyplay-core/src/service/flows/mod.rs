//! The flows of LOGIC_SPECS, as `impl AppService` blocks on the service thread. Slow steps
//! run on the job pool and re-enter as internal messages; prompts park the continuation in
//! [`super::prompts`].

pub mod accent;
pub mod artwork;
pub mod availability;
pub mod chat;
pub mod chat_services;
pub mod debug;
pub mod desktop;
pub mod launch;
pub mod maintenance;
pub mod open;
pub mod programs;
pub mod queries;
pub mod remove;
pub mod runtime;
pub mod setup;
pub mod steam;
pub mod tools;

pub use launch::not_installed;

/// Completions of the jobs started by the flows in this module, re-entering the loop as
/// `Internal::Flow`.
pub(crate) enum FlowDone {
    /// A removal's files were collected.
    Removed {
        job: crate::library::RemoveJob,
        files: crate::library::RemoveFiles,
    },
    /// A prefix reset's files were replaced.
    Reset {
        game: crate::ids::GameId,
        result: crate::error::Result<crate::library::prefix_ops::ResetReport>,
    },
    /// A tool session was handed to its supervisor (or failed).
    Tool {
        game: crate::ids::GameId,
        result: crate::error::Result<crate::ids::SessionId>,
    },
    /// A query answered on the job pool.
    Query {
        id: crate::ids::QueryId,
        result: super::event::QueryResult,
    },
    /// Dominant cover colours worked out.
    Accents(Vec<(crate::ids::GameId, Option<[u8; 3]>)>),
    /// Whether each game's main program is there.
    Availability(Vec<(crate::ids::GameId, crate::model::Availability)>),
    /// The start-up maintenance finished.
    Maintenance(maintenance::MaintenanceReport),
    /// A Steam job finished (ADR-0031).
    Steam(steam::SteamDone),
    /// The Chat engine reported, or a restart is due (ADR-0033).
    Chat(chat::ChatDone),
    /// A Chat web service (GIFs, link check) finished.
    ChatServices(chat_services::ServicesDone),
}

/// State of the flows in this module that the AppService keeps between messages.
#[derive(Default)]
pub(crate) struct FlowState {
    /// Launches cancelled before their hand-off.
    pub cancelled: std::collections::HashSet<crate::ids::LaunchId>,
    /// Read-only connection for statistics queries on the job pool.
    pub stats_db: std::sync::Arc<std::sync::Mutex<Option<crate::storage::ReadDb>>>,
    /// Steam games (ADR-0031).
    pub steam: steam::SteamState,
    /// Chat (ADR-0033).
    pub chat: chat::ChatState,
    /// Chat's web services (GIFs, link check).
    pub services: chat_services::ServicesState,
}
