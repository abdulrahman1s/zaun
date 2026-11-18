//! Run blocking work off the UI and AppService threads with cancellation and panic isolation.
//!
//! Layer 2. A fixed set of worker threads (`clamp(cores − 1, 2, 6)`) serves three priority
//! queues; within a priority jobs run in submission order. `submit` never blocks. A panicking
//! job yields [`JobError::Panicked`] (`ZN-INT-001`) and its worker survives. Each label has a
//! queue bound; beyond it the oldest queued job of that label is dropped
//! ([`JobError::Superseded`]). Knows nothing about the database, egui or domain types.

mod cancel;
mod pool;

pub use cancel::{CancelToken, Cancelled};
pub use pool::{JobHandle, JobPool, JobPoolConfig, default_workers};

use crate::error::{Error, ErrorCode};

/// Queue priority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum JobPriority {
    /// The user is waiting (opening a file, a visible thumbnail).
    Interactive,
    /// Ordinary background work.
    Normal,
    /// Maintenance (audits, clean-up, prefetch).
    Background,
}

impl JobPriority {
    pub(crate) const fn index(self) -> usize {
        match self {
            JobPriority::Interactive => 0,
            JobPriority::Normal => 1,
            JobPriority::Background => 2,
        }
    }
}

/// A short static label naming the kind of job (`"fingerprint"`, `"thumbnail"`…). Used for
/// logging, progress events and per-label queue bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobLabel(pub &'static str);

impl JobLabel {
    /// The label text.
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl std::fmt::Display for JobLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// Why a job produced no value.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum JobError {
    /// Cancelled before it started (a running job reports cancellation through its own
    /// return value).
    #[error("cancelled")]
    Cancelled,
    /// The job panicked.
    #[error("job panicked: {message}")]
    Panicked {
        /// The panic message.
        message: String,
    },
    /// Dropped from the queue because its label's bound was reached.
    #[error("superseded by a newer job")]
    Superseded,
    /// The pool shut down before the job ran.
    #[error("job pool shut down")]
    ShutDown,
}

impl From<JobError> for Error {
    fn from(e: JobError) -> Self {
        let reason = match &e {
            JobError::Cancelled => "cancelled",
            JobError::Panicked { .. } => "panicked",
            JobError::Superseded => "superseded",
            JobError::ShutDown => "shut_down",
        };
        Error::new(ErrorCode::IntTaskFailed)
            .with("reason", reason)
            .with_source(e)
    }
}
