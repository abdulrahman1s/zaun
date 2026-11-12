//! Install `tracing` subscribers; redact; provide per-session log files.
//!
//! Layer 2. Depends on `paths`, `consts` (and `fsx` for the shared append-only log file).
//! Never logs file contents, full environment dumps at `info`, or anything a game printed:
//! game output goes only to the session log (LOGGING §1).

mod redact;
pub mod session_log;
mod setup;

pub use redact::Redactor;
pub use session_log::{
    DropPattern, FramedLine, LineOutcome, LineSplitter, SessionLogConfig, SessionLogWriter,
    sanitize_line,
};
pub use setup::{
    LogFormat, LogGuard, LogOptions, LoggingError, ProcessRole, RecentLines, RotatingFile,
    effective_directive, init,
};
