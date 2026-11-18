//! Launch, stop, recover and tool sessions (SVC-005, LOGIC_SPECS §3, §6).
//!
//! The orchestrator (`crate::launch::launch`) runs on the job pool; its states come back as
//! internal messages and are forwarded as `LaunchProgress`. From `Starting` on, the session
//! belongs to its supervisor and the session watcher reports it.

mod cancel;
mod early_failure;
mod ended;
mod failed;
mod job_env;
mod progress;
mod recover;
mod resume;
mod start;
mod stop;
mod tool;

pub use failed::not_installed;
pub(crate) use progress::LaunchDone;
